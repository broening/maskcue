//! Misst die Figur-Stimme für verschiedene Fenster: Verzögerung, Rechenzeit,
//! Tonhöhe und wie sauber der Klang bleibt.
//!
//! cargo run --release --example stimme_test

use std::time::Instant;

use maskcue_lib::audio::denoise::BLOCK;
use maskcue_lib::audio::stimme::{Stimme, StimmEinstellung};
use maskcue_lib::audio::verarbeitung::Effekt;

const RATE: f32 = 48_000.0;

/// Stimmähnlicher Ton: Grundton mit Vibrato, Obertöne mit zwei Formanten.
fn stimme(f0: f32, sekunden: f32) -> Vec<f32> {
    let mut phase = 0.0f32;
    (0..(sekunden * RATE) as usize)
        .map(|i| {
            let t = i as f32 / RATE;
            let f = f0 * (1.0 + 0.02 * (std::f32::consts::TAU * 5.0 * t).sin());
            phase += f / RATE;
            (1..30)
                .map(|k| {
                    let fk = f * k as f32;
                    let formant = (-((fk - 500.0) / 300.0).powi(2)).exp() + 0.6 * (-((fk - 1500.0) / 400.0).powi(2)).exp();
                    (0.1 / k as f32 + 0.3 * formant) * (std::f32::consts::TAU * k as f32 * phase).sin()
                })
                .sum::<f32>()
                * 0.15
        })
        .collect()
}

fn grundton(x: &[f32]) -> f32 {
    let (min, max) = ((RATE / 800.0) as usize, (RATE / 60.0) as usize);
    let n = x.len() - max;
    let werte: Vec<f32> = (min..=max).map(|lag| (0..n).map(|i| x[i] * x[i + lag]).sum::<f32>()).collect();
    let grenze = 0.8 * werte.iter().cloned().fold(0.0, f32::max);
    let i = (1..werte.len() - 1).find(|&i| werte[i] >= grenze && werte[i] >= werte[i - 1] && werte[i] >= werte[i + 1]).unwrap_or(0);
    RATE / (min + i) as f32
}

fn main() {
    let f0 = 130.0;
    let original = stimme(f0, 4.0);
    // --roh <ordner>: Ausgänge als rohe f32-Dateien speichern (zur Fehlersuche).
    let args: Vec<String> = std::env::args().collect();
    if let Some(ordner) = args.iter().position(|a| a == "--roh").and_then(|i| args.get(i + 1)) {
        for (fenster_ms, ht) in [(30.0f32, -5.0f32), (30.0, 5.0), (40.0, -5.0), (20.0, -5.0)] {
            let mut s = Stimme::new(StimmEinstellung { halbtoene: ht, fenster_ms, ..Default::default() });
            let mut y = original.clone();
            for b in y.chunks_exact_mut(BLOCK) {
                s.verarbeite(b).unwrap();
            }
            let bytes: Vec<u8> = y.iter().flat_map(|v| v.to_le_bytes()).collect();
            std::fs::write(format!("{ordner}/stimme_{fenster_ms}ms_{ht}ht.f32"), bytes).unwrap();
        }
        std::fs::write(format!("{ordner}/original.f32"), original.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
        return;
    }
    println!("Testton: Stimme mit {f0} Hz Grundton, 4 s.");
    println!();
    println!("Fenster | Verzögerung gemeldet / gemessen | Rechenzeit/Block mittel / max | Klang bei 0 HT (Abstand zum Original) | Tonhöhe -5 HT / +5 HT (Fehler)");
    for fenster_ms in [20.0, 30.0, 40.0, 60.0, 120.0] {
        // 0 Halbtöne: Ausgang sollte das verzögerte Original sein. Abstand = Klangfehler.
        let mut s = Stimme::new(StimmEinstellung { fenster_ms, ..Default::default() });
        let mut x = original.clone();
        let mut zeiten = Vec::new();
        for b in x.chunks_exact_mut(BLOCK) {
            let t = Instant::now();
            s.verarbeite(b).unwrap();
            zeiten.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        let gemeldet = s.verzoegerung();
        // Gemessene Verzögerung: beste Übereinstimmung mit dem Original.
        let start = RATE as usize;
        let n = 2 * RATE as usize;
        let korr = |lag: usize| (start..start + n).map(|i| x[i] as f64 * original[i - lag] as f64).sum::<f64>();
        let lag = (0..12_000).max_by(|&a, &b| korr(a).total_cmp(&korr(b))).unwrap();
        let fehler: f64 = (start..start + n).map(|i| (x[i] - original[i - lag]) as f64).map(|d| d * d).sum::<f64>();
        let signal: f64 = (start..start + n).map(|i| (original[i - lag] as f64).powi(2)).sum::<f64>();
        let abstand_db = 10.0 * (signal / fehler.max(1e-20)).log10();
        let z = &zeiten[50..];
        let mittel = z.iter().sum::<f64>() / z.len() as f64;
        let max = z.iter().cloned().fold(0.0, f64::max);

        let mut tonhoehe = Vec::new();
        for ht in [-5.0f32, 5.0] {
            let mut s = Stimme::new(StimmEinstellung { halbtoene: ht, fenster_ms, ..Default::default() });
            let mut y = original.clone();
            for b in y.chunks_exact_mut(BLOCK) {
                s.verarbeite(b).unwrap();
            }
            let soll = f0 * 2f32.powf(ht / 12.0);
            let ist = grundton(&y[2 * RATE as usize..(2.5 * RATE) as usize]);
            tonhoehe.push(12.0 * (ist / soll).log2());
        }
        println!(
            "{:5.0} ms | {:6.1} ms / {:6.1} ms              | {:6.2} ms / {:6.2} ms              | {:6.1} dB                              | {:+.2} / {:+.2} HT",
            fenster_ms,
            gemeldet as f32 / RATE * 1000.0,
            lag as f32 / RATE * 1000.0,
            mittel,
            max,
            abstand_db,
            tonhoehe[0],
            tonhoehe[1]
        );
    }
    println!();
    println!("Klang: höher = sauberer. Über ca. 20 dB klingt es nah am Original.");
}
