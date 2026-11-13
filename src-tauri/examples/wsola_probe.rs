//! Hörprobe und Vergleich für „Leicht“ (WSOLA).
//!
//! 1. Rechnet eine Aufnahme mit WSOLA bei mehreren Tonhöhen durch und speichert
//!    sie zum Anhören. Dazu PSOLA und Signalsmith bei −15 und +30.
//! 2. Rechenzeit je 10-ms-Block für alle drei Verfahren.
//! 3. Tonhöhe und Störabstand an einem stimmähnlichen Ton (ohne Aufnahme).
//!
//! cargo run --release --example wsola_probe -- [eingang.wav] [ordner] [sekunden]
//! Standard: ../aufnahmen/test1/figur_0_ohne.wav, ../aufnahmen/wsola, 20 s.

use std::path::PathBuf;
use std::time::Instant;

use maskcue_lib::audio::denoise::BLOCK;
use maskcue_lib::audio::mitschnitt::wav_schreiben;
use maskcue_lib::audio::psola::{Psola, PsolaEinstellung};
use maskcue_lib::audio::stimme::{StimmEinstellung, Stimme};
use maskcue_lib::audio::verarbeitung::Effekt;
use maskcue_lib::audio::wsola::{Wsola, WsolaEinstellung};

const RATE: f32 = 48_000.0;

fn verfahren(name: &str, halbtoene: f32) -> Box<dyn Effekt> {
    match name {
        "psola" => Box::new(Psola::new(PsolaEinstellung { halbtoene, formant: 0.0 })),
        "signalsmith" => Box::new(Stimme::new(StimmEinstellung { halbtoene, formant: 0.0, fenster_ms: 40.0 })),
        _ => Box::new(Wsola::new(WsolaEinstellung { halbtoene, formant: 0.0 })),
    }
}

