//! Misst die Rechenzeit der Figur-Stimme je Block, auch beim Umschalten.
//! Ein Block ist 10 ms Ton. Braucht ein Block lange, wartet auch der Chat.
//!
//! cargo run --release --example figur_zeit -- <wav>

use std::time::Instant;

use maskcue_lib::audio::denoise::BLOCK;
use maskcue_lib::audio::figur::figur;
use maskcue_lib::audio::figur::{FigurStimme, Verfahren};
use maskcue_lib::audio::verarbeitung::Effekt;

fn main() {
    let pfad = std::env::args().nth(1).expect("WAV-Datei angeben");
    let bytes = std::fs::read(&pfad).expect("WAV lesen");
    let mut x: Vec<f32> = bytes[44..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    x.truncate(x.len() / BLOCK * BLOCK);
    println!("{:.1} s Ton", x.len() as f64 / 48_000.0);

    let opa = Some(FigurStimme { halbtoene: -4.0, formant: -3.0, verfahren: Verfahren::Psola, laut_anteil: 1.0, hell: 0.0, ki: None });
    let kobold = Some(FigurStimme { halbtoene: 5.0, formant: 4.0, verfahren: Verfahren::Psola, laut_anteil: 1.0, hell: 0.0, ki: None });
    let daemon = Some(FigurStimme { halbtoene: -7.0, formant: -4.0, verfahren: Verfahren::Psola, laut_anteil: 1.0, hell: 0.0, ki: None });
    let folge = [("Opa", opa), ("Kobold", kobold), ("Dämon", daemon), ("aus", None), ("Dämon", daemon), ("Kobold", kobold)];

    // Fest: jede Figur allein.
    for (name, e) in folge.iter().take(4) {
        let (mut f, _st) = figur(*e);
        let mut y = x.clone();
        let mut zeiten = Vec::new();
        for b in y.chunks_exact_mut(BLOCK) {
            let t = Instant::now();
            f.verarbeite(b).unwrap();
            zeiten.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        bericht(&format!("fest {name}"), &zeiten);
    }

    // Umschalten alle 2 s.
    let (mut f, mut st) = figur(None);
    let mut y = x.clone();
    let mut zeiten = Vec::new();
    for (i, b) in y.chunks_exact_mut(BLOCK).enumerate() {
        if i % 200 == 100 {
            let (name, e) = folge[(i / 200) % folge.len()];
            st.setzen(e);
            let _ = name;
        }
        let t = Instant::now();
        f.verarbeite(b).unwrap();
        zeiten.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    bericht("umschalten", &zeiten);
    for (i, z) in zeiten.iter().enumerate() {
        if *z > 3.0 {
            println!("  Block {i} ({:.2} s): {z:.2} ms, {} Blöcke nach Wechsel", i as f64 / 100.0, (i + 100) % 200);
        }
    }
}

fn bericht(name: &str, z: &[f64]) {
    let mut s = z.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let p = |q: f64| s[((s.len() - 1) as f64 * q) as usize];
    println!("{name:12}: mittel {:.3} ms, 99 % {:.3} ms, max {:.2} ms, über 3 ms: {}", z.iter().sum::<f64>() / z.len() as f64, p(0.99), p(1.0), z.iter().filter(|v| **v > 3.0).count());
}
