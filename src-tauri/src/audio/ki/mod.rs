//! KI-Stimme: eigene Umsetzung von Beatrice 2 (Project Beatrice, MIT).
//!
//! Liest Modell-Ordner, die der offizielle Beatrice-Trainer erzeugt, und
//! rechnet sie live auf dem Prozessor. Die offizielle Rechen-Bibliothek
//! (beatrice.lib) wird nicht gebraucht.
//!
//! [`KiStimme`] passt das Modell (16 kHz rein, 24 kHz raus) in die
//! MaskCue-Kette (48 kHz, Blöcke zu 10 ms): je Block genau ein Bild.

pub mod beatrice;
pub mod gewichte;
pub mod klang;
pub mod laute;
pub mod netz;
pub mod tonhoehe;

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicBool, AtomicU64};

use beatrice::{Rechner, StimmDaten, StimmPaket};
use windows::Win32::System::Threading::AvSetMmThreadCharacteristicsW;
use windows::core::w;
use serde::{Deserialize, Serialize};

use super::denoise::BLOCK;
use super::verarbeitung::Effekt;

/// Eine Stimme in einer Mischung.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KiAnteil {
    /// Nummer der Stimme im Modell.
    pub nr: usize,
    /// Gewicht (wird auf Summe 1 gebracht).
    pub gewicht: f32,
}

fn eins() -> f32 {
    1.0
}

/// KI-Stimme einer Figur: Modell-Ordner und Mischung.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KiWahl {
    /// Name des Modell-Ordners (in `%APPDATA%\MaskCue\ki-modelle`).
    pub modell: String,
    #[serde(default)]
    pub stimmen: Vec<KiAnteil>,
    /// 1 = Laute ganz vom Ziel-Sprecher, 0 = eigene Aussprache.
    #[serde(default = "eins")]
    pub laut_anteil: f32,
    /// Tonhöhe automatisch auf die Ziel-Stimme setzen; die Tonhöhe der
    /// Figur ist dann nur noch Feinabstimmung.
    #[serde(default = "wahr")]
    pub tonhoehe_auto: bool,
}

fn wahr() -> bool {
    true
}

/// Halbtöne von der eigenen Tonhöhe zur Ziel-Tonhöhe.
pub fn halbtoene_zu(eigene_hz: f32, ziel_hz: f32) -> f32 {
    12.0 * (ziel_hz / eigene_hz).log2()
}

/// Mittlere Tonhöhe (Hz) einer Aufnahme (48 kHz, mono), gemessen mit dem
/// Tonhöhen-Schätzer eines Modells. `None`, wenn kaum Stimme drin ist.
pub fn tonhoehe_messen(pitch_estimator: &Path, x48: &[f32]) -> Result<Option<f32>, String> {
    let mut l = gewichte::Leser::datei(pitch_estimator)?;
    let mut t = tonhoehe::Tonhoehe::lesen(&mut l)?;
    let mut x16 = Vec::new();
    Runter::new().schieben(x48, &mut x16);
    let mut stufen = Vec::new();
    for s in x16 {
        if let Some(b) = t.schieben(s).ton
            && b.stimmlos < 0.5
        {
            stufen.push(b.stufe);
        }
    }
    if stufen.len() < 50 {
        return Ok(None);
    }
    stufen.sort_unstable();
    let m = stufen[stufen.len() / 2] as f32;
    Ok(Some(55.0 * 2f32.powf(m / tonhoehe::STUFEN_JE_OKTAVE as f32)))
}

/// Hörprobe: rechnet eine Aufnahme (48 kHz, mono) mit der Wahl durch.
/// Ergebnis ohne Verzögerung, gleich lang wie der Eingang.
pub fn probe(ordner: &Path, wahl: &KiWahl, halbtoene: f32, formant: f32, x48: &[f32]) -> Result<Vec<f32>, String> {
    let r = vorbereiten_rechner(ordner, wahl, halbtoene, formant)?;
    let mut k = KiKern::new(r);
    let v = k.verzoegerung();
    // Vorne Stille zum Einschwingen, hinten für die Verzögerung.
    let vorne = 48000 / 2;
    let n = (vorne + x48.len() + v).div_ceil(BLOCK) * BLOCK;
    let mut y = vec![0.0f32; n];
    y[vorne..vorne + x48.len()].copy_from_slice(x48);
    for b in y.chunks_exact_mut(BLOCK) {
        k.block(b);
    }
    let start = vorne + v;
    Ok(y[start..start + x48.len()].to_vec())
}

