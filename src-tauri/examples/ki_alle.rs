//! Rechnet dieselbe Aufnahme mit jeder Stimme eines Modells (für die
//! Einordnung der Stimmen: Geschlecht, Größe, hell, Hauch).
//!
//! cargo run --release --example ki_alle -- <modell> <eingang-48k.wav> <ausgabe.f32> [sekunden]
//! Ausgabe: alle Stimmen hintereinander, je `sekunden` · 24000 Samples (f32).

use std::io::Write;
use std::path::Path;

use maskcue_lib::audio::ki::beatrice::Rechner;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let modell = Path::new(&a[0]);
    let sekunden: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(14);
    let bytes = std::fs::read(&a[1]).unwrap();
    let x48: Vec<f32> = bytes[44..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    // 48 → 16 kHz: Mittel über 3 genügt hier (nur für die Einordnung).
    let x16: Vec<f32> = x48.chunks_exact(3).take(16000 * (sekunden + 1)).map(|c| (c[0] + c[1] + c[2]) / 3.0).collect();
    let mut r = Rechner::laden(modell).expect("Modell");
    let n = r.info.stimmen.len();
    let mut aus = std::fs::File::create(&a[2]).unwrap();
    for nr in 0..n {
        let mut r2 = Rechner::laden(modell).expect("Modell");
        r2.sprecher_setzen(nr);
        let mut y = Vec::new();
        r2.schieben(&x16, &mut y);
        y.resize(24000 * sekunden, 0.0);
        for v in &y {
            aus.write_all(&v.to_le_bytes()).unwrap();
        }
        if nr % 10 == 0 {
            println!("{nr}/{n}");
        }
    }
    r.sprecher_setzen(0);
    println!("fertig: {n} Stimmen");
}
