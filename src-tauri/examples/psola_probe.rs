//! Rechnet eine Aufnahme mit tiefer Tonhöhe (PSOLA) durch und misst, wie stark
//! die Stimme „pulst“ (roboterhaft). Für den Vergleich vorher/nachher.
//!
//! cargo run --release --example psola_probe -- <eingang.wav> <ausgabe.wav> [halbtoene] [psola|signalsmith]

use maskcue_lib::audio::denoise::BLOCK;
use maskcue_lib::audio::mitschnitt::wav_schreiben;
use maskcue_lib::audio::psola::{Psola, PsolaEinstellung};
use maskcue_lib::audio::stimme::{StimmEinstellung, Stimme};
use maskcue_lib::audio::verarbeitung::Effekt;

/// Pulsen: Wie stark schwankt die Lautstärke innerhalb einer Schwingung?
/// Hüllkurve in 1-ms-Fenstern, Mittel der Abweichung / Mittelwert, nur laute Stellen.
fn pulsen(x: &[f32]) -> f32 {
    let f = 48;
    let h: Vec<f32> = x.chunks(f).map(|c| (c.iter().map(|s| s * s).sum::<f32>() / f as f32).sqrt()).collect();
    let max = h.iter().cloned().fold(0.0, f32::max);
    let mut summe = 0.0;
    let mut n = 0;
    for w in h.windows(10) {
        let m = w.iter().sum::<f32>() / 10.0;
        if m > max * 0.15 {
            summe += w.iter().map(|v| (v - m).abs()).sum::<f32>() / 10.0 / m;
            n += 1;
        }
    }
    summe / n.max(1) as f32
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let bytes = std::fs::read(&a[0]).expect("Eingang");
    let mut x: Vec<f32> = bytes[44..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    x.truncate((20 * 48_000) / BLOCK * BLOCK);
    let halbtoene: f32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(-12.0);
    println!("Eingang: Pulsen {:.3}", pulsen(&x));
    let verfahren = a.get(3).map(String::as_str).unwrap_or("psola");
    let mut p: Box<dyn Effekt> = if verfahren == "signalsmith" {
        Box::new(Stimme::new(StimmEinstellung { halbtoene, formant: 0.0, fenster_ms: 40.0 }))
    } else {
        Box::new(Psola::new(PsolaEinstellung { halbtoene, formant: 0.0 }))
    };
    let t = std::time::Instant::now();
    for b in x.chunks_mut(BLOCK) {
        p.verarbeite(b).unwrap();
    }
    let rechenzeit = t.elapsed().as_secs_f32() / (x.len() as f32 / 48_000.0) * 100.0;
    println!("{halbtoene} Halbtöne, {verfahren}: Pulsen {:.3}, Verzögerung {:.0} ms, Rechenzeit {rechenzeit:.2} %", pulsen(&x), p.verzoegerung() as f32 / 48.0);
    wav_schreiben(&std::path::PathBuf::from(&a[1]), &x).expect("schreiben");
}
