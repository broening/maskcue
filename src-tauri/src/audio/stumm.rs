//! Stummschalter für die Stimme (MaskCue-Stummtaste). Sounds laufen weiter:
//! Sie kommen erst im Ausgangs-Faden dazu, hinter diesem Effekt.
//!
//! Ein- und Ausblenden über 5 ms, damit es nicht knackt.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

use super::pipe::INTERN_RATE;
use super::verarbeitung::Effekt;

pub struct Stumm {
    an: Arc<AtomicBool>,
    pegel: f32,
    schritt: f32,
}

impl Stumm {
    pub fn new(an: Arc<AtomicBool>) -> Self {
        let pegel = if an.load(Relaxed) { 0.0 } else { 1.0 };
        Self { an, pegel, schritt: 1.0 / (0.005 * INTERN_RATE as f32) }
    }
}

impl Effekt for Stumm {
    fn verarbeite(&mut self, block: &mut [f32]) -> Result<(), String> {
        let ziel = if self.an.load(Relaxed) { 0.0 } else { 1.0 };
        if self.pegel == ziel {
            if ziel == 0.0 {
                block.fill(0.0);
            }
            return Ok(());
        }
        for s in block.iter_mut() {
            // Am Ziel bleiben, nicht wieder einen Schritt zurück (sonst flirrt es).
            if ziel > self.pegel {
                self.pegel = (self.pegel + self.schritt).min(1.0);
            } else if ziel < self.pegel {
                self.pegel = (self.pegel - self.schritt).max(0.0);
            }
            *s *= self.pegel;
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

    #[test]
    fn stumm_mit_weichem_uebergang() {
        let an = Arc::new(AtomicBool::new(false));
        let mut s = Stumm::new(an.clone());
        let mut b = vec![0.5f32; 480];
        s.verarbeite(&mut b).unwrap();
        assert!(b.iter().all(|&x| x == 0.5));
        an.store(true, Relaxed);
        let mut b = vec![0.5f32; 480];
        s.verarbeite(&mut b).unwrap();
        // Blendet aus: kein Sprung, am Ende still.
        assert!(b.windows(2).all(|w| (w[0] - w[1]).abs() < 0.01));
        assert_eq!(b[479], 0.0);
        let mut b = vec![0.5f32; 480];
        s.verarbeite(&mut b).unwrap();
        assert!(b.iter().all(|&x| x == 0.0));
        an.store(false, Relaxed);
        let mut b = vec![0.5f32; 480];
        s.verarbeite(&mut b).unwrap();
        assert!(b[0] < 0.01 && (b[479] - 0.5).abs() < 1e-6, "b[0]={} b[479]={}", b[0], b[479]);
    }
}
