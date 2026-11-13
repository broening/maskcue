//! Rechnet eine Aufnahme (48 kHz, f32-WAV) durch die KI-Stimme, wie live in
//! Blöcken zu 10 ms, und speichert je Ziel-Stimme eine WAV-Datei.
//!
//! cargo run --release --example ki_wav -- <modell> <eingang.wav> <ausgabe-ordner> <sprecher:halbtoene> ... [--sekunden n]
//!
//! Mit `--referenz <y.f32>` (24 kHz, ohne Verzögerung, vom Trainer) misst es
//! die Verzögerung über den Vergleich der Hüllkurven.

use std::path::Path;
use std::time::Instant;

use maskcue_lib::audio::denoise::BLOCK;
use maskcue_lib::audio::ki::{KERN_VERZOEGERUNG, KiKern};
use maskcue_lib::audio::ki::beatrice::Rechner;
use maskcue_lib::audio::mitschnitt::wav_schreiben;

fn huelle(x: &[f32], fenster: usize) -> Vec<f32> {
    x.chunks(fenster).map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt()).collect()
}

fn main() {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let referenz = a.iter().position(|s| s == "--referenz").map(|i| {
        let p = a.remove(i + 1);
        a.remove(i);
        p
    });
    // --sekunden <n>: länger als die üblichen 20 s rechnen (z. B. eine ganze Strophe).
    let sekunden: usize = a.iter().position(|s| s == "--sekunden").map_or(20, |i| {
        let n = a.remove(i + 1);
        a.remove(i);
        n.parse().expect("--sekunden")
    });
    let modell = Path::new(&a[0]);
    let bytes = std::fs::read(&a[1]).expect("Eingang lesen");
    let mut x: Vec<f32> = bytes[44..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    x.truncate((x.len() / BLOCK * BLOCK).min(48000 * sekunden / BLOCK * BLOCK));
    let ordner = Path::new(&a[2]);
    std::fs::create_dir_all(ordner).unwrap();

    for wahl in &a[3..] {
        // sprecher:halbtoene[:laut_anteil]
        let teile: Vec<&str> = wahl.split(':').collect();
        // Mischung: 42+133 oder 42*0.7+133*0.3
        let mischung: Vec<(usize, f32)> = teile[0]
            .split('+')
            .map(|t| match t.split_once('*') {
                Some((n, g)) => (n.parse().unwrap(), g.parse().unwrap()),
                None => (t.parse().unwrap(), 1.0),
            })
            .collect();
        let sprecher = teile[0].replace(['+', '*'], "_");
        let halbtoene: f32 = teile[1].parse().unwrap();
        let anteil: f32 = teile.get(2).map_or(1.0, |v| v.parse().unwrap());
        let mut r = Rechner::laden(modell).expect("Modell");
        r.mischung_setzen(&mischung);
        r.tonhoehe_setzen(halbtoene);
        r.laut_anteil_setzen(anteil);
        let mut e = KiKern::new(r);
        let mut y = x.clone();
        let t0 = Instant::now();
        let mut laengster = 0.0f64;
        for b in y.chunks_exact_mut(BLOCK) {
            let t = Instant::now();
            e.block(b);
            laengster = laengster.max(t.elapsed().as_secs_f64());
        }
        let last = t0.elapsed().as_secs_f64() / (x.len() as f64 / 48000.0) * 100.0;
        let datei = ordner.join(if teile.len() > 2 { format!("ki_stimme{sprecher}_{halbtoene:+}_laute{anteil}.wav") } else { format!("ki_stimme{sprecher}_{halbtoene:+}.wav") });
        wav_schreiben(&datei, &y).unwrap();
        println!("{}: {last:.1} % eines Kerns, längster Block {:.2} ms", datei.display(), laengster * 1000.0);

        if let Some(p) = &referenz {
            // Referenz (24 kHz) und Ausgabe (48 kHz) als Hüllkurven in 1-ms-Schritten.
            let b = std::fs::read(p).unwrap();
            let r: Vec<f32> = b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
            let hr = huelle(&r, 24);
            let hy = huelle(&y, 48);
            let n = hr.len().min(hy.len()) - 200;
            let mut beste = (0, f32::MIN);
            for lag in 0..150 {
                let s: f32 = (0..n).map(|i| hr[i] * hy[i + lag]).sum();
                if s > beste.1 {
                    beste = (lag, s);
                }
            }
            println!("  Verzögerung laut Hüllkurve: {} ms (eingestellt: {:.1} ms)", beste.0, KERN_VERZOEGERUNG as f32 / 48.0);
        }
    }
}