/// Wie schnell rechnet dieser PC die KI-Stimme? Anteil der Zeit, die ein
/// Block (10 ms) zum Rechnen braucht: 0,3 = 3 ms je Block.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct KiTempo {
    /// Durchschnitt über alle Blöcke.
    pub mittel: f32,
    /// Langsamste 5 % der Blöcke (Hänger, die man als Knacken hört).
    pub spitze: f32,
}

/// Rechnet 3 s Testsignal Block für Block und misst die Zeit (wie im
/// KI-Faden, aber ohne ihn). Dauert ca. 1–4 s: nie im Verarbeitungs-Faden.
pub fn tempo_messen(ordner: &Path, wahl: &KiWahl) -> Result<KiTempo, String> {
    let mut k = KiKern::new(vorbereiten_rechner(ordner, wahl, 0.0, 0.0)?);
    // Stimm-ähnliches Signal: Grundton mit Obertönen, langsam gleitend, dazu
    // etwas Rauschen. Stille würde die Netze nicht ehrlich fordern.
    let mut phase = 0.0f32;
    let mut zufall = 1u32;
    let mut signal = |i: usize| {
        let hz = 120.0 + 40.0 * (i as f32 / 48000.0 * 1.3).sin();
        phase = (phase + hz / 48000.0).fract();
        let ton: f32 = (1..6).map(|n| (std::f32::consts::TAU * phase * n as f32).sin() / n as f32).sum();
        zufall = zufall.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        0.15 * ton + 0.02 * ((zufall >> 9) as f32 / (1u32 << 23) as f32 - 0.5)
    };
    let einlauf = 50; // 0,5 s: Puffer und Caches füllen, nicht messen
    let bloecke = 300; // 3 s
    let mut b = vec![0.0f32; BLOCK];
    let mut zeiten = Vec::with_capacity(bloecke);
    for n in 0..einlauf + bloecke {
        for (j, v) in b.iter_mut().enumerate() {
            *v = signal(n * BLOCK + j);
        }
        let t = std::time::Instant::now();
        k.block(&mut b);
        if n >= einlauf {
            zeiten.push(t.elapsed().as_secs_f32());
        }
    }
    let dauer = BLOCK as f32 / 48000.0;
    let mittel = zeiten.iter().sum::<f32>() / zeiten.len() as f32 / dauer;
    zeiten.sort_unstable_by(f32::total_cmp);
    let spitze = zeiten[zeiten.len() * 95 / 100] / dauer;
    Ok(KiTempo { mittel, spitze })
}

/// Ist der Name als Ordner-Name unter `ki-modelle` erlaubt? Kein Pfad,
/// keine Sonderzeichen (der Name kommt auch aus fremden Paketen).
pub fn modell_name_ok(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n != "." && n != ".." && n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '))
}

impl KiWahl {
    /// Werte in den erlaubten Bereich bringen. `None`, wenn der Modell-Name nicht passt.
    pub fn sauber(self) -> Option<KiWahl> {
        if !modell_name_ok(&self.modell) {
            return None;
        }
        let stimmen = self
            .stimmen
            .into_iter()
            .filter(|a| a.gewicht.is_finite() && a.gewicht > 0.0 && a.nr < 10_000)
            .take(8)
            .map(|a| KiAnteil { nr: a.nr, gewicht: a.gewicht.min(1.0) })
            .collect();
        let laut_anteil = if self.laut_anteil.is_finite() { self.laut_anteil.clamp(0.0, 1.0) } else { 1.0 };
        Some(KiWahl { modell: self.modell, stimmen, laut_anteil, tonhoehe_auto: self.tonhoehe_auto })
    }
}

/// Lädt das Modell und stellt die Mischung ein. Dauert ca. 0,2 s: nie im
/// Verarbeitungs-Faden aufrufen.
pub fn vorbereiten(ordner: &Path, wahl: &KiWahl, halbtoene: f32, formant: f32) -> Result<KiStimme, String> {
    Ok(KiStimme::new(vorbereiten_rechner(ordner, wahl, halbtoene, formant)?))
}

