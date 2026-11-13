//! Geschwindigkeitstest der KI-Stimme wie in der App (Figur, KI-Stimme).
//!
//! cargo run --release --example ki_tempo -- <modelle-ordner> <modell>

use std::path::Path;

use maskcue_lib::audio::ki::{KiAnteil, KiWahl, tempo_messen};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let wahl = KiWahl { modell: a[1].clone(), stimmen: vec![KiAnteil { nr: 0, gewicht: 1.0 }], laut_anteil: 0.25, tonhoehe_auto: true };
    let t = std::time::Instant::now();
    let r = tempo_messen(&Path::new(&a[0]).join(&a[1]), &wahl).unwrap();
    println!("Mittel {:.0} %, Spitze {:.0} % (Test dauerte {:.1} s)", r.mittel * 100.0, r.spitze * 100.0, t.elapsed().as_secs_f32());
}
