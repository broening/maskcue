//! Figur-Stimme mit PSOLA (Pitch-Synchronous Overlap-Add), im Zeitbereich.
//!
//! Ablauf je Block:
//! 1. Grundton finden (YIN) im neuesten Stück Mikrofon.
//! 2. Analyse-Marken setzen: im Abstand einer Schwingung (stimmhaft) oder
//!    5 ms (stimmlos, z. B. „s“, „f“).
//! 3. Synthese-Marken im neuen Abstand setzen (Schwingung / Pitch-Faktor).
//!    An jede Synthese-Marke kommt das Stück (2 Schwingungen, Hann-Fenster)
//!    der nächsten Analyse-Marke.
//! 4. Formant: jedes Stück wird vor dem Einsetzen gestreckt oder gestaucht.
//!
//! Verzögerung: so viel, dass jedes Stück ganz vorliegt (ca. 30 ms).

use super::pipe::INTERN_RATE;
use super::verarbeitung::Effekt;

const RING: usize = 8192;
const MASKE: i64 = RING as i64 - 1;
/// Tiefster und höchster erkannter Grundton (Hz).
const F0_MIN: f32 = 90.0;
const F0_MAX: f32 = 600.0;
/// YIN: Fensterlänge (Samples) und Schwelle für „stimmhaft“.
const YIN_FENSTER: usize = 512;
const YIN_SCHWELLE: f32 = 0.15;
const STIMMHAFT_GRENZE: f32 = 0.3;
/// Unter diesem Pegel gilt ein Stück als stimmlos (dBFS).
const PEGEL_MIN_DB: f32 = -50.0;
/// Abstand der Marken bei stimmlosen Lauten (Samples, 5 ms).
const STIMMLOS_SCHRITT: f32 = 240.0;
/// Kleinster Formant-Faktor (−4 Halbtöne). Bestimmt die Verzögerung mit.
const FORMANT_MIN: f32 = 0.79;
/// Tiefe Stimmen (bis −12 Halbtöne): Die Stücke liegen weiter auseinander, als
/// sie lang sind, die Stimme „pulst“ (roboterhaft). Dagegen: Stücke etwas
/// länger (füllt die Lücken teils) und kleine Schwankungen in Abstand und
/// Lautstärke wie bei einer echten Stimme. Werte bei −12 Halbtönen; bis −0
/// nehmen sie gleichmäßig ab.
const TIEF_LAENGER: f32 = 0.35;
const TIEF_SCHWANKUNG: f32 = 0.025;
const TIEF_LAUTSTAERKE: f32 = 0.08;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PsolaEinstellung {
    pub halbtoene: f32,
    pub formant: f32,
}

#[derive(Clone, Copy, Default)]
struct Erkennung {
    mitte: i64,
    periode: f32,
    stimmhaft: bool,
}

#[derive(Clone, Copy, Default)]
struct Marke {
    /// Mitte des Stücks im Eingang (Samples, mit Bruchteil).
    pos: f64,
    periode: f32,
    stimmhaft: bool,
}

pub struct Psola {
    alpha: f32,
    beta: f32,
    ein: Vec<f32>,
    aus: Vec<f32>,
    n_ein: i64,
    verzoegerung: i64,
    // Grundton
    yin_puffer: Vec<f32>,
    yin_d: Vec<f32>,
    erkennungen: [Erkennung; 64],
    n_erkennungen: usize,
    letzte_perioden: [f32; 3],
    // Marken
    marken: [Marke; 128],
    n_marken: usize,
    naechste_analyse: f64,
    naechste_synthese: f64,
    /// Zufall für die Schwankungen (Xorshift, fester Start: gleiche Eingabe, gleiche Ausgabe).
    zufall: u32,
}

fn p_max() -> f32 {
    INTERN_RATE as f32 / F0_MIN
}
fn p_min() -> f32 {
    INTERN_RATE as f32 / F0_MAX
}

impl Psola {
    pub fn new(e: PsolaEinstellung) -> Self {
        // Halbe Stück-Länge nach dem Formant-Strecken höchstens p_max/β_min.
        // Ein Stück braucht Eingang bis Marke + 1 Schwingung, die Marke liegt bis
        // zu einer halben Schwingung neben der Synthese-Marke.
        let verzoegerung = (p_max() / FORMANT_MIN + 1.5 * p_max()).ceil() as i64;
        let tau_max = p_max().ceil() as usize + 2;
        let mut p = Self {
            alpha: 1.0,
            beta: 1.0,
            ein: vec![0.0; RING],
            aus: vec![0.0; RING],
            n_ein: 0,
            verzoegerung,
            yin_puffer: vec![0.0; YIN_FENSTER + tau_max + 1],
            yin_d: vec![0.0; tau_max + 1],
            erkennungen: [Erkennung::default(); 64],
            n_erkennungen: 0,
            letzte_perioden: [0.0; 3],
            marken: [Marke::default(); 128],
            n_marken: 0,
            naechste_analyse: 0.0,
            naechste_synthese: 0.0,
            zufall: 0x2545_f491,
        };
        p.einstellen(e);
        p
    }

