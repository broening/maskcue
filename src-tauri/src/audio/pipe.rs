//! Brücke zwischen Mikrofon-Strom und Ausgangs-Strom.
//!
//! Zwei Geräte haben zwei Uhren. Sie laufen nie exakt gleich schnell. Deshalb
//! sitzt zwischen beiden ein Ringpuffer. Die Ausgangsseite liest mit einem
//! leicht veränderlichen Verhältnis (Drift-Ausgleich), damit der Füllstand des
//! Puffers um einen Zielwert bleibt. Gleichzeitig rechnet sie die Eingangsrate
//! auf die interne Rate (48 kHz) um.
//!
//! Beide Seiten laufen im Audio-Faden: kein Speicher anlegen, keine Sperren.

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering::Relaxed};

/// Interne Rate des ganzen Tonwegs. DeepFilterNet und Wave Link nutzen 48 kHz.
pub const INTERN_RATE: u32 = 48_000;

/// Glättung des gemessenen Füllstands (Sekunden).
const TAU: f64 = 1.0;
/// Regler: Proportional-Anteil (1/s) und Integral-Anteil (1/s²).
/// KI = (KP/2)² ergibt einen gedämpften Regler ohne Überschwingen.
const KP: f64 = 0.1;
const KI: f64 = 0.002_5;
/// Größte Korrektur des Leseverhältnisses: 3000 ppm (ca. 5 Cent, nicht hörbar).
const MAX_KORREKTUR: f64 = 0.003;
/// Normaler Abstand zwischen zwei Mikrofon-Aufrufen (s).
const NORMAL_LUECKE: f64 = 0.010;
/// Annahme beim Start (s). Viele USB-Mikrofone liefern oft 2 Blöcke auf
/// einmal. Ohne diese Annahme kostet der erste Doppel-Block einen Aussetzer,
/// bevor der Puffer ihn lernen kann (gemessen: 2,3 s nach dem Start).
const START_LUECKE: f64 = 0.020;
/// Reserve über der größten gesehenen Lücke (s).
const RESERVE: f64 = 0.004;
/// Nach einem Aussetzer: so viel mehr Puffer (s).
const AUSSETZER_ZUSCHLAG: f64 = 0.005;
/// So langsam vergisst der Puffer große Lücken (Zeitkonstante, s).
const VERGESSEN: f64 = 600.0;

#[derive(Clone, Copy, Debug)]
pub struct PipeConfig {
    /// Kleinster Ziel-Füllstand des Ringpuffers in Millisekunden.
    pub puffer_ms: f64,
    /// Größter Ziel-Füllstand in Millisekunden.
    pub puffer_max_ms: f64,
    /// Vorlage im Ausgang in Frames. None = 1 Block + 2 ms.
    pub aus_vorlage: Option<u32>,
    /// Nur Stille ausgeben (Messung ohne Ton nach außen).
    pub stumm: bool,
}

impl Default for PipeConfig {
    fn default() -> Self {
        Self { puffer_ms: 10.0, puffer_max_ms: 60.0, aus_vorlage: None, stumm: false }
    }
}

/// Zähler und Messwerte. Der Audio-Faden schreibt, die UI liest.
pub struct Stats {
    /// Ausgang wollte lesen, Puffer war leer.
    pub aussetzer: AtomicU64,
    /// Eingang wollte schreiben, Puffer war voll.
    pub ueberlaeufe: AtomicU64,
    /// Puffer war viel zu voll und wurde auf das Ziel gekürzt.
    pub kuerzungen: AtomicU64,
    /// Fehler, die das Betriebssystem an einen Strom meldet.
    pub strom_fehler: AtomicU64,
    latenz_min_us: AtomicU64,
    latenz_max_us: AtomicU64,
    latenz_summe_us: AtomicU64,
    latenz_anzahl: AtomicU64,
    /// Geglätteter Füllstand in Mikrosekunden.
    pub fuellstand_us: AtomicU64,
    /// Aktuelle Korrektur des Leseverhältnisses in ppm.
    pub korrektur_ppm: AtomicI64,
    /// Aktueller Ziel-Füllstand (µs). Passt sich an die Lücken des Mikrofons an.
    pub ziel_us: AtomicU64,
    /// Größte Lücke zwischen Mikrofon-Aufrufen seit dem letzten Lesen des
    /// Ausgangs (µs). Der Eingang erhöht, der Ausgang liest und setzt auf 0.
    ein_luecke_neu_us: AtomicU64,
    /// Windows hat den Ausgang leer vorgefunden (wir kamen zu spät).
    pub ausgang_leer: AtomicU64,
    /// Beim letzten Aussetzer: Laufzeit (ms), Ziel (µs), Füllstand vor dem
    /// Lesen (µs) und gemerkte Lücke (µs). Zur Fehlersuche.
    pub aussetzer_info: [AtomicU64; 4],
    /// Kleinster Füllstand vor dem Lesen im Messfenster (µs). Zeigt die Reserve.
    fuell_min_us: AtomicU64,
    /// Größte Lücke zwischen zwei Mikrofon-Aufrufen im Messfenster (µs).
    ein_luecke_max_us: AtomicU64,
    /// Zeit von der Aufnahme bis zum Aufruf im Eingang (µs).
    pub ein_verzoegerung_us: AtomicU64,
    /// Zeit vom Aufruf im Ausgang bis zur Wiedergabe (µs).
    pub aus_verzoegerung_us: AtomicU64,
    /// Text des letzten Strom-Fehlers. Nur bei Fehlern beschrieben, nie im Normalbetrieb.
    pub letzter_fehler: Mutex<Option<String>>,
}

