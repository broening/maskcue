//! Figur-Stimme „Leicht“: Tonhöhe mit WSOLA und Neuabtastung, im Zeitbereich.
//!
//! Ein Lesekopf läuft mit dem Faktor a = 2^(HT/12) durch den Eingang. Das
//! tastet neu ab: Tonhöhe und Formanten ändern sich zusammen um a. Die Stimme
//! klingt nach Helium (höher) oder nach Riese (tiefer). Den Formant hält dieses
//! Verfahren nicht fest, der Formant-Wert der Figur gilt hier nicht.
//!
//! Ist a ≠ 1, läuft der Kopf dem Eingang davon (a < 1) oder holt ihn ein
//! (a > 1). Darum liest er in kurzen Stücken. Jedes neue Stück beginnt wieder
//! nahe der Soll-Verzögerung. Wo genau, sucht WSOLA: die Stelle im Suchbereich,
//! deren Wellenform am besten an das alte Stück passt (Kreuzkorrelation). Altes
//! und neues Stück werden über die Überlappung überblendet.
//!
//! Zeit dehnen (WSOLA) und Neuabtastung laufen in einem Schritt: Die Stücke
//! werden beim Lesen gleich neu abgetastet (kubisch). Kein Grundton, keine FFT,
//! darum sehr wenig Rechenzeit.
//!
//! Verzögerung: im Mittel 10–16 ms, höchstens ca. 21 ms. Sie hängt etwas von
//! der Tonhöhe ab (Tests `verzoegerung_gemessen`, `verzoegerung_am_einsatz`).

use std::f32::consts::PI;

use super::pipe::INTERN_RATE;
use super::verarbeitung::Effekt;

/// Bereich der Tonhöhe (Halbtöne). Faktor 0,42 bis 5,66.
pub const HALBTOENE_MIN: f32 = -15.0;
pub const HALBTOENE_MAX: f32 = 30.0;

const RING: usize = 4096;
const MASKE: i64 = RING as i64 - 1;
/// Suchbereich: ± 5 ms um die Soll-Stelle. Deckt eine halbe Schwingung bis
/// 100 Hz ab, so findet die Suche immer eine Stelle in gleicher Phase.
const SUCHE: usize = 240;
/// So weit (Samples) driftet der Lesekopf in einem Stück. Daraus folgt die
/// Stück-Länge (DRIFT / |a − 1|) und der größte Teil der Verzögerung.
/// Gemessen (Test `laengen_vergleichen`, Störabstand im Mittel): 5 ms 28,4 dB
/// (bei −15 Halbtönen nur 24 dB), 10 ms 29,9 dB, 15 und 20 ms auch 29,9 dB,
/// aber 3 und 7 ms mehr Verzögerung. Darum 10 ms.
const DRIFT: f64 = 480.0;
/// Grenzen der Stück-Länge (Samples). Nahe a = 1 wird das Stück lang.
const STUECK_MIN: f64 = 64.0;
const STUECK_MAX: f64 = 1920.0;
/// Überlappung: ein Drittel des Stücks, höchstens 10 ms. Gemessen wie oben:
/// ein Fünftel 29,8 dB (bei −15 Halbtönen 31,5 statt 35,5 dB), die Hälfte
/// 29,8 dB.
const UEBERLAPP: f64 = 0.33;
const UEBERLAPP_MAX: f64 = 480.0;
/// Länge des Vergleichs (Samples Eingang), höchstens 5 ms.
const VERGLEICH_MAX: usize = 240;
/// Grobe Suche: jede 4. Stelle, mit höchstens 48 Punkten je Vergleich. Danach
/// fein um die beste Stelle.
const GROB: usize = 4;
const GROB_PUNKTE: usize = 48;
/// Kleinster Abstand des Lesekopfs zum neuesten Sample (kubisch braucht zwei
/// Samples danach).
const RAND: f64 = 3.0;
/// Butterworth 4. Ordnung als zwei Stufen.
const Q: [f32; 2] = [0.541_196_1, 1.306_563];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WsolaEinstellung {
    pub halbtoene: f32,
    /// Wird nicht beachtet (siehe oben).
    pub formant: f32,
}

