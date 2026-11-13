//! Laut-Merkmale einer Aufnahme ausgeben (ohne Ersetzen), für ein eigenes Codebuch.
//!
//! cargo run --release --example ki_laute_dump -- <modell-ordner> <x-16k.f32> <aus.f32>
//! Ausgabe: [Bilder][128] f32, je Bild auf Länge 1 gebracht.

use std::path::Path;

use maskcue_lib::audio::ki::beatrice::{Mitschnitt, Rechner};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut r = Rechner::laden(Path::new(&a[0])).expect("Modell laden");
    r.laut_anteil_setzen(0.0);
    r.mitschnitt = Some(Mitschnitt::default());
    let b = std::fs::read(&a[1]).unwrap();
    let x: Vec<f32> = b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    let mut y = Vec::new();
    for stueck in x.chunks(16000) {
        r.schieben(stueck, &mut y);
        y.clear();
    }
    let m = r.mitschnitt.take().unwrap();
    let mut aus = Vec::with_capacity(m.laute.len() * 128 * 4);
    for v in &m.laute {
        let l = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
        for x in v {
            aus.extend_from_slice(&(x / l).to_le_bytes());
        }
    }
    std::fs::write(&a[2], aus).unwrap();
    println!("{} Bilder", m.laute.len());
}