/// Latenz-Werte eines Messfensters in Millisekunden.
#[derive(Clone, Copy, Debug)]
pub struct LatenzFenster {
    pub min_ms: f64,
    pub mittel_ms: f64,
    pub max_ms: f64,
    pub anzahl: u64,
}

impl Stats {
    pub fn new() -> Self {
        Self {
            aussetzer: AtomicU64::new(0),
            ueberlaeufe: AtomicU64::new(0),
            kuerzungen: AtomicU64::new(0),
            strom_fehler: AtomicU64::new(0),
            latenz_min_us: AtomicU64::new(u64::MAX),
            latenz_max_us: AtomicU64::new(0),
            latenz_summe_us: AtomicU64::new(0),
            latenz_anzahl: AtomicU64::new(0),
            fuellstand_us: AtomicU64::new(0),
            korrektur_ppm: AtomicI64::new(0),
            ziel_us: AtomicU64::new(0),
            ein_luecke_neu_us: AtomicU64::new(0),
            ausgang_leer: AtomicU64::new(0),
            aussetzer_info: Default::default(),
            fuell_min_us: AtomicU64::new(u64::MAX),
            ein_luecke_max_us: AtomicU64::new(0),
            ein_verzoegerung_us: AtomicU64::new(0),
            aus_verzoegerung_us: AtomicU64::new(0),
            letzter_fehler: Mutex::new(None),
        }
    }

    /// Merkt einen Strom-Fehler. Wird vom Fehler-Aufruf des Betriebssystems benutzt.
    pub fn fehler_merken(&self, text: String) {
        self.strom_fehler.fetch_add(1, Relaxed);
        if let Ok(mut f) = self.letzter_fehler.lock() {
            *f = Some(text);
        }
    }

    fn latenz_merken(&self, us: u64) {
        self.latenz_min_us.fetch_min(us, Relaxed);
        self.latenz_max_us.fetch_max(us, Relaxed);
        self.latenz_summe_us.fetch_add(us, Relaxed);
        self.latenz_anzahl.fetch_add(1, Relaxed);
    }

    /// Setzt alle Zähler auf 0 und gibt die alten Werte zurück
    /// (Aussetzer, Überläufe, Kürzungen, Strom-Fehler).
    /// Gedacht für das Ende der Anlaufphase: Windows meldet beim Start oft eine
    /// Lücke im ersten Mikrofon-Paket.
    pub fn zaehler_zuruecksetzen(&self) -> [u64; 5] {
        [
            self.aussetzer.swap(0, Relaxed),
            self.ausgang_leer.swap(0, Relaxed),
            self.ueberlaeufe.swap(0, Relaxed),
            self.kuerzungen.swap(0, Relaxed),
            self.strom_fehler.swap(0, Relaxed),
        ]
    }

    /// Kleinster Füllstand (ms) seit dem letzten Aufruf.
    pub fn fuell_min_fenster(&self) -> Option<f64> {
        let v = self.fuell_min_us.swap(u64::MAX, Relaxed);
        (v != u64::MAX).then(|| v as f64 / 1000.0)
    }

    /// Größte Lücke zwischen Mikrofon-Aufrufen (ms) seit dem letzten Aufruf.
    pub fn ein_luecke_fenster(&self) -> f64 {
        self.ein_luecke_max_us.swap(0, Relaxed) as f64 / 1000.0
    }

    /// Merkt die Zeit seit dem letzten Mikrofon-Aufruf.
    pub fn ein_luecke_merken(&self, us: u64) {
        self.ein_luecke_max_us.fetch_max(us, Relaxed);
        self.ein_luecke_neu_us.fetch_max(us, Relaxed);
    }