    pub fn einstellen(&mut self, e: PsolaEinstellung) {
        self.alpha = 2f32.powf(e.halbtoene / 12.0);
        self.beta = 2f32.powf(e.formant / 12.0).max(FORMANT_MIN);
    }

    /// Alles Gehörte vergessen (wie neu, Einstellung bleibt). Ohne Allokation.
    pub fn zuruecksetzen(&mut self) {
        self.ein.fill(0.0);
        self.aus.fill(0.0);
        self.n_ein = 0;
        self.yin_puffer.fill(0.0);
        self.yin_d.fill(0.0);
        self.n_erkennungen = 0;
        self.letzte_perioden = [0.0; 3];
        self.n_marken = 0;
        self.naechste_analyse = 0.0;
        self.naechste_synthese = 0.0;
        self.zufall = 0x2545_f491;
    }

    /// So viele Samples nach dem Zurücksetzen kann der Grundton noch falsch sein:
    /// Das YIN-Fenster enthält dann noch Nullen von vorher.
    pub fn anlauf(&self) -> usize {
        self.yin_puffer.len()
    }

    /// Zufallszahl −1…1.
    fn zufall(&mut self) -> f32 {
        let mut x = self.zufall;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.zufall = x;
        x as f32 / u32::MAX as f32 * 2.0 - 1.0
    }

    fn ein_bei(&self, i: i64) -> f32 {
        if i < 0 || i >= self.n_ein { 0.0 } else { self.ein[(i & MASKE) as usize] }
    }

    /// Lineare Interpolation im Eingang.
    fn ein_frac(&self, t: f64) -> f32 {
        let i = t.floor() as i64;
        let f = (t - i as f64) as f32;
        self.ein_bei(i) * (1.0 - f) + self.ein_bei(i + 1) * f
    }

    /// YIN auf dem neuesten Stück. Gibt (Periode, stimmhaft) zurück.
    fn grundton(&mut self) -> (f32, bool) {
        let tau_max = self.yin_d.len() - 1;
        let n = self.yin_puffer.len();
        let start = self.n_ein - n as i64;
        for k in 0..n {
            self.yin_puffer[k] = self.ein_bei(start + k as i64);
        }
        let x = &self.yin_puffer;
        let pegel = (x[..YIN_FENSTER].iter().map(|v| v * v).sum::<f32>() / YIN_FENSTER as f32).sqrt();
        if 20.0 * pegel.max(1e-9).log10() < PEGEL_MIN_DB {
            return (0.0, false);
        }
        // Differenzfunktion und kumulierte normierte Form.
        self.yin_d[0] = 1.0;
        let mut summe = 0.0f32;
        for tau in 1..=tau_max {
            let mut d = 0.0f32;
            for i in 0..YIN_FENSTER {
                let diff = x[i] - x[i + tau];
                d += diff * diff;
            }
            summe += d;
            self.yin_d[tau] = if summe > 0.0 { d * tau as f32 / summe } else { 1.0 };
        }
        let tau_min = p_min().floor() as usize;
        let mut tau = tau_min;
        let mut gefunden = None;
        while tau < tau_max {
            if self.yin_d[tau] < YIN_SCHWELLE {
                while tau + 1 < tau_max && self.yin_d[tau + 1] < self.yin_d[tau] {
                    tau += 1;
                }
                gefunden = Some(tau);
                break;
            }
            tau += 1;
        }
        let tau = gefunden.unwrap_or_else(|| {
            (tau_min..tau_max).min_by(|&a, &b| self.yin_d[a].total_cmp(&self.yin_d[b])).unwrap_or(tau_min)
        });
        let aperiodisch = self.yin_d[tau];
        // Parabel durch die Nachbarn für Bruchteile eines Samples.
        let (a, b, c) = (self.yin_d[tau - 1], self.yin_d[tau], self.yin_d[(tau + 1).min(tau_max)]);
        let nenner = a - 2.0 * b + c;
        let feiner = if nenner.abs() > 1e-9 { tau as f32 + 0.5 * (a - c) / nenner } else { tau as f32 };
        (feiner.clamp(p_min(), p_max()), aperiodisch < STIMMHAFT_GRENZE)
    }

