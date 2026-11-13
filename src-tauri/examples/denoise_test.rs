//! Misst beide Denoise-Modelle: Rechenzeit pro Block, Rauschminderung, Verzögerung.
//!
//! cargo run --release --example denoise_test

use std::time::Instant;

use maskcue_lib::audio::denoise::{BLOCK, Denoise, Modell};

const RATE: f64 = 48_000.0;

/// Einfacher Zufall ohne Abhängigkeit.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }
}

/// Künstliche Stimme: Grundton 140 Hz mit Vibrato und Obertönen, in Silben
/// (250 ms Ton, 250 ms Pause). Dazu Rauschen.
fn testsignal(sekunden: f64) -> (Vec<f32>, Vec<f32>, Vec<bool>) {
    let n = (sekunden * RATE) as usize;
    let mut zufall = Lcg(7);
    let mut stimme = vec![0.0f32; n];
    let mut gemischt = vec![0.0f32; n];
    let mut pause = vec![false; n];
    let mut phase = 0.0f64;
    for i in 0..n {
        let t = i as f64 / RATE;
        let silbe = (t / 0.25) as usize % 2 == 0;
        let f0 = 140.0 + 8.0 * (t * 5.0 * std::f64::consts::TAU).sin();
        phase += f0 / RATE;
        let mut v = 0.0;
        if silbe {
            // Obertöne mit abfallender Stärke und zwei Formant-Buckeln (500 / 1500 Hz).
            for k in 1..30 {
                let f = f0 * k as f64;
                let formant = (-((f - 500.0) / 300.0).powi(2)).exp() + 0.6 * (-((f - 1500.0) / 400.0).powi(2)).exp();
                v += (0.15 / k as f64 + 0.3 * formant) * (phase * k as f64 * std::f64::consts::TAU).sin();
            }
        }
        stimme[i] = (v * 0.25) as f32;
        gemischt[i] = stimme[i] + 0.03 * zufall.next();
        pause[i] = !silbe;
    }
    (stimme, gemischt, pause)
}

fn rms(x: impl Iterator<Item = f32>) -> f64 {
    let (s, n) = x.fold((0.0f64, 0usize), |(s, n), v| (s + (v as f64).powi(2), n + 1));
    (s / n.max(1) as f64).sqrt()
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-12).log10()
}

/// Verzögerung über Kreuzkorrelation (Samples).
fn verzoegerung(a: &[f32], b: &[f32], max: usize) -> usize {
    let n = a.len().min(b.len()) - max;
    (0..max)
        .max_by(|&x, &y| {
            let k = |l: usize| (0..n).step_by(4).map(|i| a[i] as f64 * b[i + l] as f64).sum::<f64>();
            k(x).total_cmp(&k(y))
        })
        .unwrap()
}

fn main() {
    let (stimme, gemischt, pause) = testsignal(10.0);
    println!("Testsignal: 10 s künstliche Stimme + Rauschen.");
    println!("Rauschen in den Pausen vorher: {:.1} dB", db(rms(gemischt.iter().zip(&pause).filter(|(_, p)| **p).map(|(v, _)| *v))));
    println!();
    println!("Rauschen beim Sprechen vorher: {:.1} dB", db(rms(gemischt.iter().zip(&stimme).zip(&pause).filter(|(_, p)| !**p).map(|((g, s), _)| g - s))));
    println!();
    println!("Modell / Nachfilter | Laden    | Rechenzeit/Block mittel / max | Rest-Rauschen Pause | beim Sprechen | Stimme   | Verzögerung gemessen / erwartet");
    for (modell, nachfilter) in [(Modell::Schnell, 0.0), (Modell::Schnell, 0.02), (Modell::Schnell, 0.05), (Modell::Normal, 0.0), (Modell::Normal, 0.02)] {
        let t0 = Instant::now();
        let mut d = match Denoise::laden(modell, 100.0, nachfilter) {
            Ok(d) => d,
            Err(e) => {
                println!("{modell:?}: Fehler: {e}");
                continue;
            }
        };
        let laden = t0.elapsed().as_secs_f64() * 1000.0;
        let mut aus = gemischt.clone();
        let mut zeiten = Vec::new();
        for block in aus.chunks_exact_mut(BLOCK) {
            let t = Instant::now();
            d.verarbeite(block).expect("Denoise");
            zeiten.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        // Erste Sekunde auslassen (Einschwingen).
        let z = &zeiten[100..];
        let mittel = z.iter().sum::<f64>() / z.len() as f64;
        let max = z.iter().cloned().fold(0.0, f64::max);
        let lag = verzoegerung(&stimme, &aus, 4000);
        let start = RATE as usize;
        let rest = rms((start..aus.len() - lag).filter(|&i| pause[i] && pause[i.saturating_sub(2400)]).map(|i| aus[i + lag]));
        let stimme_vorher = rms((start..aus.len() - lag).filter(|&i| !pause[i]).map(|i| stimme[i]));
        let stimme_nachher = rms((start..aus.len() - lag).filter(|&i| !pause[i]).map(|i| aus[i + lag]));
        // Fehler beim Sprechen: Ausgang minus saubere Stimme (passend skaliert).
        let k = stimme_nachher / stimme_vorher.max(1e-12);
        let beim_sprechen = rms((start..aus.len() - lag).filter(|&i| !pause[i]).map(|i| aus[i + lag] - (k as f32) * stimme[i]));
        println!(
            "{:<19} | {:6.0}ms | {:8.2} ms / {:6.2} ms          | {:7.1} dB          | {:7.1} dB    | {:+5.1} dB | {:5.1} ms / {:5.1} ms",
            format!("{modell:?} {nachfilter}"),
            laden,
            mittel,
            max,
            db(rest),
            db(beim_sprechen),
            db(stimme_nachher) - db(stimme_vorher),
            lag as f64 / RATE * 1000.0,
            d.verzoegerung() as f64 / RATE * 1000.0,
        );
    }
    println!();
    println!("Ein Block ist 10 ms lang. Die Rechenzeit muss deutlich darunter bleiben.");
}