    /// Gibt die Latenz seit dem letzten Aufruf zurück und setzt das Fenster neu.
    pub fn latenz_fenster(&self) -> Option<LatenzFenster> {
        let anzahl = self.latenz_anzahl.swap(0, Relaxed);
        let summe = self.latenz_summe_us.swap(0, Relaxed);
        let min = self.latenz_min_us.swap(u64::MAX, Relaxed);
        let max = self.latenz_max_us.swap(0, Relaxed);
        (anzahl > 0).then(|| LatenzFenster {
            min_ms: min as f64 / 1000.0,
            mittel_ms: summe as f64 / anzahl as f64 / 1000.0,
            max_ms: max as f64 / 1000.0,
            anzahl,
        })
    }
}

/// Erzeugt beide Seiten der Brücke.
///
/// `ein_rate`: Rate des Mikrofons. `aus_rate`: Rate, mit der die Ausgangsseite
/// liest (normal [`INTERN_RATE`]).
pub fn pipe(ein_rate: u32, aus_rate: u32, config: PipeConfig) -> (InputSide, OutputSide, Arc<Stats>) {
    let stats = Arc::new(Stats::new());
    // Eine Sekunde Platz. Normal ist der Puffer nur ein paar Millisekunden voll.
    let (samples_tx, samples_rx) = rtrb::RingBuffer::new(ein_rate as usize);
    let (timing_tx, timing_rx) = rtrb::RingBuffer::new(256);

    let ein = ein_rate as f64;
    let ziel_min = config.puffer_ms / 1000.0;
    let ziel_max = config.puffer_max_ms.max(config.puffer_ms) / 1000.0;
    let ziel = ziel_min * ein;
    let input = InputSide {
        samples: samples_tx,
        timing: timing_tx,
        geschrieben: 0,
        stats: stats.clone(),
        wecker: None,
    };
    let output = OutputSide {
        samples: samples_rx,
        timing: timing_rx,
        letztes_timing: None,
        ein_rate: ein,
        aus_rate: aus_rate as f64,
        nominal: ein / aus_rate as f64,
        ziel,
        obergrenze: obergrenze(ziel, ein),
        ziel_min,
        ziel_max,
        luecke: START_LUECKE,
        zuschlag: 0.0,
        laufzeit: 0.0,
        verlauf: [0.0; 4],
        frac: 0.0,
        gelesen: 0,
        fuellstand: 0.0,
        integral: 0.0,
        laeuft: false,
        stats: stats.clone(),
    };
    (input, output, stats)
}

/// Seite im Mikrofon-Faden.
pub struct InputSide {
    samples: rtrb::Producer<f32>,
    /// (Index des ersten Frames, Aufnahmezeit in ns) pro Aufruf.
    timing: rtrb::Producer<(u64, u64)>,
    geschrieben: u64,
    stats: Arc<Stats>,
    /// Faden, der nach jedem Schreiben geweckt wird (z. B. die Verarbeitung).
    wecker: Option<std::thread::Thread>,
}

impl InputSide {
    /// Nach jedem Schreiben diesen Faden wecken.
    pub fn wecke(&mut self, faden: std::thread::Thread) {
        self.wecker = Some(faden);
    }

    /// Nimmt verschachtelte Frames, mischt sie auf Mono und legt sie in den Puffer.
    /// `aufnahme_ns`: Zeitpunkt, an dem der erste Frame aufgenommen wurde.
    pub fn push<T: Copy>(&mut self, data: &[T], kanaele: usize, aufnahme_ns: u64, zu_f32: impl Fn(T) -> f32) {
        let kanaele = kanaele.max(1);
        let frames = data.len() / kanaele;
        let _ = self.timing.push((self.geschrieben, aufnahme_ns));
        let faktor = 1.0 / kanaele as f32;
        for (i, frame) in data.chunks_exact(kanaele).enumerate() {
            let mono = frame.iter().map(|&s| zu_f32(s)).sum::<f32>() * faktor;
            if self.samples.push(mono).is_err() {
                self.stats.ueberlaeufe.fetch_add(1, Relaxed);
                self.geschrieben += i as u64;
                break;
            }
            if i + 1 == frames {
                self.geschrieben += frames as u64;
            }
        }
        if let Some(w) = &self.wecker {
            w.unpark();
        }
    }
}

/// Ringpuffer für rohe Mikrofon-Samples vor der Verarbeitung.
pub fn roh_ring(rate: u32, stats: Arc<Stats>) -> (InputSide, RohLeser) {
    let (samples_tx, samples_rx) = rtrb::RingBuffer::new(rate as usize);
    let (timing_tx, timing_rx) = rtrb::RingBuffer::new(256);
    let input = InputSide { samples: samples_tx, timing: timing_tx, geschrieben: 0, stats, wecker: None };
    let leser = RohLeser {
        samples: samples_rx,
        timing: timing_rx,
        letztes_timing: None,
        gelesen: 0,
        rate: rate as f64,
        faktor: rate as f64 / INTERN_RATE as f64,
        verlauf: [0.0; 4],
        frac: 0.0,
    };
    (input, leser)
}