fn vorbereiten_rechner(ordner: &Path, wahl: &KiWahl, halbtoene: f32, formant: f32) -> Result<Rechner, String> {
    let mut r = Rechner::laden(ordner)?;
    let mischung: Vec<(usize, f32)> = wahl.stimmen.iter().map(|a| (a.nr, a.gewicht)).collect();
    r.mischung_setzen(&mischung);
    r.tonhoehe_setzen(halbtoene);
    r.formant_setzen(formant);
    r.laut_anteil_setzen(wahl.laut_anteil);
    Ok(r)
}

/// Tiefpass (gefenstertes sinc, Blackman). `fc` relativ zur Abtastrate.
fn tiefpass(taps: usize, fc: f32, verstaerkung: f32) -> Vec<f32> {
    let m = (taps - 1) as f32;
    let mut h: Vec<f32> = (0..taps)
        .map(|i| {
            let x = i as f32 - m / 2.0;
            let sinc = if x == 0.0 { 2.0 * fc } else { (std::f32::consts::TAU * fc * x).sin() / (std::f32::consts::PI * x) };
            let w = 0.42 - 0.5 * (std::f32::consts::TAU * i as f32 / m).cos() + 0.08 * (2.0 * std::f32::consts::TAU * i as f32 / m).cos();
            sinc * w
        })
        .collect();
    let summe: f32 = h.iter().sum();
    for v in &mut h {
        *v *= verstaerkung / summe;
    }
    h
}

const TAPS: usize = 96;

/// 48 kHz → 16 kHz.
struct Runter {
    h: Vec<f32>,
    verlauf: Vec<f32>,
    phase: usize,
}

impl Runter {
    fn new() -> Self {
        Self { h: tiefpass(TAPS, 7200.0 / 48000.0, 1.0), verlauf: vec![0.0; TAPS], phase: 0 }
    }

    fn schieben(&mut self, x: &[f32], aus: &mut Vec<f32>) {
        for &s in x {
            self.verlauf.copy_within(1.., 0);
            self.verlauf[TAPS - 1] = s;
            self.phase += 1;
            if self.phase == 3 {
                self.phase = 0;
                aus.push(netz::punkt(&self.verlauf, &self.h));
            }
        }
    }
}

/// 24 kHz → 48 kHz.
struct Hoch {
    /// Zwei Teil-Filter (gerade und ungerade Taps).
    h: [Vec<f32>; 2],
    verlauf: Vec<f32>,
}

impl Hoch {
    fn new() -> Self {
        let h = tiefpass(TAPS, 11000.0 / 48000.0, 2.0);
        // y[2n+p] = Σ_k h[2k+p'] x[n-k]: Teilfilter mit umgekehrter Reihenfolge für `punkt`.
        let teil = |p: usize| -> Vec<f32> {
            let mut t: Vec<f32> = h.iter().skip(p).step_by(2).copied().collect();
            t.reverse();
            t
        };
        Self { h: [teil(0), teil(1)], verlauf: vec![0.0; TAPS / 2] }
    }

    fn schieben(&mut self, x: &[f32], aus: &mut VecDeque<f32>) {
        let n = TAPS / 2;
        for &s in x {
            self.verlauf.copy_within(1.., 0);
            self.verlauf[n - 1] = s;
            aus.push_back(netz::punkt(&self.verlauf, &self.h[0]));
            aus.push_back(netz::punkt(&self.verlauf, &self.h[1]));
        }
    }
}

/// Für Tests: die beiden Filter als Funktionen.
#[doc(hidden)]
pub fn filter_fuer_test() -> (impl Fn(&[f32]) -> Vec<f32>, impl Fn(&[f32]) -> Vec<f32>) {
    (
        |x: &[f32]| {
            let mut r = Runter::new();
            let mut a = Vec::new();
            r.schieben(x, &mut a);
            a
        },
        |x: &[f32]| {
            let mut h = Hoch::new();
            let mut a = VecDeque::new();
            h.schieben(x, &mut a);
            a.into_iter().collect()
        },
    )
}

/// So viele Nullen (16 kHz) kommen vorne dazu, damit jedes Bild genau am
/// Ende eines Blocks fertig wird (das Netz schaut 40 Samples voraus).
const AUSRICHTUNG: usize = 120;

/// KI-Stimme ohne eigenen Faden: 48 kHz rein, 48 kHz raus, je Block genau
/// ein Bild. Gleiches Ergebnis wie der Kern, nur verschoben (für Tests).
pub struct KiKern {
    rechner: Rechner,
    runter: Runter,
    hoch: Hoch,
    ein16: Vec<f32>,
    aus24: Vec<f32>,
    fifo: VecDeque<f32>,
}

