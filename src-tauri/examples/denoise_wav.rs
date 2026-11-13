//! Spielt eine Mitschnitt-WAV (1_roh.wav) offline durch Hochpass + Denoise.
//! Zur Fehlersuche: gleiche Eingabe, verschiedene Einstellungen vergleichen.
//!
//! cargo run --release --example denoise_wav -- <ein.wav> <aus.wav> [leicht|schnell|normal] [grundrauschen_dbfs|aus]

use maskcue_lib::audio::denoise::{BLOCK, Denoise, Modell};
use maskcue_lib::audio::eq::{Band, Equalizer};
use maskcue_lib::audio::mitschnitt::wav_schreiben;
use maskcue_lib::audio::verarbeitung::Effekt;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let bytes = std::fs::read(&a[0]).expect("WAV lesen");
    let mut x: Vec<f32> = bytes[44..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    let modell = match a.get(2).map(|s| s.as_str()) {
        Some("normal") => Modell::Normal,
        Some("leicht") => Modell::Leicht,
        _ => Modell::Schnell,
    };
    let rauschen_db: Option<f32> = a.get(3).and_then(|s| s.parse().ok());
    let mut hp = Equalizer::new(&[Band::Hochpass { hz: 80.0 }]);
    let mut d = Denoise::laden(modell, 100.0, 0.0).expect("Modell");
    let mut zufall = 1u64;
    let amp = rauschen_db.map_or(0.0, |db| 10f32.powf(db / 20.0) * 3f32.sqrt());
    for b in x.chunks_exact_mut(BLOCK) {
        hp.verarbeite(b).unwrap();
        if amp > 0.0 {
            for s in b.iter_mut() {
                zufall = zufall.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                *s += amp * (((zufall >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0);
            }
        }
        d.verarbeite(b).unwrap();
    }
    wav_schreiben(&a[1].clone().into(), &x).expect("WAV schreiben");
}
