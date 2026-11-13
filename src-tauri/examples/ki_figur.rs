//! Prüft den Figur-Effekt mit KI-Stimme: PSOLA → KI → andere Mischung → aus.
//! Es darf keine Lücke (Stille) und kein Knacken an den Wechseln geben.
//!
//! cargo run --release --example ki_figur -- <modell-ordner> <eingang.wav> <ausgabe.wav>

use std::path::Path;

use maskcue_lib::audio::denoise::BLOCK;
use maskcue_lib::audio::figur::{FigurStimme, Verfahren, figur};
use maskcue_lib::audio::ki::{KiAnteil, KiWahl, vorbereiten};
use maskcue_lib::audio::mitschnitt::wav_schreiben;
use maskcue_lib::audio::verarbeitung::Effekt;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let modell = Path::new(&a[0]);
    let bytes = std::fs::read(&a[1]).unwrap();
    let mut x: Vec<f32> = bytes[44..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    x.truncate(48000 * 12 / BLOCK * BLOCK);

    let psola = FigurStimme { halbtoene: 4.0, formant: 2.0, verfahren: Verfahren::Psola, laut_anteil: 1.0, hell: 0.0, ki: None };
    let ki_a = FigurStimme { halbtoene: 0.0, formant: 0.0, verfahren: Verfahren::Ohne, laut_anteil: 1.0, hell: 0.0, ki: Some(maskcue_lib::audio::figur::KiTon { halbtoene: 0.0, formant: 0.0 }) };
    let wahl = |nr: usize| KiWahl { modell: "x".into(), stimmen: vec![KiAnteil { nr, gewicht: 1.0 }], laut_anteil: 1.0, tonhoehe_auto: false };

    let (mut f, mut st) = figur(None);
    let mut daten = None;
    let mut y = x.clone();
    let mut wechsel = Vec::new();
    for (i, b) in y.chunks_mut(BLOCK).enumerate() {
        match i {
            100 => {
                st.setzen(Some(psola));
            }
            300 => {
                // KI gewählt; das Modell „lädt“ (kommt 20 Blöcke später).
                st.setzen(Some(ki_a));
            }
            320 => {
                let k = vorbereiten(modell, &wahl(0), 0.0, 0.0).unwrap();
                daten = k.stimm_daten();
                st.ki(Box::new(k));
            }
            505 => {
                // Andere Stimme im selben Modell: nur die Mischung tauschen.
                st.ki_paket(Box::new(daten.as_ref().unwrap().paket(&[(0, 1.0)])));
            }
            660 => {
                st.setzen(None);
            }
            _ => {}
        }
        f.verarbeite(b).unwrap();
        // Wie live: Der KI-Faden hat 10 ms für jeden Block.
        std::thread::sleep(std::time::Duration::from_millis(10));
        if f.ki_laeuft() != wechsel.last().map(|w: &(usize, bool)| w.1).unwrap_or(false) {
            wechsel.push((i, f.ki_laeuft()));
        }
    }
    println!("KI läuft (Block, an): {wechsel:?}");
    // Größter Sprung von Sample zu Sample je Abschnitt um die Wechsel herum
    // gegen den typischen Sprung im Abschnitt davor.
    let sprung = |a: usize, b: usize| y[a * BLOCK..b * BLOCK].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    for (name, k) in [("PSOLA an", 100), ("KI an", 350), ("KI-Wechsel", 505), ("aus", 660)] {
        println!("{name:<11} Block {k}: Sprung am Wechsel {:.3}, davor max {:.3}", sprung(k - 1, k + 2), sprung(k - 60, k - 2));
    }
    // Stille-Lücke: leisester Block im Bereich nach den Wechseln.
    let rms = |i: usize| (y[i * BLOCK..(i + 1) * BLOCK].iter().map(|v| v * v).sum::<f32>() / BLOCK as f32).sqrt();
    let rms_x = |i: usize| (x[i * BLOCK..(i + 1) * BLOCK].iter().map(|v| v * v).sum::<f32>() / BLOCK as f32).sqrt();
    for k in [350usize, 505, 660] {
        let schlimmste = (k - 3..k + 6).map(|i| (i, rms(i), rms_x(i))).filter(|t| t.2 > 0.01).map(|t| t.1 / t.2).fold(f32::MAX, f32::min);
        println!("Wechsel {k}: kleinstes Verhältnis Ausgabe/Eingang (nur laute Blöcke) {schlimmste:.2}");
    }
    assert!(y.iter().all(|v| v.is_finite()));
    wav_schreiben(&std::path::PathBuf::from(&a[2]), &y).unwrap();
}