impl KiKern {
    pub fn new(mut rechner: Rechner) -> Self {
        let mut aus24 = Vec::new();
        rechner.schieben(&[0.0; AUSRICHTUNG], &mut aus24);
        Self { rechner, runter: Runter::new(), hoch: Hoch::new(), ein16: Vec::with_capacity(BLOCK), aus24, fifo: VecDeque::with_capacity(4 * BLOCK) }
    }

    pub fn rechner(&mut self) -> &mut Rechner {
        &mut self.rechner
    }

    /// Verzögerung in Samples (48 kHz): [`KERN_VERZOEGERUNG`] plus Vorgriff der Laute.
    pub fn verzoegerung(&self) -> usize {
        KERN_VERZOEGERUNG + self.rechner.vorgriff() * BLOCK
    }

    pub fn block(&mut self, block: &mut [f32]) {
        self.ein16.clear();
        self.runter.schieben(block, &mut self.ein16);
        self.rechner.schieben(&self.ein16, &mut self.aus24);
        self.hoch.schieben(&self.aus24, &mut self.fifo);
        self.aus24.clear();
        // Beim Anlauf kommen mehrere Bilder auf einmal: Überschuss weg, damit
        // die Verzögerung nicht wächst.
        while self.fifo.len() > block.len() {
            self.fifo.pop_front();
        }
        let fehlt = block.len() - self.fifo.len();
        block[..fehlt].fill(0.0);
        for (z, v) in block[fehlt..].iter_mut().zip(self.fifo.drain(..)) {
            *z = v;
        }
    }
}

/// Verzögerung von [`KiKern`] gegenüber dem Trainer (der die ganze Datei auf
/// einmal rechnet), in Samples bei 48 kHz: Filter runter 47,5 + Ausrichtung
/// 120 × 3 + Klang-Vorlauf 120 × 2 + Warten auf das Bild 480 × 2 + Filter
/// hoch 47,5 = 1655 (34,5 ms). Geprüft mit examples/ki_takt.rs.
pub const KERN_VERZOEGERUNG: usize = 1655;

/// Verzögerung von [`KiStimme`]: ein Block mehr, weil der eigene Faden den
/// Block parallel rechnet (44,5 ms; mit Vorgriff der Laute mehr, siehe
/// [`KiKern::verzoegerung`]).
pub const VERZOEGERUNG: usize = KERN_VERZOEGERUNG + BLOCK;

/// Platz in den Warteschlangen zum Faden (Blöcke): reicht für einen Hänger
/// von 0,6 s.
const PUFFER_BLOECKE: usize = 64;

/// Was im KI-Faden rechnet.
enum Werk {
    Beatrice(KiKern),
    /// Für Tests: eigenes Rechenwerk (z. B. mit künstlicher Pause).
    #[cfg(test)]
    Probe(Box<dyn FnMut(&mut [f32]) + Send>),
}

impl Werk {
    fn block(&mut self, b: &mut [f32]) {
        match self {
            Werk::Beatrice(k) => k.block(b),
            #[cfg(test)]
            Werk::Probe(f) => f(b),
        }
    }
}

enum KiBefehl {
    Einstellen { halbtoene: f32, formant: f32, laut_anteil: f32 },
    Paket(Box<StimmPaket>),
}

/// Alle KI-Blöcke, die nicht rechtzeitig fertig waren (für die Warnung).
pub static ZU_SPAET_GESAMT: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct Geteilt {
    stop: AtomicBool,
    /// Blöcke, die nicht rechtzeitig fertig waren (dann kam Stille).
    zu_spaet: AtomicU64,
}

/// KI-Stimme mit eigenem Faden (Windows-Vorrang „Pro Audio“). Sie rechnet
/// Block b, während der Ton-Faden schon den nächsten holt: Die KI bremst so
/// die übrigen Effekte nicht, und ein zweiter Prozessor-Kern hilft mit.
/// Ist ein Block nicht rechtzeitig fertig, kommt Stille (die Verzögerung
/// bleibt immer gleich).
pub struct KiStimme {
    ein: rtrb::Producer<f32>,
    aus: rtrb::Consumer<f32>,
    befehle: rtrb::Producer<KiBefehl>,
    faden: Option<std::thread::JoinHandle<()>>,
    geteilt: Arc<Geteilt>,
    /// Nur Beatrice: Stimm-Daten zum Vorrechnen neuer Mischungen.
    daten: Option<Arc<StimmDaten>>,
    /// Blöcke, die hineingingen bzw. herausgeholt wurden.
    rein: u64,
    raus: u64,
    /// Blöcke Puffer, die der Faden zum Rechnen hat (Beatrice 1).
    nachlauf: u64,
    verzoegerung: usize,
    /// Alle Aufrufe bisher (auch weggefallene Blöcke).
    gesehen: u64,
}

