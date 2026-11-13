//! KI-Stimme vor einem Verfahren (Dan, 21.10.2026): Rechnet eine Aufnahme
//! einmal nur mit KI und einmal mit KI + PSOLA (+7 Halbtöne) und misst die
//! Tonhöhe. Mit PSOLA muss sie so viel höher liegen wie PSOLA ohne KI. Schreibt beide
//! Ergebnisse als WAV zum Anhören.
//!
//! cargo run --release --example ki_mit_verfahren -- <modell-ordner> <eingang.wav> <ausgabe-ordner>

use std::path::Path;

use maskcue_lib::audio::denoise::BLOCK;
use maskcue_lib::audio::figur::{FigurStimme, KiTon, Verfahren, figur};
use maskcue_lib::audio::ki::{KiAnteil, KiWahl, tonhoehe_messen, vorbereiten};
use maskcue_lib::audio::mitschnitt::wav_schreiben;
use maskcue_lib::audio::verarbeitung::Effekt;

fn rechnen(modell: &Path, x: &[f32], e: FigurStimme) -> Vec<f32> {
    let wahl = KiWahl { modell: "x".into(), stimmen: vec![KiAnteil { nr: 0, gewicht: 1.0 }], laut_anteil: 0.25, tonhoehe_auto: false };
    let (mut f, mut st) = figur(None);
    st.setzen(Some(e));
    st.ki(Box::new(vorbereiten(modell, &wahl, 0.0, 0.0).unwrap()));
    let mut y = x.to_vec();
    for b in y.chunks_mut(BLOCK) {
        f.verarbeite(b).unwrap();
        // Wie live: Der KI-Faden hat 10 ms für jeden Block.
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // Die ersten 2 s: Warmlaufen und Verzögerung.
    y.split_off(96_000)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let modell = Path::new(&a[0]);
    let bytes = std::fs::read(&a[1]).unwrap();
    let mut x: Vec<f32> = bytes[44..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    x.truncate(48000 * 15 / BLOCK * BLOCK);
    let aus = Path::new(&a[2]);
    std::fs::create_dir_all(aus).unwrap();
    let ki = Some(KiTon { halbtoene: 0.0, formant: 0.0 });
    let nur_ki = FigurStimme { halbtoene: 0.0, formant: 0.0, verfahren: Verfahren::Ohne, laut_anteil: 0.25, hell: 0.0, ki };
    let mit_psola = FigurStimme { halbtoene: 7.0, formant: 0.0, verfahren: Verfahren::Psola, laut_anteil: 0.25, hell: 0.0, ki };
    let schaetzer = modell.join("pitch_estimator.bin");
    let mut hz = Vec::new();
    for (name, e) in [("nur_ki", nur_ki), ("ki_psola_plus7", mit_psola)] {
        let y = rechnen(modell, &x, e);
        wav_schreiben(&aus.join(format!("{name}.wav")), &y).unwrap();
        let h = tonhoehe_messen(&schaetzer, &y).unwrap().unwrap_or(0.0);
        println!("{name:16} Tonhöhe {h:.0} Hz");
        hz.push(h);
    }
    // Gegenprobe ohne KI: misst der Schätzer PSOLA +7 richtig?
    for (name, e) in [("trocken", FigurStimme { verfahren: Verfahren::Ohne, ki: None, ..nur_ki }), ("psola_plus7", FigurStimme { ki: None, ..mit_psola })] {
        let y = rechnen(modell, &x, e);
        let h = tonhoehe_messen(&schaetzer, &y).unwrap().unwrap_or(0.0);
        println!("{name:16} Tonhöhe {h:.0} Hz");
        hz.push(h);
    }
    // Der Schätzer misst PSOLA +7 als ca. +5 (Median grober Stufen). Darum
    // zählt der Vergleich mit PSOLA ohne KI.
    let ohne_ki = 12.0 * (hz[3] / hz[2]).log2();
    let abstand = 12.0 * (hz[1] / hz[0]).log2();
    println!("PSOLA +7 ohne KI: {ohne_ki:+.1} Halbtöne gemessen, mit KI: {abstand:+.1}");
    assert!((abstand - ohne_ki).abs() < 1.0, "PSOLA wirkt auf die KI-Stimme anders als ohne KI");
}
