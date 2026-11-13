//! Abgriff für den Aufnahme-Bereich: nimmt auf, was aus dem Ausgang „Spiel“
//! kommt (Stimme mit Figur und Umgebung, dazu Sounds und Musik).
//!
//! Der Abgriff sitzt im Ausgangs-Faden (Echtzeit). Er wartet nie und legt
//! keinen Speicher an: Er schreibt in einen Ringpuffer. Ist der voll, gehen
//! Samples verloren (wird gezählt), der Ton läuft weiter. Der Motor holt
//! die Samples regelmäßig ab.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

use super::pipe::INTERN_RATE;

/// Platz im Ringpuffer: 3 Sekunden. Der Motor holt alle 100 ms ab.
const PLATZ: usize = INTERN_RATE as usize * 3;

/// Seite im Ausgangs-Faden.
pub struct AufnahmeAbgriff {
    an: Arc<AtomicBool>,
    verloren: Arc<AtomicU64>,
    tx: rtrb::Producer<f32>,
}

/// Seite im Motor.
pub struct AufnahmeQuelle {
    an: Arc<AtomicBool>,
    verloren: Arc<AtomicU64>,
    rx: rtrb::Consumer<f32>,
}

pub fn aufnahme() -> (AufnahmeAbgriff, AufnahmeQuelle) {
    let (tx, rx) = rtrb::RingBuffer::new(PLATZ);
    let an = Arc::new(AtomicBool::new(false));
    let verloren = Arc::new(AtomicU64::new(0));
    (AufnahmeAbgriff { an: an.clone(), verloren: verloren.clone(), tx }, AufnahmeQuelle { an, verloren, rx })
}

impl AufnahmeAbgriff {
    /// `data`: verschachtelt mit `kanaele` Kanälen, 48 kHz. Nimmt den
    /// Mittelwert der Kanäle (mono).
    #[inline]
    pub fn abgreifen(&mut self, data: &[f32], kanaele: usize) {
        if !self.an.load(Relaxed) {
            return;
        }
        let k = kanaele.max(1);
        let frames = data.len() / k;
        let frei = self.tx.slots();
        if frames > frei {
            self.verloren.fetch_add((frames - frei) as u64, Relaxed);
        }
        let n = frames.min(frei);
        if n == 0 {
            return;
        }
        if let Ok(mut chunk) = self.tx.write_chunk(n) {
            let (a, b) = chunk.as_mut_slices();
            for (i, s) in a.iter_mut().chain(b.iter_mut()).enumerate() {
                let frame = &data[i * k..(i + 1) * k];
                *s = frame.iter().sum::<f32>() / k as f32;
            }
            chunk.commit_all();
        }
    }
}

impl AufnahmeQuelle {
    /// Aufnahme an oder aus. Beim Einschalten wird Altes verworfen.
    pub fn schalten(&mut self, an: bool) {
        if an {
            self.abholen(&mut |_| {});
            self.verloren.store(0, Relaxed);
        }
        self.an.store(an, Relaxed);
    }

    pub fn ist_an(&self) -> bool {
        self.an.load(Relaxed)
    }

    /// Verlorene Samples seit dem Einschalten (Puffer war voll).
    pub fn verloren(&self) -> u64 {
        self.verloren.load(Relaxed)
    }

    /// Gibt alle wartenden Samples an `ziel`.
    pub fn abholen(&mut self, ziel: &mut dyn FnMut(&[f32])) {
        let n = self.rx.slots();
        if n == 0 {
            return;
        }
        if let Ok(chunk) = self.rx.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            ziel(a);
            ziel(b);
            chunk.commit_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alles(q: &mut AufnahmeQuelle) -> Vec<f32> {
        let mut v = Vec::new();
        q.abholen(&mut |s| v.extend_from_slice(s));
        v
    }

    #[test]
    fn aus_nimmt_nichts_auf() {
        let (mut a, mut q) = aufnahme();
        a.abgreifen(&[0.5; 960], 2);
        assert!(alles(&mut q).is_empty());
    }

    #[test]
    fn nimmt_mono_als_mittelwert_auf() {
        let (mut a, mut q) = aufnahme();
        q.schalten(true);
        a.abgreifen(&[0.2, 0.4, -1.0, 1.0, 0.6, 0.6], 2);
        let v = alles(&mut q);
        assert_eq!(v.len(), 3);
        assert!((v[0] - 0.3).abs() < 1e-6 && v[1].abs() < 1e-6 && (v[2] - 0.6).abs() < 1e-6, "{v:?}");
    }

    #[test]
    fn voller_puffer_zaehlt_verluste_und_blockiert_nicht() {
        let (mut a, mut q) = aufnahme();
        q.schalten(true);
        let block = vec![0.1; 4800];
        for _ in 0..40 {
            a.abgreifen(&block, 1); // 4 s, Platz nur für 3 s
        }
        assert_eq!(q.verloren(), 48_000);
        assert_eq!(alles(&mut q).len(), PLATZ);
        // Neu einschalten: Altes weg, Zähler auf null.
        a.abgreifen(&block, 1);
        q.schalten(true);
        assert_eq!(q.verloren(), 0);
        assert!(alles(&mut q).is_empty());
    }
}