impl KiStimme {
    pub fn new(rechner: Rechner) -> Self {
        let daten = rechner.stimm_daten();
        let kern = KiKern::new(rechner);
        let v = kern.verzoegerung() + BLOCK;
        Self::starten(Werk::Beatrice(kern), Some(daten), 1, v)
    }

    fn starten(mut werk: Werk, daten: Option<Arc<StimmDaten>>, nachlauf: u64, verzoegerung: usize) -> Self {
        let (ein, mut ein_rx) = rtrb::RingBuffer::<f32>::new(PUFFER_BLOECKE * BLOCK);
        // Ausgang größer als Eingang: Holt der Faden nach einem Hänger auf,
        // darf kein fertiger Block wegfallen (sonst verschiebt sich alles).
        let (mut aus_tx, aus) = rtrb::RingBuffer::<f32>::new(2 * PUFFER_BLOECKE * BLOCK);
        let (befehle, mut befehle_rx) = rtrb::RingBuffer::<KiBefehl>::new(32);
        let geteilt = Arc::new(Geteilt::default());
        let g = geteilt.clone();
        let faden = std::thread::Builder::new()
            .name("maskcue-ki".into())
            .spawn(move || {
                let mut task = 0u32;
                // SAFETY: Windows-Aufruf ohne Zeiger außer `task`.
                let _ = unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut task) };
                let mut block = vec![0.0f32; BLOCK];
                while !g.stop.load(Relaxed) {
                    while let Ok(b) = befehle_rx.pop() {
                        match (b, &mut werk) {
                            (KiBefehl::Einstellen { halbtoene, formant, laut_anteil }, Werk::Beatrice(kern)) => {
                                let r = kern.rechner();
                                r.tonhoehe_setzen(halbtoene);
                                r.formant_setzen(formant);
                                r.laut_anteil_setzen(laut_anteil);
                            }
                            (KiBefehl::Paket(p), Werk::Beatrice(kern)) => kern.rechner().paket_setzen(*p),
                            #[cfg(test)]
                            (_, Werk::Probe(_)) => {}
                        }
                    }
                    if ein_rx.slots() >= BLOCK {
                        if let Ok(c) = ein_rx.read_chunk(BLOCK) {
                            let (a, b) = c.as_slices();
                            block[..a.len()].copy_from_slice(a);
                            block[a.len()..].copy_from_slice(b);
                            c.commit_all();
                        }
                        werk.block(&mut block);
                        if let Ok(mut c) = aus_tx.write_chunk(BLOCK) {
                            let (a, b) = c.as_mut_slices();
                            let n = a.len();
                            a.copy_from_slice(&block[..n]);
                            b.copy_from_slice(&block[n..]);
                            c.commit_all();
                        }
                        continue;
                    }
                    std::thread::park_timeout(std::time::Duration::from_millis(20));
                }
            })
            .expect("KI-Faden starten");
        Self { ein, aus, befehle, faden: Some(faden), geteilt, daten, rein: 0, raus: 0, nachlauf, verzoegerung, gesehen: 0 }
    }

    /// Stimm-Daten des Modells (zum Vorrechnen neuer Mischungen).
    pub fn stimm_daten(&self) -> Option<Arc<StimmDaten>> {
        self.daten.clone()
    }

    pub fn einstellen(&mut self, halbtoene: f32, formant: f32, laut_anteil: f32) {
        let _ = self.befehle.push(KiBefehl::Einstellen { halbtoene, formant, laut_anteil });
    }

    /// Fertige Mischung setzen (gilt ab dem nächsten Block).
    pub fn paket(&mut self, p: Box<StimmPaket>) {
        let _ = self.befehle.push(KiBefehl::Paket(p));
    }

    /// Blöcke, die nicht rechtzeitig fertig waren.
    pub fn zu_spaet(&self) -> u64 {
        self.geteilt.zu_spaet.load(Relaxed)
    }

    /// So viele Blöcke läuft die Stimme mit, bevor man sie hört (siehe
    /// `figur.rs`).
    pub fn warm_bloecke(&self) -> usize {
        30
    }

    /// Zu späte Blöcke zählen, aber nicht beim Einlaufen (die hört man nicht,
    /// und sie sollen keine Warnung auslösen).
    fn zu_spaet_zaehlen(&mut self) {
        if self.gesehen > self.warm_bloecke() as u64 {
            self.geteilt.zu_spaet.fetch_add(1, Relaxed);
            ZU_SPAET_GESAMT.fetch_add(1, Relaxed);
        }
    }
}