/// Tiefpass 2. Ordnung (transponierte Direktform II). Eigener statt
/// `eq::Biquad`: Die Grenze ändert sich im Lauf, der Zustand muss bleiben.
#[derive(Clone, Copy, Default)]
struct Tiefpass {
    b0: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Tiefpass {
    fn setzen(&mut self, hz: f32, q: f32) {
        let w = std::f32::consts::TAU * hz / INTERN_RATE as f32;
        let (c, alpha) = (w.cos(), w.sin() / (2.0 * q));
        let a0 = 1.0 + alpha;
        self.b0 = (1.0 - c) / 2.0 / a0;
        self.a1 = -2.0 * c / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    #[inline]
    fn schritt(&mut self, x: f32) -> f32 {
        // Tiefpass: b1 = 2·b0, b2 = b0.
        let y = self.b0 * x + self.z1;
        self.z1 = 2.0 * self.b0 * x - self.a1 * y + self.z2;
        self.z2 = self.b0 * x - self.a2 * y;
        y
    }
}

/// Wählbare Längen (nur für den Vergleich im Test verschieden).
#[derive(Clone, Copy)]
struct Laengen {
    drift: f64,
    ueberlapp: f64,
}

const STANDARD: Laengen = Laengen { drift: DRIFT, ueberlapp: UEBERLAPP };

/// Längen für einen Faktor (Samples).
#[derive(Clone, Copy)]
struct Masse {
    stueck: usize,
    ueberlapp: usize,
    vergleich: usize,
    /// Kleinste Verzögerung am Anfang eines Stücks. Der Suchbereich reicht
    /// von hier bis 2 · SUCHE darüber.
    d_min: f64,
}

fn masse(a: f64, l: Laengen) -> Masse {
    let drift = (a - 1.0).abs();
    let stueck = if drift > 1e-6 { (l.drift / drift).clamp(STUECK_MIN, STUECK_MAX) } else { STUECK_MAX }.round();
    let ueberlapp = (stueck * l.ueberlapp).min(UEBERLAPP_MAX).round().max(1.0);
    let schritt = stueck - ueberlapp;
    let vergleich = (a * ueberlapp).ceil().clamp(1.0, VERGLEICH_MAX as f64);
    // Bei a > 1 holt der Kopf auf. Das Stück darf den Rand nicht erreichen,
    // und am Ende muss der Vergleich für das nächste Stück schon vorliegen.
    let auf = (a - 1.0).max(0.0);
    let d_min = (RAND + (vergleich + auf * schritt).max(auf * stueck)).ceil();
    Masse { stueck: stueck as usize, ueberlapp: ueberlapp as usize, vergleich: vergleich as usize, d_min }
}

#[cfg(test)]
#[derive(Clone, Copy)]
struct Messung {
    summe: f64,
    zahl: f64,
    min: f64,
    max: f64,
}

#[cfg(test)]
impl Default for Messung {
    fn default() -> Self {
        Self { summe: 0.0, zahl: 0.0, min: f64::MAX, max: 0.0 }
    }
}

pub struct Wsola {
    a: f64,
    laengen: Laengen,
    m: Masse,
    ein: Vec<f32>,
    /// Samples bisher. Ein- und Ausgang laufen im gleichen Takt.
    n: i64,
    /// Lesestelle im Eingang: aktuelles Stück und altes (beim Überblenden).
    pos: f64,
    alt: f64,
    blende: usize,
    blende_laenge: usize,
    bis_naechstes: usize,
    bezug: [f32; VERGLEICH_MAX],
    such: Vec<f32>,
    /// Gegen Aliasing bei a > 1: Grenze 0,45 · 48 kHz / a.
    tiefpass: [Tiefpass; 2],
    /// Alter Tiefpass nach einem Wechsel. Ein harter Wechsel der Grenze ließe
    /// den Filter klingeln (nachgestellt im Test `umschalten_ohne_sprung`),
    /// darum blendet der nächste Block vom alten zum neuen über.
    tiefpass_alt: Option<[Tiefpass; 2]>,
    #[cfg(test)]
    mess: Messung,
}

impl Wsola {
    pub fn new(e: WsolaEinstellung) -> Self {
        Self::mit_laengen(e, STANDARD)
    }

    fn mit_laengen(e: WsolaEinstellung, laengen: Laengen) -> Self {
        let mut w = Self {
            // NaN: `einstellen` rechnet sicher alles neu.
            a: f64::NAN,
            laengen,
            m: masse(1.0, laengen),
            ein: vec![0.0; RING],
            n: 0,
            pos: 0.0,
            alt: 0.0,
            blende: 0,
            blende_laenge: 1,
            bis_naechstes: 0,
            bezug: [0.0; VERGLEICH_MAX],
            such: vec![0.0; 2 * SUCHE + 2 + VERGLEICH_MAX],
            tiefpass: [Tiefpass::default(); 2],
            tiefpass_alt: None,
            #[cfg(test)]
            mess: Messung::default(),
        };
        w.einstellen(e);
        w.zuruecksetzen();
        w
    }

    /// Alles Gehörte vergessen (wie neu, Einstellung bleibt). Ohne Allokation.
    pub fn zuruecksetzen(&mut self) {
        self.ein.fill(0.0);
        self.n = 0;
        for t in self.tiefpass.iter_mut() {
            (t.z1, t.z2) = (0.0, 0.0);
        }
        self.tiefpass_alt = None;
        self.pos = -(self.m.d_min + SUCHE as f64);
        self.alt = self.pos;
        self.blende = 0;
        self.bis_naechstes = self.m.stueck - self.m.ueberlapp;
    }

    /// Tonhöhe ändern. Gilt sofort; der Lesekopf läuft ohne Sprung weiter.
    pub fn einstellen(&mut self, e: WsolaEinstellung) {
        let ht = if e.halbtoene.is_finite() { e.halbtoene.clamp(HALBTOENE_MIN, HALBTOENE_MAX) } else { 0.0 };
        let a = 2f64.powf(ht as f64 / 12.0);
        if a == self.a {
            return;
        }
        self.a = a;
        self.m = masse(a, self.laengen);
        // Ein langes Stück vom alten Faktor früher beenden.
        self.bis_naechstes = self.bis_naechstes.min(self.m.stueck - self.m.ueberlapp);
        let grenze = (0.45 * INTERN_RATE as f64 / a.max(1.0)) as f32;
        if self.tiefpass_alt.is_none() {
            self.tiefpass_alt = Some(self.tiefpass);
        }
        for (t, q) in self.tiefpass.iter_mut().zip(Q) {
            t.setzen(grenze, q);
        }
    }

    fn ein_bei(&self, i: i64) -> f32 {
        if i < 0 || i >= self.n || i < self.n - RING as i64 { 0.0 } else { self.ein[(i & MASKE) as usize] }
    }

    /// Kubische Interpolation (Catmull-Rom). Bei ganzen Stellen genau das Sample.
    fn lesen(&self, t: f64) -> f32 {
        let basis = t.floor();
        let f = (t - basis) as f32;
        let i = basis as i64;
        let (xm, x0, x1, x2) = (self.ein_bei(i - 1), self.ein_bei(i), self.ein_bei(i + 1), self.ein_bei(i + 2));
        let c1 = 0.5 * (x1 - xm);
        let c2 = xm - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm) + 1.5 * (x0 - x1);
        ((c3 * f + c2) * f + c1) * f + x0
    }

    /// Nächstes Stück ab Ausgabe-Stelle `t`: im Suchbereich die Stelle, die am
    /// besten zur Fortsetzung des alten Stücks passt.
    fn neues_stueck(&mut self, t: f64) {
        let m = self.m;
        let d_max = m.d_min + 2.0 * SUCHE as f64;
        let basis = self.pos.floor();
        let frac = self.pos - basis;
        let r0 = basis as i64;
        // Kandidat c: Das Stück beginnt bei c + frac. Gleicher Bruchteil wie das
        // alte, so vergleicht die Suche genau die Samples, die überblendet werden.
        let c_min = (t - d_max - frac).ceil() as i64;
        let c_max = (t - m.d_min - frac).floor() as i64;
        let v = m.vergleich;
        let anzahl = ((c_max - c_min + 1).max(1) as usize).min(self.such.len() - v);
        let fort = r0 - c_min;
        let fort = (0..anzahl as i64).contains(&fort).then_some(fort as usize);
        // Bei 0 Halbtönen bleibt der Kopf, wo er ist: genau das Original, verzögert.
        // (Die Suche könnte sonst bei leisen Einsätzen springen.)
        if let Some(fort) = fort.filter(|_| self.a == 1.0) {
            self.weiter(c_min + fort as i64, frac);
            return;
        }
        for k in 0..v {
            self.bezug[k] = self.ein_bei(r0 + k as i64);
        }
        for k in 0..anzahl + v {
            self.such[k] = self.ein_bei(c_min + k as i64);
        }
        let (bezug, such) = (&self.bezug[..v], &self.such[..]);
        // Kreuzkorrelation, geteilt durch die Energie des Kandidaten.
        let wert = |off: usize, schritt: usize| {
            let (mut kreuz, mut energie) = (0.0f32, 1e-9f32);
            let mut k = 0;
            while k < v {
                let x = such[off + k];
                kreuz += bezug[k] * x;
                energie += x * x;
                k += schritt;
            }
            kreuz / energie.sqrt()
        };
        // Die Fortsetzung zuerst: Bei Gleichstand bleibt sie.
        let mut beste = fort.unwrap_or(anzahl / 2);
        let grob = (v / GROB_PUNKTE).max(1);
        let mut bester = wert(beste, grob);
        for off in (0..anzahl).step_by(GROB) {
            let w = wert(off, grob);
            if w > bester {
                (bester, beste) = (w, off);
            }
        }
        let mitte = beste;
        let mut bester = wert(mitte, 1);
        for off in mitte.saturating_sub(GROB - 1)..=(mitte + GROB - 1).min(anzahl - 1) {
            let w = wert(off, 1);
            if w > bester {
                (bester, beste) = (w, off);
            }
        }
        self.weiter(c_min + beste as i64, frac);
    }

    /// Neues Stück ab Eingang `c + frac`, das alte blendet aus.
    fn weiter(&mut self, c: i64, frac: f64) {
        let m = self.m;
        self.alt = self.pos;
        self.pos = c as f64 + frac;
        self.blende_laenge = m.ueberlapp;
        self.blende = m.ueberlapp;
        self.bis_naechstes = m.stueck - m.ueberlapp;
    }
}

impl Effekt for Wsola {
    fn verarbeite(&mut self, block: &mut [f32]) -> Result<(), String> {
        // 1. Eingang ablegen, gefiltert gegen Aliasing.
        let start = self.n;
        let mut alt = self.tiefpass_alt.take();
        let laenge = block.len() as f32;
        for (k, &s) in block.iter().enumerate() {
            let s = if s.is_finite() { s } else { 0.0 };
            let neu = self.tiefpass[0].schritt(s);
            let mut y = self.tiefpass[1].schritt(neu);
            if let Some(tp) = alt.as_mut() {
                let v = tp[0].schritt(s);
                let w = (k + 1) as f32 / laenge;
                y = w * y + (1.0 - w) * tp[1].schritt(v);
            }
            self.ein[(self.n & MASKE) as usize] = y;
            self.n += 1;
        }
        // 2. Ausgabe: Lesekopf mit Faktor a, neue Stücke nach Plan oder wenn
        //    er (nach einem Wechsel der Tonhöhe) zu nah an den Eingang kommt.
        let a = self.a;
        for (k, aus) in block.iter_mut().enumerate() {
            let t = (start + k as i64) as f64;
            if self.bis_naechstes == 0 || t - self.pos < RAND {
                self.neues_stueck(t);
            }
            let mut y = self.lesen(self.pos);
            #[cfg(test)]
            let mut d = t - self.pos;
            if self.blende > 0 {
                let x = ((self.blende_laenge - self.blende) as f32 + 0.5) / self.blende_laenge as f32;
                let w = 0.5 - 0.5 * (PI * x).cos();
                y = w * y + (1.0 - w) * self.lesen(self.alt);
                #[cfg(test)]
                {
                    d = w as f64 * d + (1.0 - w as f64) * (t - self.alt);
                }
                self.alt += a;
                self.blende -= 1;
            }
            #[cfg(test)]
            {
                let m = &mut self.mess;
                (m.summe, m.zahl, m.min, m.max) = (m.summe + d, m.zahl + 1.0, m.min.min(d), m.max.max(d));
            }
            self.pos += a;
            self.bis_naechstes -= 1;
            *aus = y;
        }
        Ok(())
    }