/// Liest rohe Mikrofon-Samples blockweise, mit Aufnahmezeit. Liefert immer
/// 48 kHz: Ein Mikrofon mit anderer Rate (z. B. 44,1 kHz) wird hier
/// umgerechnet (kubisch, ca. 2 Samples Verzögerung). Die Effekte brauchen 48 kHz.
pub struct RohLeser {
    samples: rtrb::Consumer<f32>,
    timing: rtrb::Consumer<(u64, u64)>,
    letztes_timing: Option<(u64, u64)>,
    /// Gelesene Eingangs-Samples.
    gelesen: u64,
    rate: f64,
    /// Eingangs-Samples je Ausgangs-Sample (1.0 = 48 kHz, nichts umrechnen).
    faktor: f64,
    /// Die letzten 4 Eingangs-Samples; interpoliert wird zwischen [1] und [2].
    verlauf: [f32; 4],
    frac: f64,
}

impl RohLeser {
    fn umrechnen(&self) -> bool {
        (self.faktor - 1.0).abs() > 1e-9
    }

    /// Wie viele 48-kHz-Samples sicher gelesen werden können.
    pub fn verfuegbar(&self) -> usize {
        let da = self.samples.slots();
        if !self.umrechnen() {
            return da;
        }
        // Für n Ausgaben braucht man höchstens frac + n·faktor neue Eingaben.
        let frei = da as f64 - self.frac;
        if frei < 0.0 { 0 } else { (frei / self.faktor).floor() as usize }
    }

    /// Aufnahmezeit (ns) des Eingangs-Samples an Stelle `pos`.
    fn zeit(&mut self, pos: f64) -> u64 {
        while let Ok(&t) = self.timing.peek() {
            if t.0 as f64 > pos {
                break;
            }
            self.letztes_timing = Some(t);
            let _ = self.timing.pop();
        }
        match self.letztes_timing {
            Some((index, aufnahme)) => (aufnahme as f64 + (pos - index as f64) * 1e9 / self.rate).max(0.0) as u64,
            None => 0,
        }
    }

    /// Füllt `block` ganz (48 kHz). Gibt die Aufnahmezeit des ersten Samples
    /// zurück (ns), oder None, wenn nicht genug Samples da sind.
    pub fn lies_block(&mut self, block: &mut [f32]) -> Option<u64> {
        if !self.umrechnen() {
            let chunk = self.samples.read_chunk(block.len()).ok()?;
            let (a, b) = chunk.as_slices();
            block[..a.len()].copy_from_slice(a);
            block[a.len()..].copy_from_slice(b);
            chunk.commit_all();
            let ns = self.zeit(self.gelesen as f64);
            self.gelesen += block.len() as u64;
            return Some(ns);
        }
        if self.verfuegbar() < block.len() {
            return None;
        }
        let mut ns = 0;
        for (i, s) in block.iter_mut().enumerate() {
            while self.frac >= 1.0 {
                let x = self.samples.pop().unwrap_or(self.verlauf[3]);
                self.verlauf = [self.verlauf[1], self.verlauf[2], self.verlauf[3], x];
                self.gelesen += 1;
                self.frac -= 1.0;
            }
            if i == 0 {
                // verlauf[1] ist Eingangs-Sample Nummer gelesen - 3.
                ns = self.zeit(self.gelesen as f64 - 3.0 + self.frac);
            }
            *s = catmull_rom(self.verlauf, self.frac as f32);
            self.frac += self.faktor;
        }
        Some(ns)
    }
}

/// Seite im Ausgangs-Faden.
pub struct OutputSide {
    samples: rtrb::Consumer<f32>,
    timing: rtrb::Consumer<(u64, u64)>,
    letztes_timing: Option<(u64, u64)>,
    ein_rate: f64,
    aus_rate: f64,
    /// Eingangs-Frames pro Ausgangs-Frame ohne Korrektur.
    nominal: f64,
    /// Ziel-Füllstand in Eingangs-Frames.
    ziel: f64,
    /// Ab diesem Füllstand wird hart gekürzt.
    obergrenze: f64,
    /// Grenzen für das Ziel (s).
    ziel_min: f64,
    ziel_max: f64,
    /// Größte Mikrofon-Lücke, langsam vergessend (s).
    luecke: f64,
    /// Zuschlag nach Aussetzern, langsam vergessend (s).
    zuschlag: f64,
    /// Zeit seit dem Start des Ausgangs (s).
    laufzeit: f64,
    /// Die letzten 4 gelesenen Samples. Interpoliert wird zwischen [1] und [2].
    verlauf: [f32; 4],
    /// Position zwischen verlauf[1] und verlauf[2], 0 bis < 1.
    frac: f64,
    /// Anzahl gelesener Eingangs-Frames.
    gelesen: u64,
    /// Geglätteter Füllstand in Eingangs-Frames.
    fuellstand: f64,
    integral: f64,
    /// false: Puffer füllt sich noch bis zum Ziel, Ausgang ist still.
    laeuft: bool,
    stats: Arc<Stats>,
}

