//! Laut-Merkmale (phone extractor): Was gesagt wird, ohne wer es sagt.
//! 16 kHz rein, alle 10 ms ein Bild mit 128 Werten.

use super::gewichte::Leser;
use super::netz::{Art, Linear, Schrittfaltung, Stapel, StapelBau, gelu, normieren};

/// Wie viele Bilder je Folge die Selbst-Aufmerksamkeit zurückschaut
/// (150 × 4 Bilder = 6 s, so lang wie die Übungs-Stücke des Trainers).
pub const RUECKBLICK: usize = 150;
pub const KANAELE: usize = 128;

pub struct Laute {
    stufen: Vec<Schrittfaltung>,
    stapel: Stapel,
    kopf: Linear,
    x: Vec<f32>,
    y: Vec<f32>,
    pub bild: Vec<f32>,
}

impl Laute {
    pub fn lesen(l: &mut Leser) -> Result<Self, String> {
        let c = KANAELE;
        let mut stufen = Vec::new();
        for (ein, aus, k, s) in [(1, c / 8, 10, 5), (c / 8, c / 4, 3, 2), (c / 4, c / 2, 3, 2), (c / 2, c, 3, 2), (c, c, 3, 2), (c, c, 2, 2)] {
            stufen.push(Schrittfaltung::lesen(l, ein, aus, k, s)?);
        }
        let stapel = Stapel::lesen(
            l,
            StapelBau { ein: c, c, zwischen: c * 3, bloecke: 20, verzoegerung: 0, k_embed: 9, k: 17, ws: false, art: Art::Selbst(4, RUECKBLICK) },
        )?;
        let kopf = Linear::lesen(l, c, c, true)?;
        let mut laute = Self { stufen, stapel, kopf, x: vec![0.0; c], y: vec![0.0; c], bild: vec![0.0; c] };
        // 40 Nullen vorne wie im Trainer.
        for _ in 0..40 {
            laute.schieben(0.0);
        }
        Ok(laute)
    }

    /// Ein Sample (16 kHz). `true`: `bild` enthält ein neues Laut-Bild
    /// (noch ohne Zuordnung zum Ziel-Sprecher).
    pub fn schieben(&mut self, s: f32) -> bool {
        if !kaskade(&mut self.stufen, &[s]) {
            return false;
        }
        self.x.copy_from_slice(&self.stufen.last().expect("Stufen").aus_bild);
        normieren(&mut self.x);
        self.stapel.schritt(&self.x, &mut self.y);
        gelu(&mut self.y);
        self.kopf.rechne(&self.y, &mut self.bild);
        true
    }
}

fn kaskade(stufen: &mut [Schrittfaltung], x: &[f32]) -> bool {
    let Some((erste, rest)) = stufen.split_first_mut() else { return true };
    if !erste.schieben(x) {
        return false;
    }
    kaskade(rest, &erste.aus_bild)
}