/// Rechnet `x` durch und gibt die Rechenzeit je Block zurück (Mittel, größte, ms).
fn rechnen(e: &mut dyn Effekt, x: &mut [f32]) -> (f64, f64) {
    let mut zeiten = Vec::with_capacity(x.len() / BLOCK);
    for b in x.chunks_exact_mut(BLOCK) {
        let t = Instant::now();
        e.verarbeite(b).unwrap();
        zeiten.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    (zeiten.iter().sum::<f64>() / zeiten.len() as f64, zeiten.iter().cloned().fold(0.0, f64::max))
}

/// Stimmähnlicher Ton mit leichtem Vibrato (wie in den Tests).
fn stimme(f0: f32, sekunden: f32) -> Vec<f32> {
    let mut phase = 0.0f32;
    (0..(sekunden * RATE) as usize)
        .map(|i| {
            let t = i as f32 / RATE;
            let f = f0 * (1.0 + 0.01 * (std::f32::consts::TAU * 5.0 * t).sin());
            phase = (phase + f / RATE).fract();
            (1..20).map(|k| (std::f32::consts::TAU * k as f32 * phase).sin() / k as f32).sum::<f32>() * 0.2
        })
        .collect()
}

/// Grundton mit YIN, 40–1600 Hz.
fn grundton(x: &[f32]) -> f32 {
    let (tau_min, tau_max) = ((RATE / 1600.0) as usize, (RATE / 40.0) as usize);
    let w = x.len() - tau_max - 2;
    let mut d = vec![1.0f32; tau_max + 2];
    let mut summe = 0.0f32;
    for tau in 1..tau_max + 2 {
        let s: f32 = (0..w).map(|i| (x[i] - x[i + tau]).powi(2)).sum();
        summe += s;
        d[tau] = if summe > 0.0 { s * tau as f32 / summe } else { 1.0 };
    }
    let mut tau = (tau_min..=tau_max).find(|&t| d[t] < 0.15).unwrap_or_else(|| (tau_min..=tau_max).min_by(|&a, &b| d[a].total_cmp(&d[b])).unwrap());
    while tau < tau_max && d[tau + 1] < d[tau] {
        tau += 1;
    }
    let (a, b, c) = (d[tau - 1], d[tau], d[tau + 1]);
    let nenner = a - 2.0 * b + c;
    RATE / if nenner.abs() > 1e-9 { tau as f32 + 0.5 * (a - c) / nenner } else { tau as f32 }
}

/// Energie an den Obertönen von `f0` gegen den Rest (dB), 60 Hz bis 20 kHz, 1 s.
fn stoerabstand(y: &[f32], f0: f32) -> f32 {
    let n = 48_000;
    let mut planer = realfft::RealFftPlanner::<f32>::new();
    let fft = planer.plan_fft_forward(n);
    let mut ein: Vec<f32> = y[..n].iter().enumerate().map(|(i, v)| v * (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos())).collect();
    let mut aus = fft.make_output_vec();
    fft.process(&mut ein, &mut aus).unwrap();
    let (mut harm, mut rest) = (0.0f64, 0.0f64);
    for (hz, c) in aus.iter().enumerate().take(20_000).skip(60) {
        let k = (hz as f32 / f0).round().max(1.0);
        if (hz as f32 - k * f0).abs() <= (0.015 * k * f0).max(15.0) {
            harm += c.norm_sqr() as f64;
        } else {
            rest += c.norm_sqr() as f64;
        }
    }
    10.0 * (harm / rest.max(1e-12)).log10() as f32
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let eingang = PathBuf::from(a.first().map_or("../aufnahmen/test1/figur_0_ohne.wav", String::as_str));
    let ordner = PathBuf::from(a.get(1).map_or("../aufnahmen/wsola", String::as_str));
    let sekunden: f32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(20.0);
    std::fs::create_dir_all(&ordner).expect("Ordner anlegen");

    // 1. und 2.: Aufnahme.
    let bytes = std::fs::read(&eingang).expect("Eingang lesen");
    let mut x: Vec<f32> = bytes[44..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    x.truncate(((sekunden * RATE) as usize).min(x.len()) / BLOCK * BLOCK);
    println!("{}: {:.1} s", eingang.display(), x.len() as f32 / RATE);
    let mut proben: Vec<(&str, f32)> = [-15.0, -7.0, -3.0, 4.0, 12.0, 24.0, 30.0].iter().map(|&h| ("wsola", h)).collect();
    proben.extend([("psola", -15.0), ("psola", 30.0), ("signalsmith", -15.0), ("signalsmith", 30.0), ("psola", 4.0), ("signalsmith", 4.0)]);
    for (name, halbtoene) in proben {
        let mut e = verfahren(name, halbtoene);
        let mut y = x.clone();
        let (mittel, max) = rechnen(e.as_mut(), &mut y);
        let datei = ordner.join(format!("{name}_{halbtoene:+}.wav"));
        wav_schreiben(&datei, &y).expect("schreiben");
        println!(
            "{name:12} {halbtoene:+5} HT: Verzögerung {:4.1} ms, Rechenzeit je Block mittel {mittel:.4} ms, größte {max:.3} ms → {}",
            e.verzoegerung() as f32 / 48.0,
            datei.display()
        );
    }

    // 3.: Tonhöhe und Störabstand am stimmähnlichen Ton.
    println!("\nStimmähnlicher Ton (Fehler in Halbtönen, Störabstand in dB, mehr = sauberer):");
    for halbtoene in [-15.0, -7.0, 4.0, 12.0, 30.0] {
        for name in ["wsola", "psola", "signalsmith"] {
            let mut zeile = format!("{name:12} {halbtoene:+5} HT:");
            for f0 in [120.0, 170.0, 220.0] {
                let original = stimme(f0, 1.6);
                let mut y = original.clone();
                let mut e = verfahren(name, halbtoene);
                rechnen(e.as_mut(), &mut y);
                let d = e.verzoegerung();
                let soll = grundton(&original[30_000 - d..34_800 - d]) * 2f32.powf(halbtoene / 12.0);
                let fehler = 12.0 * (grundton(&y[30_000..34_800]) / soll).log2();
                zeile += &format!("  {f0} Hz {fehler:+6.2} / {:5.1} dB", stoerabstand(&y[24_000..], f0 * 2f32.powf(halbtoene / 12.0)));
            }
            println!("{zeile}");
        }
    }
}