impl OutputSide {
    /// Füllt `out` (verschachtelt, `kanaele` Kanäle) mit dem Mikrofon-Signal.
    /// `wiedergabe_ns`: Zeitpunkt, an dem der erste Frame von `out` abgespielt wird.
    pub fn render(&mut self, out: &mut [f32], kanaele: usize, wiedergabe_ns: u64) {
        let kanaele = kanaele.max(1);
        let frames = out.len() / kanaele;
        while let Ok(t) = self.timing.pop() {
            self.letztes_timing = Some(t);
        }
        let fuell = self.samples.slots() as f64;
        let dt = frames as f64 / self.aus_rate;
        self.ziel_anpassen(dt);
        self.laufzeit += dt;

        if !self.laeuft {
            if fuell < self.ziel {
                out.fill(0.0);
                return;
            }
            self.laeuft = true;
            self.fuellstand = fuell;
            self.integral = 0.0;
        }

        self.stats.fuell_min_us.fetch_min((fuell / self.ein_rate * 1e6) as u64, Relaxed);

        if fuell > self.obergrenze {
            let weg = (fuell - self.ziel) as usize;
            if let Ok(chunk) = self.samples.read_chunk(weg) {
                chunk.commit_all();
                self.gelesen += weg as u64;
            }
            self.fuellstand = self.ziel;
            self.stats.kuerzungen.fetch_add(1, Relaxed);
        }

        // Drift-Ausgleich: Füllstand glätten, Leseverhältnis nachregeln.
        let a = 1.0 - (-dt / TAU).exp();
        self.fuellstand += a * (fuell - self.fuellstand);
        let fehler = (self.fuellstand - self.ziel) / self.ein_rate;
        let grenze = MAX_KORREKTUR / KI;
        self.integral = (self.integral + fehler * dt).clamp(-grenze, grenze);
        let korrektur = (KP * fehler + KI * self.integral).clamp(-MAX_KORREKTUR, MAX_KORREKTUR);
        let verhaeltnis = self.nominal * (1.0 + korrektur);
        self.stats.fuellstand_us.store((self.fuellstand / self.ein_rate * 1e6) as u64, Relaxed);
        self.stats.korrektur_ppm.store((korrektur * 1e6) as i64, Relaxed);

        // Latenz des ersten Frames: Wiedergabezeit minus Aufnahmezeit.
        if let Some((index, aufnahme_ns)) = self.letztes_timing {
            let pos = self.gelesen as f64 - 3.0 + self.frac;
            let aufnahme_pos = aufnahme_ns as f64 + (pos - index as f64) * 1e9 / self.ein_rate;
            let latenz_ns = wiedergabe_ns as f64 - aufnahme_pos;
            if latenz_ns > 0.0 {
                self.stats.latenz_merken((latenz_ns / 1000.0) as u64);
            }
        }

        for (f, frame) in out.chunks_exact_mut(kanaele).enumerate() {
            while self.frac >= 1.0 {
                match self.samples.pop() {
                    Ok(s) => {
                        self.verlauf = [self.verlauf[1], self.verlauf[2], self.verlauf[3], s];
                        self.gelesen += 1;
                        self.frac -= 1.0;
                    }
                    Err(_) => {
                        // Puffer leer: Rest still, neu füllen bis zum Ziel.
                        self.stats.aussetzer.fetch_add(1, Relaxed);
                        let i = &self.stats.aussetzer_info;
                        i[0].store((self.laufzeit * 1000.0) as u64, Relaxed);
                        i[1].store((self.ziel / self.ein_rate * 1e6) as u64, Relaxed);
                        i[2].store((fuell / self.ein_rate * 1e6) as u64, Relaxed);
                        i[3].store((self.luecke * 1e6) as u64, Relaxed);
                        self.laeuft = false;
                        self.zuschlag += AUSSETZER_ZUSCHLAG;
                        out[f * kanaele..].fill(0.0);
                        return;
                    }
                }
            }
            frame.fill(catmull_rom(self.verlauf, self.frac as f32));
            self.frac += verhaeltnis;
        }
    }
}