    fn erkennung_bei(&self, t: f64) -> Erkennung {
        let t = t as i64;
        let n = self.n_erkennungen.min(self.erkennungen.len());
        (0..n)
            .map(|k| self.erkennungen[k])
            .min_by_key(|e| (e.mitte - t).abs())
            .unwrap_or(Erkennung { mitte: t, periode: STIMMLOS_SCHRITT, stimmhaft: false })
    }

    fn marke_bei(&self, t: f64) -> Option<Marke> {
        let n = self.n_marken.min(self.marken.len());
        (0..n).map(|k| self.marken[k]).min_by(|a, b| (a.pos - t).abs().total_cmp(&(b.pos - t).abs()))
    }
}

impl Effekt for Psola {
    fn verarbeite(&mut self, block: &mut [f32]) -> Result<(), String> {
        // 1. Eingang ablegen.
        for &s in block.iter() {
            self.ein[(self.n_ein & MASKE) as usize] = s;
            self.n_ein += 1;
        }

        // 2. Grundton im neuesten Stück. Glätten über die letzten 3 stimmhaften
        //    Werte (Median), damit einzelne Oktav-Fehler nicht durchschlagen.
        let (periode, stimmhaft) = self.grundton();
        let periode = if stimmhaft {
            self.letzte_perioden = [self.letzte_perioden[1], self.letzte_perioden[2], periode];
            let mut p = self.letzte_perioden;
            p.sort_by(|a, b| a.total_cmp(b));
            if p[0] > 0.0 { p[1] } else { periode }
        } else {
            STIMMLOS_SCHRITT
        };
        let mitte = self.n_ein - (self.yin_puffer.len() as i64) / 2;
        let k = self.n_erkennungen % self.erkennungen.len();
        self.erkennungen[k] = Erkennung { mitte, periode, stimmhaft };
        self.n_erkennungen += 1;

        // 3. Analyse-Marken, solange das Stück ganz im Eingang liegt.
        loop {
            let e = self.erkennung_bei(self.naechste_analyse);
            let p = if e.stimmhaft { e.periode } else { STIMMLOS_SCHRITT };
            if self.naechste_analyse + p as f64 + 2.0 > self.n_ein as f64 {
                break;
            }
            let k = self.n_marken % self.marken.len();
            self.marken[k] = Marke { pos: self.naechste_analyse, periode: p, stimmhaft: e.stimmhaft };
            self.n_marken += 1;
            // Gleicher Schritt wie bei der Synthese: ohne Änderung liegen beide
            // Marken genau übereinander, und das Original kommt unverändert heraus.
            self.naechste_analyse += p.max(1.0) as f64;
        }

        // 4. Synthese: Stücke einsetzen, bis der Ausgabe-Bereich fertig ist.
        let aus_ende = self.n_ein - self.verzoegerung;
        let reichweite = (p_max() / FORMANT_MIN) as f64;
        while self.naechste_synthese - reichweite < aus_ende as f64 {
            let Some(m) = self.marke_bei(self.naechste_synthese) else { break };
            let (alpha, beta) = if m.stimmhaft { (self.alpha, self.beta) } else { (1.0, self.beta) };
            // 0 bei gleicher oder höherer Stimme, 1 bei einer Oktave tiefer.
            let tief = if m.stimmhaft { ((1.0 - alpha) / 0.5).clamp(0.0, 1.0) } else { 0.0 };
            let l_g = m.periode / beta * (1.0 + TIEF_LAENGER * tief);
            if m.pos + (l_g * beta) as f64 + 2.0 > self.n_ein as f64 {
                break;
            }
            let p_s = m.periode / alpha;
            let skala = (p_s / l_g).min(2.0) * (1.0 + TIEF_LAUTSTAERKE * tief * self.zufall());
            let ts = self.naechste_synthese;
            let von = (ts - l_g as f64).ceil() as i64;
            let bis = (ts + l_g as f64).floor() as i64;
            for o in von..=bis {
                let j = o as f64 - ts;
                let w = 0.5 * (1.0 + (std::f64::consts::PI * j / l_g as f64).cos()) as f32;
                let quelle = m.pos + j * beta as f64;
                self.aus[(o & MASKE) as usize] += skala * w * self.ein_frac(quelle);
            }
            self.naechste_synthese += (p_s * (1.0 + TIEF_SCHWANKUNG * tief * self.zufall())).max(1.0) as f64;
        }

        // 5. Fertigen Bereich ausgeben und Platz frei machen.
        let start = aus_ende - block.len() as i64;
        for (k, s) in block.iter_mut().enumerate() {
            let o = start + k as i64;
            if o < 0 {
                *s = 0.0;
            } else {
                let i = (o & MASKE) as usize;
                *s = self.aus[i];
                self.aus[i] = 0.0;
            }
        }
        Ok(())
    }

