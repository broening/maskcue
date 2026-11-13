//! Figur-Stimme: Tonhöhe (Pitch) und Formant mit Signalsmith Stretch.
//!
//! Tonhöhe ändert, wie hoch die Stimme klingt. Formant ändert die „Größe“
//! des Sprechers (Kopf, Hals). Beides getrennt ergibt glaubhafte Figuren:
//! z. B. tiefer + größer = Riese, höher + kleiner = Kobold.

use std::ffi::c_void;

use super::denoise::BLOCK;
use super::pipe::INTERN_RATE;
use super::verarbeitung::Effekt;

unsafe extern "C" {
    fn gx_stretch_neu(block: i32, intervall: i32, verteilen: i32) -> *mut c_void;
    fn gx_stretch_loeschen(s: *mut c_void);
    fn gx_stretch_zuruecksetzen(s: *mut c_void);
    fn gx_stretch_latenz(s: *mut c_void) -> i32;
    fn gx_stretch_tonhoehe(s: *mut c_void, faktor: f32, tonal_grenze: f32);
    fn gx_stretch_formant(s: *mut c_void, faktor: f32, tonhoehe_ausgleichen: i32);
    fn gx_stretch_formant_basis(s: *mut c_void, hz: f32);
    fn gx_stretch_verarbeite(s: *mut c_void, ein: *const f32, aus: *mut f32, laenge: i32);
}

/// Einstellung einer Figur-Stimme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StimmEinstellung {
    /// Tonhöhe in Halbtönen (+12 = eine Oktave höher).
    pub halbtoene: f32,
    /// Formant in Halbtönen. Positiv = kleinerer Sprecher, negativ = größerer.
    pub formant: f32,
    /// Fensterlänge der Analyse (ms). Kürzer = weniger Verzögerung, aber
    /// tiefe Stimmen klingen rauer.
    pub fenster_ms: f32,
}

impl Default for StimmEinstellung {
    fn default() -> Self {
        Self { halbtoene: 0.0, formant: 0.0, fenster_ms: 40.0 }
    }
}

pub struct Stimme {
    s: *mut c_void,
    aus: [f32; BLOCK],
    latenz: usize,
}

// Signalsmith hat keinen Zustand, der an einen Faden gebunden ist.
unsafe impl Send for Stimme {}

impl Stimme {
    pub fn new(e: StimmEinstellung) -> Self {
        let block = ((e.fenster_ms.max(5.0) * 0.001 * INTERN_RATE as f32) as i32).max(BLOCK as i32 / 2);
        // Intervall = ein Viertel des Fensters (übliche Überlappung).
        let intervall = (block / 4).max(1);
        let s = unsafe { gx_stretch_neu(block, intervall, 1) };
        let mut stimme = Self { s, aus: [0.0; BLOCK], latenz: unsafe { gx_stretch_latenz(s) } as usize };
        stimme.einstellen(e);
        stimme
    }

    /// Tonhöhe und Formant ändern (ohne Neustart, Fenster bleibt).
    pub fn einstellen(&mut self, e: StimmEinstellung) {
        unsafe {
            gx_stretch_tonhoehe(self.s, 2f32.powf(e.halbtoene / 12.0), 0.0);
            // Formant: relativ zur Tonhöhe ausgleichen, damit „nur Pitch“ die
            // Formanten an ihrem Platz lässt (kein Mickymaus-Effekt).
            gx_stretch_formant(self.s, 2f32.powf(e.formant / 12.0), 1);
            gx_stretch_formant_basis(self.s, 0.0);
        }
    }

    pub fn zuruecksetzen(&mut self) {
        unsafe { gx_stretch_zuruecksetzen(self.s) }
    }
}

impl Drop for Stimme {
    fn drop(&mut self) {
        unsafe { gx_stretch_loeschen(self.s) }
    }
}

impl Effekt for Stimme {
    fn verarbeite(&mut self, block: &mut [f32]) -> Result<(), String> {
        debug_assert_eq!(block.len(), BLOCK);
        unsafe { gx_stretch_verarbeite(self.s, block.as_ptr(), self.aus.as_mut_ptr(), block.len() as i32) };
        block.copy_from_slice(&self.aus[..block.len()]);
        Ok(())
    }
    fn verzoegerung(&self) -> usize {
        self.latenz
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = INTERN_RATE as f32;

    /// Harmonischer Ton (wie eine Stimme) mit Grundton `f0`.
    fn stimme(f0: f32, sekunden: f32) -> Vec<f32> {
        (0..(sekunden * RATE) as usize)
            .map(|i| {
                let t = i as f32 / RATE;
                (1..15).map(|k| (std::f32::consts::TAU * f0 * k as f32 * t).sin() / k as f32).sum::<f32>() * 0.2
            })
            .collect()
    }

    /// Grundton über Autokorrelation (60..800 Hz).
    fn grundton(x: &[f32]) -> f32 {
        let (min, max) = ((RATE / 800.0) as usize, (RATE / 60.0) as usize);
        let n = x.len() - max;
        let r = |lag: usize| (0..n).map(|i| x[i] * x[i + lag]).sum::<f32>();
        let r0 = r(0);
        // Erstes deutliches Maximum (vermeidet Oktavfehler nach unten).
        let werte: Vec<f32> = (min..=max).map(r).collect();
        let grenze = 0.8 * werte.iter().cloned().fold(0.0, f32::max);
        let mut beste = 0;
        for i in 1..werte.len() - 1 {
            if werte[i] >= grenze && werte[i] >= werte[i - 1] && werte[i] >= werte[i + 1] {
                beste = i;
                break;
            }
        }
        let _ = r0;
        RATE / (min + beste) as f32
    }

    fn verarbeite(s: &mut Stimme, x: &mut [f32]) {
        for b in x.chunks_exact_mut(BLOCK) {
            s.verarbeite(b).unwrap();
        }
    }

    #[test]
    fn tonhoehe_stimmt() {
        for (halbtoene, f0) in [(0.0, 140.0), (5.0, 140.0), (-5.0, 140.0), (12.0, 120.0), (-7.0, 200.0)] {
            let mut s = Stimme::new(StimmEinstellung { halbtoene, ..Default::default() });
            let mut x = stimme(f0, 1.0);
            verarbeite(&mut s, &mut x);
            let soll = f0 * 2f32.powf(halbtoene / 12.0);
            let ist = grundton(&x[24_000..43_200]);
            // Toleranz: 1/4 Halbton.
            let fehler = 12.0 * (ist / soll).log2();
            assert!(fehler.abs() < 0.25, "{halbtoene} HT bei {f0} Hz: {ist} statt {soll} Hz");
        }
    }

    #[test]
    fn latenz_ist_fenster_plus_schritt() {
        // Signalsmith: halbes Fenster rein + halbes Fenster und ein Schritt raus.
        // Schritt = ein Viertel Fenster. 40 ms Fenster → 50 ms.
        let s = Stimme::new(StimmEinstellung { fenster_ms: 40.0, ..Default::default() });
        let ms = s.verzoegerung() as f32 / RATE * 1000.0;
        assert!((ms - 50.0).abs() < 1.0, "{ms} ms");
    }
}