    /// Mittlere Verzögerung beim aktuellen Faktor: Mitte des Suchbereichs,
    /// dazu die halbe Drift eines Stücks.
    fn verzoegerung(&self) -> usize {
        let mitte = self.m.d_min + SUCHE as f64 - (self.a - 1.0) * self.m.stueck as f64 / 2.0;
        mitte.max(0.0).round() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::denoise::BLOCK;

    const RATE: f32 = INTERN_RATE as f32;
    const HALBTOENE: [f32; 8] = [-15.0, -7.0, -3.0, 0.0, 4.0, 12.0, 24.0, 30.0];

    /// Stimmähnlicher Ton mit leichtem Vibrato (wie in psola.rs).
    fn stimme(f0: f32, sekunden: f32) -> Vec<f32> {
        let mut phase = 0.0f32;
        (0..(sekunden * RATE) as usize)
            .map(|i| {
                let t = i as f32 / RATE;
                let f = f0 * (1.0 + 0.01 * (std::f32::consts::TAU * 5.0 * t).sin());
                phase = (phase + f / RATE).fract();
                (1..20).map(|k| (std::f32::consts::TAU * k as f32 * phase).sin() / k as f32).sum::<f32>() * 0.2
            })
            .collect()
    }

    /// Grundton mit YIN, 40–1600 Hz (auch für +30 und −15 Halbtöne).
    fn grundton(x: &[f32]) -> f32 {
        let (tau_min, tau_max) = ((RATE / 1600.0) as usize, (RATE / 40.0) as usize);
        let w = x.len() - tau_max - 2;
        let mut d = vec![1.0f32; tau_max + 2];
        let mut summe = 0.0f32;
        for tau in 1..tau_max + 2 {
            let s: f32 = (0..w).map(|i| (x[i] - x[i + tau]).powi(2)).sum();
            summe += s;
            d[tau] = if summe > 0.0 { s * tau as f32 / summe } else { 1.0 };
        }
        let mut tau = (tau_min..=tau_max).find(|&t| d[t] < 0.15).unwrap_or_else(|| (tau_min..=tau_max).min_by(|&a, &b| d[a].total_cmp(&d[b])).unwrap());
        while tau < tau_max && d[tau + 1] < d[tau] {
            tau += 1;
        }
        let (a, b, c) = (d[tau - 1], d[tau], d[tau + 1]);
        let nenner = a - 2.0 * b + c;
        let feiner = if nenner.abs() > 1e-9 { tau as f32 + 0.5 * (a - c) / nenner } else { tau as f32 };
        RATE / feiner
    }

    fn verarbeite(w: &mut Wsola, x: &mut [f32]) {
        for b in x.chunks_exact_mut(BLOCK) {
            w.verarbeite(b).unwrap();
        }
    }

    fn einst(halbtoene: f32) -> WsolaEinstellung {
        WsolaEinstellung { halbtoene, formant: 0.0 }
    }

    #[test]
    fn tonhoehe_stimmt() {
        let mut schlimmster = 0.0f32;
        for halbtoene in HALBTOENE {
            for f0 in [120.0, 170.0, 220.0] {
                let mut w = Wsola::new(einst(halbtoene));
                let original = stimme(f0, 1.0);
                let mut x = original.clone();
                verarbeite(&mut w, &mut x);
                // Soll: Ton des Eingangs an der verzögerten Stelle (Vibrato) mal Faktor.
                let d = w.verzoegerung();
                let soll = grundton(&original[30_000 - d..30_000 - d + 4800]) * 2f32.powf(halbtoene / 12.0);
                let ist = grundton(&x[30_000..30_000 + 4800]);
                let fehler = 12.0 * (ist / soll).log2();
                println!("{halbtoene:+} HT bei {f0} Hz: {ist:.1} statt {soll:.1} Hz, Fehler {fehler:+.3} HT");
                schlimmster = schlimmster.max(fehler.abs());
                assert!(fehler.abs() < 0.5, "{halbtoene} HT bei {f0} Hz: {ist} statt {soll}");
            }
        }
        println!("größter Fehler {schlimmster:.3} HT");
    }

    #[test]
    fn ohne_aenderung_kommt_das_original_verzoegert_heraus() {
        let mut w = Wsola::new(einst(0.0));
        let original = stimme(130.0, 1.0);
        let mut x = original.clone();
        verarbeite(&mut w, &mut x);
        // Vergleich mit dem gleichen Tiefpass (bei a = 1: 21,6 kHz, fast nichts).
        let mut tp = [Tiefpass::default(); 2];
        for (t, q) in tp.iter_mut().zip(Q) {
            t.setzen(0.45 * RATE, q);
        }
        let gefiltert: Vec<f32> = original.iter().map(|&s| {
            let s = tp[0].schritt(s);
            tp[1].schritt(s)
        }).collect();
        let d = w.verzoegerung();
        let fehler = (d..x.len()).map(|i| (x[i] - gefiltert[i - d]).abs()).fold(0.0, f32::max);
        assert!(fehler < 1e-5, "Fehler {fehler}");
    }

    #[test]
    fn verzoegerung_gemessen() {
        for halbtoene in HALBTOENE {
            let mut w = Wsola::new(einst(halbtoene));
            let mut x = stimme(150.0, 2.0);
            let (vorlauf, rest) = x.split_at_mut(BLOCK * 50);
            verarbeite(&mut w, vorlauf);
            w.mess = Messung::default();
            verarbeite(&mut w, rest);
            let ms = |s: f64| s / RATE as f64 * 1000.0;
            let m = w.mess;
            let mittel = m.summe / m.zahl;
            println!(
                "{halbtoene:+} HT: mittel {:.1} ms (angegeben {:.1}), min {:.1}, max {:.1} ms",
                ms(mittel),
                ms(w.verzoegerung() as f64),
                ms(m.min),
                ms(m.max)
            );
            assert!(m.min >= RAND, "{halbtoene} HT: Kopf zu nah am Eingang ({})", m.min);
            assert!(ms(m.max) <= 30.0, "{halbtoene} HT: höchstens {} ms", ms(m.max));
            assert!((ms(mittel) - ms(w.verzoegerung() as f64)).abs() < 2.5, "{halbtoene} HT: gemessen {} ms, angegeben {} ms", ms(mittel), ms(w.verzoegerung() as f64));
        }
    }

    /// Von außen gemessen: Wann erscheint ein Ton, der nach Stille einsetzt?
    #[test]
    fn verzoegerung_am_einsatz() {
        for halbtoene in HALBTOENE {
            let mut w = Wsola::new(einst(halbtoene));
            let ton = stimme(150.0, 0.05);
            let mut x = vec![0.0f32; BLOCK * 400];
            // 16 Einsätze, jeder an einer anderen Stelle im Stück-Takt.
            let starts: Vec<usize> = (0..16).map(|k| 14_400 + k * 9_600 + k * 351).collect();
            for &s in &starts {
                x[s..s + ton.len()].copy_from_slice(&ton);
            }
            verarbeite(&mut w, &mut x);
            let ms: Vec<f32> = starts.iter().map(|&s| (s..).find(|&i| x[i].abs() > 0.02).unwrap() - s).map(|d| d as f32 / 48.0).collect();
            let mittel = ms.iter().sum::<f32>() / ms.len() as f32;
            let angegeben = w.verzoegerung() as f32 / 48.0;
            println!("{halbtoene:+} HT: Einsatz nach {mittel:.1} ms (angegeben {angegeben:.1}), {:.1} bis {:.1} ms", ms.iter().cloned().fold(f32::MAX, f32::min), ms.iter().cloned().fold(0.0, f32::max));
            // Der erste Ton kommt mit dem Kopf, der die Stelle zuerst erreicht:
            // eher früher als im Mittel (bei +24: 8 statt 11 ms), nie später.
            assert!(mittel > 2.0 && mittel < angegeben + 2.0, "{halbtoene} HT: {mittel} ms, angegeben {angegeben} ms");
        }
    }

    #[test]
    fn zuruecksetzen_wie_neu() {
        for halbtoene in [-15.0, 7.0, 30.0] {
            let mut w = Wsola::new(einst(halbtoene));
            let mut alt = stimme(200.0, 0.3);
            verarbeite(&mut w, &mut alt);
            w.zuruecksetzen();
            let mut neu = Wsola::new(einst(halbtoene));
            let (mut a, mut b) = (stimme(130.0, 0.3), stimme(130.0, 0.3));
            verarbeite(&mut w, &mut a);
            verarbeite(&mut neu, &mut b);
            assert_eq!(a, b, "{halbtoene} HT: alte Samples nach dem Zurücksetzen");
        }
    }

    /// Größter Sprung zwischen zwei Samples.
    fn groesster_sprung(y: &[f32]) -> f32 {
        y.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
    }

    #[test]
    fn umschalten_ohne_sprung() {
        let folge = [0.0, 12.0, -15.0, 30.0, -7.0, 4.0, 24.0, -3.0, 0.0];
        let mut w = Wsola::new(einst(0.0));
        let x: Vec<f32> = (0..BLOCK * 20 * folge.len()).map(|i| 0.5 * (std::f32::consts::TAU * 200.0 * i as f32 / RATE).sin()).collect();
        let mut y = x.clone();
        for (i, b) in y.chunks_exact_mut(BLOCK).enumerate() {
            if i % 20 == 0 {
                w.einstellen(einst(folge[i / 20]));
            }
            w.verarbeite(b).unwrap();
        }
        // Normaler Sprung beim höchsten Ton (200 Hz · 5,66 = 1132 Hz):
        // 0,5 · 2π · 1132 / 48000 ≈ 0,074. Harter Wechsel wäre bis 1,0.
        let sprung = groesster_sprung(&y[BLOCK * 2..]);
        println!("größter Sprung {sprung:.4}");
        assert!(sprung < 0.1, "Sprung {sprung}");
    }

    #[test]
    fn extreme_werte_bleiben_endlich() {
        let mut w = Wsola::new(einst(30.0));
        // Rauschen (Xorshift) mit einzelnen NaN/∞ und großen Ausschlägen.
        let mut z = 0x1234_5678u32;
        let mut zufall = || {
            z ^= z << 13;
            z ^= z >> 17;
            z ^= z << 5;
            z as f32 / u32::MAX as f32 * 2.0 - 1.0
        };
        let werte = [30.0, -15.0, f32::NAN, 1e9, -1e9, f32::INFINITY, 30.0, -15.0, 0.0, 29.95];
        let mut b = vec![0.0f32; BLOCK];
        for i in 0..2000 {
            w.einstellen(einst(werte[i % werte.len()]));
            for (k, s) in b.iter_mut().enumerate() {
                *s = match (i * BLOCK + k) % 9973 {
                    0 => f32::NAN,
                    1 => f32::INFINITY,
                    _ => zufall(),
                };
            }
            w.verarbeite(&mut b).unwrap();
            assert!(b.iter().all(|v| v.is_finite() && v.abs() < 4.0), "Block {i}: {:?}", b.iter().find(|v| !v.is_finite() || v.abs() >= 4.0));
        }
    }

    /// Störabstand: Energie an den Obertönen von `f0` gegen den Rest (dB),
    /// 60 Hz bis 20 kHz, Hann-Fenster über 1 s.
    fn stoerabstand(y: &[f32], f0: f32) -> f32 {
        let n = 48_000;
        let mut planer = realfft::RealFftPlanner::<f32>::new();
        let fft = planer.plan_fft_forward(n);
        let mut ein: Vec<f32> = y[..n].iter().enumerate().map(|(i, v)| v * (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos())).collect();
        let mut aus = fft.make_output_vec();
        fft.process(&mut ein, &mut aus).unwrap();
        let (mut harm, mut rest) = (0.0f64, 0.0f64);
        for (hz, c) in aus.iter().enumerate().take(20_000).skip(60) {
            let e = c.norm_sqr() as f64;
            let k = (hz as f32 / f0).round().max(1.0);
            if (hz as f32 - k * f0).abs() <= (0.015 * k * f0).max(15.0) {
                harm += e;
            } else {
                rest += e;
            }
        }
        10.0 * (harm / rest.max(1e-12)).log10() as f32
    }

    /// Vergleich der Längen (Grundlage für DRIFT und UEBERLAPP):
    /// cargo test --release laengen_vergleichen -- --ignored --nocapture
    #[test]
    #[ignore]
    fn laengen_vergleichen() {
        let basis: Vec<f32> = [110.0, 160.0, 230.0].iter().map(|&f0| stoerabstand(&stimme(f0, 1.6)[24_000..], f0)).collect();
        println!("Eingang: {basis:?}");
        for (drift, ueberlapp) in [(240.0, 0.33), (480.0, 0.2), (480.0, 0.33), (480.0, 0.5), (720.0, 0.33), (960.0, 0.33)] {
            let mut summe = 0.0;
            let mut zeilen = String::new();
            let mut d_max = 0.0f64;
            for halbtoene in [-15.0, -7.0, -3.0, 4.0, 12.0, 30.0] {
                for f0 in [110.0, 160.0, 230.0] {
                    let mut w = Wsola::mit_laengen(einst(halbtoene), Laengen { drift, ueberlapp });
                    let mut x = stimme(f0, 1.6);
                    verarbeite(&mut w, &mut x);
                    d_max = d_max.max(w.mess.max);
                    let s = stoerabstand(&x[24_000..], f0 * 2f32.powf(halbtoene / 12.0));
                    summe += s;
                    zeilen += &format!(" {s:5.1}");
                }
            }
            println!("Drift {drift}, Überlappung {ueberlapp}: mittel {:.2} dB, max Verzögerung {:.1} ms |{zeilen}", summe / 18.0, d_max / 48.0);
        }
    }
}
