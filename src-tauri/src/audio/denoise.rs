//! KI-Denoise mit DeepFilterNet3 oder RNNoise.
//!
//! Arbeitet in Blöcken von 480 Samples (10 ms bei 48 kHz).
//! Drei Modelle:
//! - `Leicht` (RNNoise): wenig CPU, ca. 10 ms Verzögerung, filtert schwächer.
//! - `Schnell` (DeepFilterNet3 „ll“): keine Vorschau, ca. 10 ms Verzögerung.
//!   Nur noch für die Mess-Programme: Es braucht mehr CPU als `Normal`, darum
//!   hat die App es nicht mehr (examples/denoise_vergleich.rs).
//! - `Normal` (DeepFilterNet3): 2 Blöcke Vorschau, ca. 30 ms Verzögerung.

use std::path::PathBuf;

use df::tract::{DfParams, DfTract, RuntimeParams};
use ndarray::{ArrayView2, ArrayViewMut2};
use nnnoiseless::DenoiseState;

/// Blockgröße von DeepFilterNet (Samples bei 48 kHz).
pub const BLOCK: usize = 480;
/// Fenstergröße der Analyse (Samples).
const FFT: usize = 960;
/// Ab diesem geschätzten Abstand zum Rauschen (dB) würde DeepFilterNet ein
/// Stück unbearbeitet durchlassen (ab Werk 30 bzw. 20). Ein Tastatur-Klick
/// nach einer stillen Pause liegt darüber und kam voll durch (Dan: „nach jeder
/// Pause höre ich zwei Klicks“). Darum immer bearbeiten. Die Stimme ändert
/// sich dadurch kaum (Unterschied 35 dB unter der Stimme, gemessen mit
/// examples/klick_analyse.rs).
const IMMER_BEARBEITEN_DB: f32 = 100.0;
/// Darunter gilt ein Stück als reines Rauschen (ab Werk).
const NUR_RAUSCHEN_DB: f32 = -10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modell {
    Leicht,
    Schnell,
    Normal,
}

impl Modell {
    /// Modelldatei von DeepFilterNet. RNNoise hat keine (Modell ist eingebaut).
    fn datei(self) -> &'static str {
        match self {
            Modell::Leicht => unreachable!("RNNoise hat keine Modelldatei"),
            Modell::Schnell => "DeepFilterNet3_ll_onnx.tar.gz",
            Modell::Normal => "DeepFilterNet3_onnx.tar.gz",
        }
    }
}

static MODELL_ORDNER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Die installierte App setzt hier ihren Ressourcen-Ordner (beim Start, einmal).
pub fn modell_ordner_setzen(ordner: PathBuf) {
    let _ = MODELL_ORDNER.set(ordner);
}

/// Liegt in `ordner` ein mitgeliefertes Denoise-Modell? (Ressourcen der
/// installierten App.)
pub fn ist_modell_ordner(ordner: &std::path::Path) -> bool {
    ordner.join(Modell::Normal.datei()).exists()
}

/// Ordner mit den Modellen: installiert neben der App, beim Entwickeln
/// `src-tauri/modelle`.
pub fn modell_ordner() -> PathBuf {
    MODELL_ORDNER.get().cloned().unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("modelle"))
}

/// Werte, die sich im Lauf ändern lassen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DenoiseWerte {
    /// Wie stark Rauschen höchstens gedämpft wird (dB, 100 = ganz weg).
    pub daempfung_db: f32,
    /// Nachfilter (0 = aus, 0.02 = normal).
    pub nachfilter: f32,
}

impl super::regler::Einstellbar for Denoise {
    type Werte = DenoiseWerte;
    fn einstellen(&mut self, w: DenoiseWerte) {
        match &mut self.motor {
            Motor::DeepFilterNet(df) => {
                df.set_atten_lim(w.daempfung_db);
                df.set_pf_beta(w.nachfilter.max(0.0));
            }
            // Den Nachfilter gibt es bei RNNoise nicht: Es filtert nur über
            // seine eigenen Bandverstärkungen. Nur die Dämpfung wirkt.
            Motor::Rnnoise(r) => r.mischung = mischung(w.daempfung_db),
        }
    }
}

/// Anteil des Originals im Ergebnis, damit Rauschen höchstens um `db`
/// leiser wird (100 und mehr = ganz entrauscht).
fn mischung(db: f32) -> f32 {
    if db >= 100.0 { 0.0 } else { 10f32.powf(-db.max(0.0) / 20.0) }
}

/// RNNoise-Zustand. RNNoise rechnet in 16-Bit-Werten und liefert sein Ergebnis
/// einen Block später, darum wird das Original für die Mischung um einen
/// Block verzögert.
struct Rnnoise {
    zustand: Box<DenoiseState<'static>>,
    /// Vorheriger Eingangsblock (passt zur Ausgabe des aktuellen Blocks).
    vorher: [f32; BLOCK],
    ein: [f32; BLOCK],
    aus: [f32; BLOCK],
    /// Anteil Original im Ergebnis (0 = voll entrauscht).
    mischung: f32,
}

