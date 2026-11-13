//! Equalizer aus Biquad-Filtern (Formeln: RBJ Audio EQ Cookbook). 0 ms Verzögerung.

use super::pipe::INTERN_RATE;
use super::regler::Einstellbar;
use super::verarbeitung::Effekt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Band {
    /// Lässt nur Töne über `hz` durch (gegen Brummen und Trittschall).
    Hochpass { hz: f32 },
    /// Hebt oder senkt alles unter `hz`.
    Tiefen { hz: f32, db: f32 },
    /// Hebt oder senkt einen Bereich um `hz`. `q`: Breite (klein = breit).
    Glocke { hz: f32, db: f32, q: f32 },
    /// Hebt oder senkt alles über `hz`.
    Hoehen { hz: f32, db: f32 },
    /// Lässt nur Töne unter `hz` durch.
    Tiefpass { hz: f32 },
    /// Lässt nur einen Bereich um `hz` durch (Spitze 0 dB). `q`: Breite.
    Bandpass { hz: f32, q: f32 },
}

/// Ein Filter zweiter Ordnung (transponierte Direktform II).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    pub(crate) fn new(band: Band) -> Self {
        let fs = INTERN_RATE as f32;
        let w0 = |hz: f32| std::f32::consts::TAU * hz.clamp(10.0, fs * 0.45) / fs;
        let (b0, b1, b2, a0, a1, a2) = match band {
            Band::Hochpass { hz } => {
                let w = w0(hz);
                let (c, alpha) = (w.cos(), w.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2));
                ((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha)
            }
            Band::Tiefpass { hz } => {
                let w = w0(hz);
                let (c, alpha) = (w.cos(), w.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2));
                ((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha)
            }
            Band::Bandpass { hz, q } => {
                let w = w0(hz);
                let (c, alpha) = (w.cos(), w.sin() / (2.0 * q.max(0.1)));
                (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * c, 1.0 - alpha)
            }
            Band::Glocke { hz, db, q } => {
                let w = w0(hz);
                let a = 10f32.powf(db / 40.0);
                let (c, alpha) = (w.cos(), w.sin() / (2.0 * q.max(0.1)));
                (1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a)
            }
            Band::Tiefen { hz, db } | Band::Hoehen { hz, db } => {
                let w = w0(hz);
                let a = 10f32.powf(db / 40.0);
                let c = w.cos();
                // Steilheit S = 1.
                let alpha = w.sin() / 2.0 * std::f32::consts::SQRT_2;
                let wa = 2.0 * a.sqrt() * alpha;
                if matches!(band, Band::Tiefen { .. }) {
                    (
                        a * ((a + 1.0) - (a - 1.0) * c + wa),
                        2.0 * a * ((a - 1.0) - (a + 1.0) * c),
                        a * ((a + 1.0) - (a - 1.0) * c - wa),
                        (a + 1.0) + (a - 1.0) * c + wa,
                        -2.0 * ((a - 1.0) + (a + 1.0) * c),
                        (a + 1.0) + (a - 1.0) * c - wa,
                    )
                } else {
                    (
                        a * ((a + 1.0) + (a - 1.0) * c + wa),
                        -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
                        a * ((a + 1.0) + (a - 1.0) * c - wa),
                        (a + 1.0) - (a - 1.0) * c + wa,
                        2.0 * ((a - 1.0) - (a + 1.0) * c),
                        (a + 1.0) - (a - 1.0) * c - wa,
                    )
                }
            }
        };
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0, z1: 0.0, z2: 0.0 }
    }

    #[inline]
    pub(crate) fn schritt(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

pub struct Equalizer {
    filter: Vec<Biquad>,
}

impl Equalizer {
    pub fn new(baender: &[Band]) -> Self {
        Self { filter: baender.iter().map(|&b| Biquad::new(b)).collect() }
    }

    /// Standard fürs Mikrofon: nur Hochpass bei 80 Hz.
    pub fn standard() -> Self {
        Self::new(&[Band::Hochpass { hz: 80.0 }])
    }
}

impl Einstellbar for Equalizer {
    type Werte = Vec<Band>;
    /// Neue Bänder. Der Zustand gleicher Plätze bleibt, damit es nicht knackt.
    fn einstellen(&mut self, baender: Vec<Band>) {
        let alt = std::mem::take(&mut self.filter);
        self.filter = baender
            .iter()
            .enumerate()
            .map(|(i, &b)| {
                let mut f = Biquad::new(b);
                if let Some(a) = alt.get(i) {
                    f.z1 = a.z1;
                    f.z2 = a.z2;
                }
                f
            })
            .collect();
    }
}

impl Effekt for Equalizer {
    fn verarbeite(&mut self, block: &mut [f32]) -> Result<(), String> {
        for f in self.filter.iter_mut() {
            for x in block.iter_mut() {
                *x = f.schritt(*x);
            }
        }
        Ok(())
    }
    fn verzoegerung(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verstärkung in dB für einen Sinus mit `hz`, nach dem Einschwingen.
    fn verstaerkung(baender: &[Band], hz: f32) -> f32 {
        let mut eq = Equalizer::new(baender);
        let fs = INTERN_RATE as f32;
        let mut x: Vec<f32> = (0..48_000).map(|i| (std::f32::consts::TAU * hz * i as f32 / fs).sin()).collect();
        for b in x.chunks_mut(480) {
            eq.verarbeite(b).unwrap();
        }
        let rms = |v: &[f32]| (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt();
        20.0 * (rms(&x[24_000..]) / std::f32::consts::FRAC_1_SQRT_2).log10()
    }

    #[test]
    fn hochpass() {
        let b = [Band::Hochpass { hz: 80.0 }];
        assert!(verstaerkung(&b, 20.0) < -20.0);
        assert!((verstaerkung(&b, 80.0) + 3.0).abs() < 0.3, "{}", verstaerkung(&b, 80.0));
        assert!(verstaerkung(&b, 1000.0).abs() < 0.1);
    }

    #[test]
    fn glocke() {
        let b = [Band::Glocke { hz: 1000.0, db: 6.0, q: 1.0 }];
        assert!((verstaerkung(&b, 1000.0) - 6.0).abs() < 0.1);
        assert!(verstaerkung(&b, 100.0).abs() < 0.3);
        assert!(verstaerkung(&b, 10_000.0).abs() < 0.3);
    }

    #[test]
    fn kuhschwanz() {
        let t = [Band::Tiefen { hz: 200.0, db: -6.0 }];
        assert!((verstaerkung(&t, 40.0) + 6.0).abs() < 0.3);
        assert!(verstaerkung(&t, 3000.0).abs() < 0.3);
        let h = [Band::Hoehen { hz: 5000.0, db: 4.0 }];
        assert!((verstaerkung(&h, 15_000.0) - 4.0).abs() < 0.3);
        assert!(verstaerkung(&h, 200.0).abs() < 0.3);
    }
}