impl OutputSide {
    /// Ziel-Füllstand an die Lücken des Mikrofons anpassen.
    /// Große Lücke: sofort mehr Puffer. Danach über ca. 10 Minuten wieder weniger.
    fn ziel_anpassen(&mut self, dt: f64) {
        let jetzt = self.stats.ein_luecke_neu_us.swap(0, Relaxed) as f64 / 1e6;
        let vergessen = dt / VERGESSEN;
        if jetzt > self.luecke {
            self.luecke = jetzt;
        } else {
            self.luecke -= (self.luecke - NORMAL_LUECKE).max(0.0) * vergessen;
        }
        self.zuschlag -= self.zuschlag * vergessen;
        // Gemessen wird der Füllstand vor dem Lesen. Er muss die längste Lücke
        // des Mikrofons überbrücken. Dazu kommt ein halber Block, weil die
        // Aufrufe beider Geräte gegeneinander wandern (zwei Uhren).
        let ziel_s = (self.luecke + NORMAL_LUECKE / 2.0 + RESERVE + self.zuschlag).clamp(self.ziel_min, self.ziel_max);
        self.ziel = ziel_s * self.ein_rate;
        self.obergrenze = obergrenze(self.ziel, self.ein_rate);
        self.stats.ziel_us.store((ziel_s * 1e6) as u64, Relaxed);
    }
}

fn obergrenze(ziel: f64, ein_rate: f64) -> f64 {
    ziel + (2.0 * ziel).max(0.05 * ein_rate)
}

