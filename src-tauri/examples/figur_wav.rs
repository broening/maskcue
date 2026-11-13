//! Rechnet eine Aufnahme (1_roh.wav) offline durch die ganze Kette und speichert
//! die Figur-Stimme mit Signalsmith und mit PSOLA zum Vergleich.
//!
//! cargo run --release --example figur_wav -- <ordner> <halbtoene> <formant>

use maskcue_lib::audio::denoise::{BLOCK, Denoise, Modell};
use maskcue_lib::audio::dynamik::{Gate, GateEinstellung, Kompressor, KompressorEinstellung};
use maskcue_lib::audio::eq::{Band, Equalizer};
use maskcue_lib::audio::mitschnitt::wav_schreiben;
use maskcue_lib::audio::psola::{Psola, PsolaEinstellung};
use maskcue_lib::audio::stimme::{Stimme, StimmEinstellung};
use maskcue_lib::audio::verarbeitung::Effekt;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let ordner = std::path::PathBuf::from(&a[0]);
    let halbtoene: f32 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(-4.0);
    let formant: f32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(-3.0);
    let bytes = std::fs::read(ordner.join("1_roh.wav")).expect("1_roh.wav lesen");
    let mut x: Vec<f32> = bytes[44..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    x.truncate(x.len() / BLOCK * BLOCK);

    // Gemeinsame Kette wie live.
    let mut kette: Vec<Box<dyn Effekt>> = vec![
        Box::new(Equalizer::new(&[Band::Hochpass { hz: 80.0 }])),
        Box::new(Denoise::laden(Modell::Schnell, 100.0, 0.0).expect("Modell")),
        Box::new(Gate::new(GateEinstellung::default())),
        Box::new(Kompressor::new(KompressorEinstellung::default())),
    ];
    for b in x.chunks_exact_mut(BLOCK) {
        for e in kette.iter_mut() {
            e.verarbeite(b).unwrap();
        }
    }
    wav_schreiben(&ordner.join("figur_0_ohne.wav"), &x).unwrap();

    let varianten: Vec<(&str, Box<dyn Effekt>)> = vec![
        ("figur_signalsmith.wav", Box::new(Stimme::new(StimmEinstellung { halbtoene, formant, fenster_ms: 40.0 }))),
        ("figur_psola.wav", Box::new(Psola::new(PsolaEinstellung { halbtoene, formant }))),
    ];
    for (name, mut e) in varianten {
        let mut y = x.clone();
        let t = std::time::Instant::now();
        for b in y.chunks_exact_mut(BLOCK) {
            e.verarbeite(b).unwrap();
        }
        let ms_pro_block = t.elapsed().as_secs_f64() * 1000.0 / (y.len() / BLOCK) as f64;
        wav_schreiben(&ordner.join(name), &y).unwrap();
        println!("{name}: Verzögerung {:.1} ms, Rechenzeit {:.3} ms pro 10-ms-Block", e.verzoegerung() as f32 / 48.0, ms_pro_block);
    }
    println!("Gespeichert in {}", ordner.display());
}
