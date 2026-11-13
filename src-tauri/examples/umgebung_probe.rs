//! Rechnet eine Aufnahme durch alle Stimm-Umgebungen, zum Anhören.
//! Eingang: WAV 48 kHz mono f32 (z. B. `aufnahmen/test1/4_kette.wav`).
//!
//! cargo run --release --example umgebung_probe -- <eingang.wav> <ausgabe-ordner> [sekunden]

use maskcue_lib::audio::denoise::BLOCK;
use maskcue_lib::audio::mitschnitt::wav_schreiben;
use maskcue_lib::audio::umgebung::{Umgebung, umgebung};
use maskcue_lib::audio::verarbeitung::Effekt;

fn lesen(p: &std::path::Path) -> Vec<f32> {
    let bytes = std::fs::read(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let mut x: Vec<f32> = bytes[44..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    x.truncate(x.len() / BLOCK * BLOCK);
    x
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let ein = std::path::PathBuf::from(&a[0]);
    let aus = std::path::PathBuf::from(&a[1]);
    let sekunden: f32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(f32::MAX);
    std::fs::create_dir_all(&aus).expect("Ausgabe-Ordner");
    let mut x = lesen(&ein);
    let n = ((sekunden * 48_000.0) as usize).min(x.len()) / BLOCK * BLOCK;
    x.truncate(n);
    // 3 s Stille am Ende, damit man den Nachhall hört.
    x.extend(vec![0.0; 48_000 * 3 / BLOCK * BLOCK]);
    for (i, u) in Umgebung::ALLE.into_iter().enumerate() {
        let (mut e, _st) = umgebung(Some((u, u.staerke_standard())));
        let mut y = x.clone();
        let t = std::time::Instant::now();
        for b in y.chunks_mut(BLOCK) {
            e.verarbeite(b).unwrap();
        }
        let dauer = t.elapsed().as_secs_f32() / (y.len() as f32 / 48_000.0) * 100.0;
        let datei = aus.join(format!("{}_{:?}.wav", i + 1, u).to_lowercase());
        wav_schreiben(&datei, &y).expect("schreiben");
        println!("{:<16} {} (Rechenzeit {dauer:.2} % der Spielzeit)", u.name(), datei.display());
    }
}