/// Kubische Interpolation zwischen x[1] und x[2] an Stelle t (0..1).
#[inline]
fn catmull_rom(x: [f32; 4], t: f32) -> f32 {
    let c1 = 0.5 * (x[2] - x[0]);
    let c2 = x[0] - 2.5 * x[1] + 2.0 * x[2] - 0.5 * x[3];
    let c3 = 0.5 * (x[3] - x[0]) + 1.5 * (x[1] - x[2]);
    ((c3 * t + c2) * t + c1) * t + x[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Einfacher Zufall ohne Abhängigkeit (für Zeit-Zittern im Test).
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    struct Ergebnis {
        stats: Arc<Stats>,
        ausgang: Vec<f32>,
        fuellstand_ms_ende: f64,
    }

    /// Simuliert zwei Geräte mit eigener Uhr.
    /// `drift_ppm`: so viel schneller läuft die Mikrofon-Uhr.
    /// `zittern_ms`: zufällige Verspätung der Aufrufe.
    fn simuliere(ein_rate: u32, drift_ppm: f64, zittern_ms: f64, sekunden: f64, ton_hz: f64) -> Ergebnis {
        let config = PipeConfig { puffer_ms: 20.0, ..Default::default() };
        simuliere_mit(ein_rate, drift_ppm, zittern_ms, sekunden, ton_hz, None, config)
    }

    /// `stau`: (alle x Sekunden, so lange in s) liefert das Mikrofon nichts
    /// und danach alles auf einmal.
    fn simuliere_mit(
        ein_rate: u32,
        drift_ppm: f64,
        zittern_ms: f64,
        sekunden: f64,
        ton_hz: f64,
        stau: Option<(f64, f64)>,
        config: PipeConfig,
    ) -> Ergebnis {
        let (mut ein, mut aus, stats) = pipe(ein_rate, INTERN_RATE, config);
        let mut letzte_lieferung = 0.0f64;
        let block_ein = (ein_rate / 100) as usize; // 10 ms
        let block_aus = (INTERN_RATE / 100) as usize;
        let periode_ein = block_ein as f64 / (ein_rate as f64 * (1.0 + drift_ppm * 1e-6));
        let periode_aus = block_aus as f64 / INTERN_RATE as f64;
        let mut zufall = Lcg(42);
        let mut t_ein = 0.0;
        let mut t_aus = 0.003;
        let mut phase = 0.0f64;
        let mut eingabe = vec![0.0f32; block_ein];
        let mut ausgabe = vec![0.0f32; block_aus * 2];
        let mut ausgang = Vec::new();
        while t_aus < sekunden {
            let mut z_ein = t_ein + zufall.next() * zittern_ms / 1000.0;
            if let Some((alle, dauer)) = stau {
                let start = (t_ein / alle).floor() * alle;
                if t_ein > alle && t_ein - start < dauer {
                    z_ein = z_ein.max(start + dauer);
                }
            }
            let z_aus = t_aus + zufall.next() * zittern_ms / 1000.0;
            if z_ein <= z_aus {
                for s in eingabe.iter_mut() {
                    *s = (phase * std::f64::consts::TAU).sin() as f32;
                    phase = (phase + ton_hz / ein_rate as f64).fract();
                }
                let aufnahme = ((t_ein - periode_ein) * 1e9) as u64;
                stats.ein_luecke_merken(((z_ein - letzte_lieferung).max(0.0) * 1e6) as u64);
                letzte_lieferung = z_ein;
                ein.push(&eingabe, 1, aufnahme, |s| s);
                t_ein += periode_ein;
            } else {
                let wiedergabe = ((t_aus + 0.01) * 1e9) as u64;
                aus.render(&mut ausgabe, 2, wiedergabe);
                if t_aus > sekunden - 2.0 {
                    ausgang.extend(ausgabe.iter().step_by(2));
                }
                t_aus += periode_aus;
            }
        }
        let fuellstand_ms_ende = stats.fuellstand_us.load(Relaxed) as f64 / 1000.0;
        Ergebnis { stats, ausgang, fuellstand_ms_ende }
    }

    fn nulldurchgaenge(x: &[f32]) -> usize {
        x.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count()
    }

    #[test]
    fn drift_30_minuten_ohne_aussetzer() {
        for drift in [-200.0, 0.0, 200.0] {
            let e = simuliere(48_000, drift, 2.0, 1800.0, 440.0);
            assert_eq!(e.stats.aussetzer.load(Relaxed), 0, "Aussetzer bei {drift} ppm");
            assert_eq!(e.stats.ueberlaeufe.load(Relaxed), 0, "Überlauf bei {drift} ppm");
            assert_eq!(e.stats.kuerzungen.load(Relaxed), 0, "Kürzung bei {drift} ppm");
            // Bei Drift wandert die Phase zwischen den Aufrufen beider Geräte.
            // Der gemessene Füllstand pendelt darum bis zu einen halben Block
            // (5 ms) um das Ziel. Wichtig ist: die Latenz wächst nicht.
            assert!(
                (e.fuellstand_ms_ende - 20.0).abs() < 6.0,
                "Füllstand {} ms bei {drift} ppm",
                e.fuellstand_ms_ende
            );
            let l = e.stats.latenz_fenster().expect("Latenz gemessen");
            // Start mit 20 ms Lücken-Annahme: am Anfang bis ca. 47 ms, danach weniger.
            assert!(l.max_ms < 50.0, "Latenz {l:?} bei {drift} ppm");
        }
    }

    #[test]
    fn mikrofon_stau_vergroessert_puffer() {
        // Alle 60 s liefert das Mikrofon 30 ms nichts (wie im echten Test gesehen).
        let e = simuliere_mit(48_000, 100.0, 1.0, 1800.0, 440.0, Some((60.0, 0.030)), PipeConfig::default());
        // Der Puffer wächst vorher mit der gemessenen Lücke. Höchstens 1 Aussetzer.
        let aussetzer = e.stats.aussetzer.load(Relaxed);
        assert!(aussetzer <= 1, "{aussetzer} Aussetzer");
        let ziel_ms = e.stats.ziel_us.load(Relaxed) as f64 / 1000.0;
        assert!(ziel_ms > 30.0 && ziel_ms < 45.0, "Ziel {ziel_ms} ms");
    }

    #[test]
    fn ohne_stau_bleibt_puffer_klein() {
        // Start mit 20 ms Annahme, ohne Doppel-Blöcke schrumpft das Ziel wieder.
        let e = simuliere_mit(48_000, 100.0, 1.0, 1800.0, 440.0, None, PipeConfig::default());
        assert_eq!(e.stats.aussetzer.load(Relaxed), 0);
        let ziel_ms = e.stats.ziel_us.load(Relaxed) as f64 / 1000.0;
        assert!(ziel_ms < 21.0, "Ziel {ziel_ms} ms");
    }

    #[test]
    fn erster_doppelblock_kurz_nach_dem_start_ohne_aussetzer() {
        // Mikrofon liefert erst 2 s gleichmäßig, dann einmal 2 Blöcke auf einmal.
        for drift in [-300.0, 0.0, 300.0] {
            let e = simuliere_mit(48_000, drift, 1.0, 10.0, 440.0, Some((2.3, 0.011)), PipeConfig::default());
            assert_eq!(e.stats.aussetzer.load(Relaxed), 0, "Aussetzer bei {drift} ppm");
        }
    }

    #[test]
    fn anlauf_fuellt_schnell_auf_wenn_ziel_steigt() {
        // Mikrofon liefert von Anfang an oft 2 Blöcke auf einmal (Lücke 20 ms).
        // Der Puffer muss das sofort lernen, ohne Aussetzer.
        for drift in [-300.0, 0.0, 300.0] {
            let e = simuliere_mit(48_000, drift, 1.0, 120.0, 440.0, Some((0.02, 0.012)), PipeConfig::default());
            assert_eq!(e.stats.aussetzer.load(Relaxed), 0, "Aussetzer bei {drift} ppm");
        }
    }

    #[test]
    fn umrechnung_44100_behaelt_tonhoehe() {
        let e = simuliere(44_100, 30.0, 1.0, 20.0, 1000.0);
        assert_eq!(e.stats.aussetzer.load(Relaxed), 0);
        // Letzte 2 s: 1000 Hz → ca. 2000 Nulldurchgänge.
        let n = nulldurchgaenge(&e.ausgang) as f64;
        let sekunden = e.ausgang.len() as f64 / INTERN_RATE as f64;
        let hz = n / sekunden;
        assert!((hz - 1000.0).abs() < 2.0, "gemessen {hz} Hz");
    }

    #[test]
    fn latenz_passt_zum_puffer() {
        let e = simuliere(48_000, 0.0, 0.0, 10.0, 440.0);
        let l = e.stats.latenz_fenster().expect("Latenz gemessen");
        // Puffer 20 ms + 10 ms bis zur Wiedergabe, Aufnahme liegt 10 ms vor dem Aufruf.
        assert!(l.mittel_ms > 30.0 && l.mittel_ms < 45.0, "{l:?}");
    }

    #[test]
    fn roh_leser_rechnet_44100_auf_48000_um() {
        let stats = Arc::new(Stats::new());
        let (mut ein, mut leser) = roh_ring(44_100, stats);
        // 1 s Sinus 1 kHz bei 44,1 kHz, in Paketen à 441 (10 ms), 10 ms Abstand.
        let x: Vec<f32> = (0..44_100).map(|i| 0.5 * (std::f32::consts::TAU * 1000.0 * i as f32 / 44_100.0).sin()).collect();
        let mut y = Vec::new();
        let mut zeiten = Vec::new();
        let mut block = [0.0f32; 480];
        for (k, paket) in x.chunks(441).enumerate() {
            ein.push(paket, 1, 1_000_000_000 + k as u64 * 10_000_000, |s| s);
            while leser.verfuegbar() >= 480 {
                zeiten.push(leser.lies_block(&mut block).expect("Block"));
                y.extend_from_slice(&block);
            }
        }
        // Fast genau 48000 Samples (bis auf den letzten angefangenen Block).
        assert!(y.len() >= 47_040 && y.len() <= 48_000, "{}", y.len());
        // Frequenz bleibt 1 kHz: Nulldurchgänge nach oben je Sekunde.
        let n = y[480..].windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() as f64;
        let hz = n * 48_000.0 / (y.len() - 480) as f64;
        assert!((hz - 1000.0).abs() < 3.0, "{hz} Hz");
        // Pegel bleibt (Spitze ca. 0,5).
        let spitze = y[480..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((spitze - 0.5).abs() < 0.01, "{spitze}");
        // Zeit läuft je Block um 10 ms weiter.
        for w in zeiten[1..].windows(2) {
            let d = w[1] as i64 - w[0] as i64;
            assert!((d - 10_000_000).abs() < 50_000, "{d}");
        }
    }

    #[test]
    fn roh_leser_gibt_aufnahmezeit_pro_block() {
        let stats = Arc::new(Stats::new());
        let (mut ein, mut leser) = roh_ring(48_000, stats);
        // Zwei Pakete à 720 Frames, das zweite 15 ms nach dem ersten aufgenommen.
        ein.push(&vec![0.1f32; 720], 1, 1_000_000_000, |s| s);
        ein.push(&vec![0.2f32; 720], 1, 1_015_000_000, |s| s);
        let mut block = [0.0f32; 480];
        // Block 1: Frames 0..480 → Zeit des ersten Pakets.
        assert_eq!(leser.lies_block(&mut block), Some(1_000_000_000));
        // Block 2: Frames 480..960 → 10 ms nach dem ersten Paket.
        assert_eq!(leser.lies_block(&mut block), Some(1_010_000_000));
        assert!(block[..240].iter().all(|&s| s == 0.1) && block[240..].iter().all(|&s| s == 0.2));
        // Block 3: Frames 960..1440 → 5 ms nach dem zweiten Paket.
        assert_eq!(leser.lies_block(&mut block), Some(1_020_000_000));
        assert_eq!(leser.lies_block(&mut block), None);
    }

    #[test]
    fn leerer_puffer_zaehlt_aussetzer_und_erholt_sich() {
        let (mut ein, mut aus, stats) = pipe(48_000, 48_000, PipeConfig::default());
        let block = vec![0.5f32; 480];
        let mut out = vec![0.0f32; 480];
        for _ in 0..3 {
            ein.push(&block, 1, 0, |s| s);
        }
        for _ in 0..4 {
            aus.render(&mut out, 1, 0);
        }
        assert_eq!(stats.aussetzer.load(Relaxed), 1);
        // Puffer leer: still, bis er wieder bis zum Ziel gefüllt ist.
        aus.render(&mut out, 1, 0);
        assert_eq!(stats.aussetzer.load(Relaxed), 1);
        assert!(out.iter().all(|&s| s == 0.0));
        // Nach dem Aussetzer: Ziel 29 ms + 5 ms Zuschlag. 4 Blöcke (40 ms) reichen.
        for _ in 0..4 {
            ein.push(&block, 1, 0, |s| s);
        }
        aus.render(&mut out, 1, 0);
        assert!(out[100..].iter().all(|&s| (s - 0.5).abs() < 1e-4));
    }
}