enum Motor {
    DeepFilterNet(DfTract),
    Rnnoise(Box<Rnnoise>),
}

pub struct Denoise {
    motor: Motor,
    ein: [f32; BLOCK],
    aus: [f32; BLOCK],
}

impl Denoise {
    /// Lädt ein Modell. `daempfung_db`: wie stark Rauschen höchstens gedämpft
    /// wird (100 = ganz weg, 20 = etwas Rest bleibt hörbar). `nachfilter`:
    /// dämpft Rest-Rauschen auch während der Sprache (0 = aus, 0.02 = normal,
    /// mehr = stärker, aber die Stimme wird dünner). Bei `Leicht` gibt es den
    /// Nachfilter nicht, der Wert wird ignoriert.
    pub fn laden(modell: Modell, daempfung_db: f32, nachfilter: f32) -> Result<Self, String> {
        if modell == Modell::Leicht {
            let r = Rnnoise {
                zustand: DenoiseState::new(),
                vorher: [0.0; BLOCK],
                ein: [0.0; BLOCK],
                aus: [0.0; BLOCK],
                mischung: mischung(daempfung_db),
            };
            return Ok(Self { motor: Motor::Rnnoise(Box::new(r)), ein: [0.0; BLOCK], aus: [0.0; BLOCK] });
        }
        let pfad = modell_ordner().join(modell.datei());
        let dfp = DfParams::new(pfad.clone()).map_err(|e| format!("Denoise-Modell {}: {e}", pfad.display()))?;
        let rp = RuntimeParams::default_with_ch(1).with_atten_lim(daempfung_db).with_post_filter(nachfilter.max(0.0));
        let df = DfTract::new(dfp, &rp).map_err(|e| format!("Denoise: {e}"))?;
        let mut d = Self { motor: Motor::DeepFilterNet(df), ein: [0.0; BLOCK], aus: [0.0; BLOCK] };
        d.schwellen(NUR_RAUSCHEN_DB, IMMER_BEARBEITEN_DB, IMMER_BEARBEITEN_DB);
        Ok(d)
    }

    /// Schwellen des Modells (dB, geschätzter Abstand zum Rauschen): Unter
    /// `min` nur Stille, über `max_erb` keine Bearbeitung, über `max_df` kein
    /// Feinfilter. Ab Werk −10 / 30 / 20. Bei `Leicht` ohne Wirkung.
    pub fn schwellen(&mut self, min: f32, max_erb: f32, max_df: f32) {
        if let Motor::DeepFilterNet(df) = &mut self.motor {
            df.min_db_thresh = min;
            df.max_db_erb_thresh = max_erb;
            df.max_db_df_thresh = max_df;
        }
    }

    /// Verzögerung durch Fenster und Vorschau (Samples).
    pub fn verzoegerung(&self) -> usize {
        match &self.motor {
            Motor::DeepFilterNet(df) => FFT - BLOCK + df.lookahead * BLOCK,
            Motor::Rnnoise(_) => BLOCK,
        }
    }