    fn verzoegerung(&self) -> usize {
        self.verzoegerung as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::denoise::BLOCK;

    const RATE: f32 = INTERN_RATE as f32;

    /// Stimmähnlicher Ton mit leichtem Vibrato.
    fn stimme(f0: f32, sekunden: f32) -> Vec<f32> {
        let mut phase = 0.0f32;
        (0..(sekunden * RATE) as usize)
            .map(|i| {
                let t = i as f32 / RATE;
                let f = f0 * (1.0 + 0.01 * (std::f32::consts::TAU * 5.0 * t).sin());
                phase += f / RATE;
                (1..20).map(|k| (std::f32::consts::TAU * k as f32 * phase).sin() / k as f32).sum::<f32>() * 0.2
            })
            .collect()
    }

    fn grundton(x: &[f32]) -> f32 {
        // Eigene YIN-Instanz über ein langes Stück.
        let mut p = Psola::new(PsolaEinstellung { halbtoene: 0.0, formant: 0.0 });
        for &s in x {
            p.ein[(p.n_ein & MASKE) as usize] = s;
            p.n_ein += 1;
        }
        let (periode, stimmhaft) = p.grundton();
        assert!(stimmhaft, "nicht stimmhaft erkannt");
        RATE / periode
    }

    fn verarbeite(p: &mut Psola, x: &mut [f32]) {
        for b in x.chunks_exact_mut(BLOCK) {
            p.verarbeite(b).unwrap();
        }
    }

    #[test]
    fn yin_findet_grundton() {
        for f0 in [95.0, 130.0, 220.0, 400.0] {
            let ist = grundton(&stimme(f0, 0.1));
            assert!((12.0 * (ist / f0).log2()).abs() < 0.2, "{f0} Hz: {ist}");
        }
    }

    #[test]
    fn ohne_aenderung_kommt_das_original_verzoegert_heraus() {
        let mut p = Psola::new(PsolaEinstellung { halbtoene: 0.0, formant: 0.0 });
        let original = stimme(130.0, 1.0);
        let mut x = original.clone();
        verarbeite(&mut p, &mut x);
        let d = p.verzoegerung();
        let (mut fehler, mut signal) = (0.0f64, 0.0f64);
        for i in 24_000..44_000 {
            fehler += ((x[i] - original[i - d]) as f64).powi(2);
            signal += (original[i - d] as f64).powi(2);
        }
        let abstand = 10.0 * (signal / fehler).log10();
        assert!(abstand > 20.0, "Abstand {abstand} dB");
    }

    #[test]
    fn tonhoehe_stimmt() {
        for (halbtoene, f0) in [(-4.0, 130.0), (4.0, 130.0), (-7.0, 200.0), (5.0, 110.0), (-12.0, 220.0), (-12.0, 180.0)] {
            let mut p = Psola::new(PsolaEinstellung { halbtoene, formant: 0.0 });
            let mut x = stimme(f0, 1.0);
            verarbeite(&mut p, &mut x);
            let soll = f0 * 2f32.powf(halbtoene / 12.0);
            let ist = grundton(&x[30_000..30_000 + 2400]);
            assert!((12.0 * (ist / soll).log2()).abs() < 0.25, "{halbtoene} HT bei {f0} Hz: {ist} statt {soll}");
        }
    }

    #[test]
    fn formant_aendert_tonhoehe_nicht() {
        let mut p = Psola::new(PsolaEinstellung { halbtoene: -4.0, formant: -3.0 });
        let mut x = stimme(130.0, 1.0);
        verarbeite(&mut p, &mut x);
        let soll = 130.0 * 2f32.powf(-4.0 / 12.0);
        let ist = grundton(&x[30_000..30_000 + 2400]);
        assert!((12.0 * (ist / soll).log2()).abs() < 0.25, "{ist} statt {soll}");
    }

    #[test]
    fn zuruecksetzen_wie_neu() {
        let e = PsolaEinstellung { halbtoene: -4.0, formant: -3.0 };
        let mut p = Psola::new(e);
        let mut alt = stimme(200.0, 0.3);
        verarbeite(&mut p, &mut alt);
        p.zuruecksetzen();
        let mut neu = Psola::new(e);
        let (mut a, mut b) = (stimme(130.0, 0.3), stimme(130.0, 0.3));
        verarbeite(&mut p, &mut a);
        verarbeite(&mut neu, &mut b);
        assert_eq!(a, b, "alte Samples nach dem Zurücksetzen");
    }

    #[test]
    fn latenz_ca_30_ms() {
        let p = Psola::new(PsolaEinstellung { halbtoene: 0.0, formant: 0.0 });
        let ms = p.verzoegerung() as f32 / RATE * 1000.0;
        assert!(ms > 25.0 && ms < 35.0, "{ms} ms");
    }
}
