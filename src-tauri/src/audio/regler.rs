//! Effekte im Lauf verstellen und ein- oder ausschalten (Regler in der UI).
//!
//! Die App schickt neue Werte über eine Warteschlange ohne Sperre. Der
//! Effekt übernimmt sie vor dem nächsten Block. Der Ton läuft dabei weiter.

use super::verarbeitung::Effekt;

/// Ein Effekt, dessen Werte sich im Lauf ändern lassen.
pub trait Einstellbar {
    type Werte: Send + 'static;
    /// Neue Werte übernehmen. Der innere Zustand (Filter, Hülle) bleibt,
    /// damit es nicht knackt.
    fn einstellen(&mut self, w: Self::Werte);
}

enum Befehl<W> {
    Werte(W),
    An(bool),
}

/// Läuft im Verarbeitungs-Faden.
pub struct Live<T: Einstellbar> {
    inner: T,
    an: bool,
    rx: rtrb::Consumer<Befehl<T::Werte>>,
}

/// Bleibt in der App.
pub struct Regler<W> {
    tx: rtrb::Producer<Befehl<W>>,
}

impl<W> Regler<W> {
    /// Gibt `false` zurück, wenn die Warteschlange voll ist.
    pub fn setzen(&mut self, w: W) -> bool {
        self.tx.push(Befehl::Werte(w)).is_ok()
    }
    pub fn an(&mut self, an: bool) -> bool {
        self.tx.push(Befehl::An(an)).is_ok()
    }
}

/// Empfangsseite eines Reglers. Wandert in den Verarbeitungs-Faden, wo der
/// Effekt entsteht (DeepFilterNet darf den Faden nicht wechseln).
pub struct Empfang<W> {
    rx: rtrb::Consumer<Befehl<W>>,
}

pub fn kanal<W>() -> (Regler<W>, Empfang<W>) {
    let (tx, rx) = rtrb::RingBuffer::new(32);
    (Regler { tx }, Empfang { rx })
}

impl<T: Einstellbar> Live<T> {
    pub fn new(inner: T, an: bool, empfang: Empfang<T::Werte>) -> Self {
        Live { inner, an, rx: empfang.rx }
    }
}

pub fn live<T: Einstellbar>(inner: T, an: bool) -> (Live<T>, Regler<T::Werte>) {
    let (regler, empfang) = kanal();
    (Live::new(inner, an, empfang), regler)
}

impl<T: Effekt + Einstellbar> Effekt for Live<T> {
    fn verarbeite(&mut self, block: &mut [f32]) -> Result<(), String> {
        while let Ok(b) = self.rx.pop() {
            match b {
                Befehl::Werte(w) => self.inner.einstellen(w),
                Befehl::An(an) => self.an = an,
            }
        }
        if self.an { self.inner.verarbeite(block) } else { Ok(()) }
    }
    fn verzoegerung(&self) -> usize {
        if self.an { self.inner.verzoegerung() } else { 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::dynamik::{Kompressor, KompressorEinstellung};
    use crate::audio::eq::{Band, Equalizer};

    fn rms_db(x: &[f32]) -> f32 {
        10.0 * (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).log10()
    }

    fn sinus(hz: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / 48_000.0).sin()).collect()
    }

    #[test]
    fn aus_laesst_durch_an_wirkt() {
        let (mut eq, mut r) = live(Equalizer::new(&[Band::Hochpass { hz: 1000.0 }]), false);
        let x = sinus(100.0, 48_000);
        let mut y = x.clone();
        for b in y.chunks_mut(480) {
            eq.verarbeite(b).unwrap();
        }
        assert_eq!(x, y);
        r.an(true);
        let mut y = x.clone();
        for b in y.chunks_mut(480) {
            eq.verarbeite(b).unwrap();
        }
        assert!(rms_db(&y[24_000..]) < rms_db(&x) - 30.0);
    }

    #[test]
    fn werte_aendern_im_lauf() {
        // Tiefen +12 dB bis 2 kHz, dann 0 dB: Ein 200-Hz-Ton wird erst lauter, dann normal.
        let (mut eq, mut r) = live(Equalizer::new(&[Band::Tiefen { hz: 2000.0, db: 12.0 }]), true);
        let x = sinus(200.0, 48_000);
        let mut y = x.clone();
        for (i, b) in y.chunks_mut(480).enumerate() {
            if i == 50 {
                r.setzen(vec![Band::Tiefen { hz: 2000.0, db: 0.0 }]);
            }
            eq.verarbeite(b).unwrap();
        }
        let vorher = rms_db(&y[10_000..24_000]) - rms_db(&x[10_000..24_000]);
        let nachher = rms_db(&y[30_000..]) - rms_db(&x[30_000..]);
        assert!(vorher > 10.0, "{vorher}");
        assert!(nachher.abs() < 0.5, "{nachher}");
        // Kein Sprung beim Wechsel (Filter-Zustand bleibt). Normaler Schritt
        // beim Sinus: 2,0 · 2π · 200 / 48000 ≈ 0,05. Ein leerer Filter-Zustand gäbe bis 2.
        let sprung = y[23_000..25_000].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(sprung < 0.2, "{sprung}");
    }

    #[test]
    fn kompressor_aufholen_im_lauf() {
        let (mut k, mut r) = live(Kompressor::new(KompressorEinstellung { aufholen_db: 0.0, ..Default::default() }), true);
        let x: Vec<f32> = vec![0.01; 48_000]; // -40 dBFS, unter der Schwelle
        let mut y = x.clone();
        for (i, b) in y.chunks_mut(480).enumerate() {
            if i == 50 {
                r.setzen(KompressorEinstellung { aufholen_db: 6.0, ..Default::default() });
            }
            k.verarbeite(b).unwrap();
        }
        assert!((y[10_000] / 0.01 - 1.0).abs() < 0.01);
        assert!((y[40_000] / 0.01 - 2.0).abs() < 0.02, "{}", y[40_000] / 0.01);
    }
}