impl Effekt for KiStimme {
    fn verarbeite(&mut self, block: &mut [f32]) -> Result<(), String> {
        self.gesehen += 1;
        // Block an den Faden geben. Ist die Warteschlange voll (Faden hängt),
        // fällt der Block weg und zählt nicht: sonst passt die Zählung nie
        // wieder zu dem, was der Faden zurückgibt.
        if let Ok(mut c) = self.ein.write_chunk(block.len()) {
            let (a, b) = c.as_mut_slices();
            let n = a.len();
            a.copy_from_slice(&block[..n]);
            b.copy_from_slice(&block[n..]);
            c.commit_all();
            self.rein += 1;
        } else {
            self.zu_spaet_zaehlen();
            block.fill(0.0);
            return Ok(());
        }
        if let Some(f) = &self.faden {
            f.thread().unpark();
        }
        // Gebraucht wird der Block von vor `nachlauf` Blöcken. Was zu spät kam, fliegt weg.
        let ziel = self.rein.saturating_sub(1 + self.nachlauf);
        while self.raus < ziel && self.aus.slots() >= block.len() {
            if let Ok(c) = self.aus.read_chunk(block.len()) {
                c.commit_all();
            }
            self.raus += 1;
        }
        let bereit = self.rein > self.nachlauf;
        if bereit && self.raus == ziel && self.aus.slots() >= block.len() {
            if let Ok(c) = self.aus.read_chunk(block.len()) {
                let (a, b) = c.as_slices();
                block[..a.len()].copy_from_slice(a);
                block[a.len()..].copy_from_slice(b);
                c.commit_all();
            }
            self.raus += 1;
        } else {
            if bereit {
                self.zu_spaet_zaehlen();
            }
            block.fill(0.0);
        }
        Ok(())
    }

    fn verzoegerung(&self) -> usize {
        self.verzoegerung
    }
}

impl Drop for KiStimme {
    fn drop(&mut self) {
        self.geteilt.stop.store(true, Relaxed);
        if let Some(f) = self.faden.take() {
            f.thread().unpark();
            let _ = f.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Der Faden hängt am Anfang (z. B. der Prozessor ist kurz belegt).
    /// Danach muss jeder Block wieder pünktlich und an der
    /// richtigen Stelle kommen: Ausgabe = Eingang von vor `nachlauf` Blöcken.
    #[test]
    fn faengt_sich_nach_haenger_am_anfang() {
        haenger(400);
    }

    /// Hänger länger als die Warteschlange: Blöcke fallen weg, danach muss es
    /// trotzdem wieder stimmen.
    #[test]
    fn faengt_sich_nach_langem_haenger() {
        haenger(1000);
    }

    fn haenger(ms: u64) {
        let mut erster = true;
        let werk = Werk::Probe(Box::new(move |_b: &mut [f32]| {
            if erster {
                erster = false;
                std::thread::sleep(std::time::Duration::from_millis(ms));
            }
        }));
        let nachlauf = 4;
        let mut k = KiStimme::starten(werk, None, nachlauf, 0);
        let start = std::time::Instant::now();
        let mut falsch = Vec::new();
        for i in 0..300u64 {
            // Block i trägt seine Nummer.
            let mut b = vec![i as f32; BLOCK];
            k.verarbeite(&mut b).unwrap();
            if i >= 150 && b[0] != (i - nachlauf) as f32 {
                falsch.push((i, b[0]));
            }
            if let Some(rest) = std::time::Duration::from_millis(10 * (i + 1)).checked_sub(start.elapsed()) {
                std::thread::sleep(rest);
            }
        }
        assert!(falsch.is_empty(), "falsche oder fehlende Blöcke nach dem Hänger: {:?}", &falsch[..falsch.len().min(10)]);
    }
}