    /// Entrauscht einen Block von genau [`BLOCK`] Samples an Ort und Stelle.
    /// Gibt den geschätzten Signal-Rausch-Abstand (dB) zurück. Bei `Leicht`
    /// gibt es keinen solchen Wert, dort kommt 0 zurück.
    pub fn verarbeite(&mut self, block: &mut [f32]) -> Result<f32, String> {
        debug_assert_eq!(block.len(), BLOCK);
        match &mut self.motor {
            Motor::DeepFilterNet(df) => {
                self.ein.copy_from_slice(block);
                let ein = ArrayView2::from_shape((1, BLOCK), &self.ein[..]).map_err(|e| e.to_string())?;
                let aus = ArrayViewMut2::from_shape((1, BLOCK), &mut self.aus[..]).map_err(|e| e.to_string())?;
                let snr = df.process(ein, aus).map_err(|e| format!("Denoise: {e}"))?;
                block.copy_from_slice(&self.aus);
                Ok(snr)
            }
            Motor::Rnnoise(r) => {
                // Auf den 16-Bit-Bereich begrenzen, den RNNoise erwartet.
                for (e, b) in r.ein.iter_mut().zip(block.iter()) {
                    *e = b.clamp(-1.0, 1.0) * 32768.0;
                }
                r.zustand.process_frame(&mut r.aus, &r.ein);
                let m = r.mischung;
                for i in 0..BLOCK {
                    let sauber = r.aus[i] / 32768.0;
                    block[i] = if m > 0.0 { (1.0 - m) * sauber + m * r.vorher[i] } else { sauber };
                }
                for (v, e) in r.vorher.iter_mut().zip(r.ein.iter()) {
                    *v = e / 32768.0;
                }
                Ok(0.0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Das Installationspaket enthält seit 19.10.2026 nur das Modell
    /// „Normal“. Der Start prüfte aber auf „Schnell“ und fand den Ordner der
    /// installierten App dann nicht (21.10.2026).
    #[test]
    fn installierter_ordner_mit_nur_normal() {
        let o = std::env::temp_dir().join(format!("maskcue_dfn_{}", std::process::id()));
        std::fs::create_dir_all(&o).unwrap();
        std::fs::write(o.join(Modell::Normal.datei()), b"x").unwrap();
        let ok = ist_modell_ordner(&o);
        std::fs::remove_dir_all(&o).unwrap();
        assert!(ok);
    }

    /// Fehler von Dan: Nach einer Pause kamen die ersten Tastatur-Klicks voll
    /// durch. Erster Klick nach 1 s Stille muss deutlich leiser sein.
    #[test]
    fn klick_nach_pause_kommt_nicht_voll_durch() {
        let mut d = Denoise::laden(Modell::Schnell, 100.0, 0.0).expect("Modell");
        let fs = 48_000usize;
        let mut z: u32 = 7;
        let mut n = || {
            z ^= z << 13;
            z ^= z >> 17;
            z ^= z << 5;
            z as f32 / u32::MAX as f32 * 2.0 - 1.0
        };
        let mut x: Vec<f32> = (0..fs * 2).map(|_| n() * 0.0004).collect();
        let start = fs;
        for i in 0..fs / 50 {
            let t = i as f32 / fs as f32;
            x[start + i] += 0.25 * (n() * (-t / 0.0015).exp() + 0.6 * (std::f32::consts::TAU * 2500.0 * t).sin() * (-t / 0.006).exp());
        }
        let v = d.verzoegerung();
        let mut y = x.clone();
        for b in y.chunks_mut(BLOCK) {
            d.verarbeite(b).unwrap();
        }
        let spitze = |s: &[f32]| s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let rein = spitze(&x[start..start + fs / 25]);
        let raus = spitze(&y[start + v..start + v + fs / 25]);
        let db = 20.0 * (raus / rein).log10();
        assert!(db < -10.0, "Klick nur {db:.1} dB leiser");
    }

    /// Leicht (RNNoise): Ausgabe endlich, Rauschen leiser, `daempfung_db`
    /// begrenzt die Dämpfung. RNNoise ist auf Lüfter- und Raumrauschen
    /// trainiert; reines weißes Rauschen dämpft es nur schwach (gemessen
    /// ca. 7 dB), tiefpass-gefiltertes Rauschen dagegen um über 30 dB.
    #[test]
    fn leicht_daempft_rauschen() {
        let fs = 48_000usize;
        let mut z: u32 = 12345;
        let mut n = || {
            z ^= z << 13;
            z ^= z >> 17;
            z ^= z << 5;
            z as f32 / u32::MAX as f32 * 2.0 - 1.0
        };
        let weiss: Vec<f32> = (0..fs * 2).map(|_| n() * 0.03).collect();
        let mut tp = 0.0f32;
        let lueftung: Vec<f32> = (0..fs * 2)
            .map(|_| {
                tp = 0.9 * tp + 0.1 * n() * 0.1;
                tp * 3.0
            })
            .collect();
        let sinus = |i: usize| 0.2 * (std::f32::consts::TAU * 200.0 * i as f32 / fs as f32).sin();
        let weiss_sinus: Vec<f32> = weiss.iter().enumerate().map(|(i, r)| r + sinus(i)).collect();
        let laufen = |x: &[f32], db: f32| -> Vec<f32> {
            let mut d = Denoise::laden(Modell::Leicht, db, 0.0).expect("RNNoise");
            assert_eq!(d.verzoegerung(), BLOCK);
            let mut y = x.to_vec();
            for b in y.chunks_mut(BLOCK) {
                d.verarbeite(b).unwrap();
            }
            assert!(y.iter().all(|v| v.is_finite()), "Ausgabe nicht endlich");
            y
        };
        // Energie ohne die erste halbe Sekunde (Einschwingen).
        let energie = |s: &[f32]| s[fs / 2..].iter().map(|v| v * v).sum::<f32>() / (s.len() - fs / 2) as f32;
        let db = |a: &[f32], b: &[f32]| 10.0 * (energie(a) / energie(b)).log10();

        let w = db(&laufen(&weiss, 100.0), &weiss);
        let ws = db(&laufen(&weiss_sinus, 100.0), &weiss_sinus);
        let l = db(&laufen(&lueftung, 100.0), &lueftung);
        let l12 = db(&laufen(&lueftung, 12.0), &lueftung);
        println!("Leicht: weiss {w:.1} dB, weiss+Sinus 200 Hz {ws:.1} dB, Lüfter {l:.1} dB, Lüfter bei 12 dB Grenze {l12:.1} dB");
        assert!(w < -3.0, "weißes Rauschen nur {w:.1} dB leiser");
        assert!(l < -20.0, "Lüfterrauschen nur {l:.1} dB leiser");
        assert!((-13.5..-10.5).contains(&l12), "Grenze 12 dB wirkt nicht: {l12:.1} dB");
    }
}
