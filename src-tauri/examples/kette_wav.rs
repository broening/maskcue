//! Eine WAV (48 kHz, f32) offline durch die Mikrofon-Kette der App schicken:
//! Hochpass 40 Hz → Denoise „leicht“ → Gate → Kompressor. Zum Prüfen, was die
//! KI-Stimme danach noch hört.
//!
//! cargo run --release --example kette_wav -- <ein.wav> <aus.wav> <dämpfung_db|aus> <gate_db|aus> [kompressor an|aus]

use maskcue_lib::audio::denoise::{BLOCK, Denoise, Modell};
use maskcue_lib::audio::dynamik::{Gate, GateEinstellung, Kompressor, KompressorEinstellung};
use maskcue_lib::audio::eq::{Band, Equalizer};
use maskcue_lib::audio::mitschnitt::wav_schreiben;
use maskcue_lib::audio::verarbeitung::Effekt;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let bytes = std::fs::read(&a[0]).expect("WAV lesen");
    let mut x: Vec<f32> = bytes[44..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    x.truncate(x.len() / BLOCK * BLOCK);
    let mut kette: Vec<Box<dyn Effekt>> = vec![Box::new(Equalizer::new(&[Band::Hochpass { hz: 40.0 }]))];
    if let Ok(db) = a[2].parse::<f32>() {
        kette.push(Box::new(Denoise::laden(Modell::Leicht, db, 0.0).expect("Denoise")));
    }
    if let Ok(db) = a[3].parse::<f32>() {
        kette.push(Box::new(Gate::new(GateEinstellung { schwelle_db: db, ..Default::default() })));
    }
    if a.get(4).map(|s| s.as_str()) != Some("aus") {
        // Dans Einstellung: -20 dB, 3:1, +4 dB.
        kette.push(Box::new(Kompressor::new(KompressorEinstellung { schwelle_db: -20.0, verhaeltnis: 3.0, aufholen_db: 4.0, ..Default::default() })));
    }
    let v: usize = kette.iter().map(|e| e.verzoegerung()).sum();
    for b in x.chunks_exact_mut(BLOCK) {
        for e in &mut kette {
            e.verarbeite(b).unwrap();
        }
    }
    // Verzögerung herausnehmen, damit die Satzgrenzen gleich bleiben.
    x.drain(..v.min(x.len()));
    wav_schreiben(&a[1].clone().into(), &x).expect("WAV schreiben");
    println!("Verzögerung {v} Samples");
}
