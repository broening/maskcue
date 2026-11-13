//! Vergleicht Leicht (RNNoise), Schnell (DeepFilterNet3 ll) und Normal
//! (DeepFilterNet3) an einer echten Aufnahme (WAV, 32-Bit-Float, 48 kHz).
//!
//! Zur Aufnahme kommen bekanntes Rauschen und Tastatur-Klicks in den Pausen.
//! Die Aufnahme selbst ist die saubere Vorlage. So lässt sich messen:
//! Rechenzeit je Block, Verzögerung, Rest-Rauschen in Pausen, Rest der Klicks,
//! Fehler beim Sprechen (Ausgang minus Vorlage) und Pegel der Stimme.
//! Für das Ohr schreibt es je Modell eine WAV in den Ausgabe-Ordner.
//!
//! cargo run --example denoise_vergleich -- <ein.wav> <ausgabe-ordner> [rauschen_dbfs]

use std::time::Instant;

use maskcue_lib::audio::denoise::{BLOCK, Denoise, Modell};
use maskcue_lib::audio::mitschnitt::wav_schreiben;

const RATE: usize = 48_000;

fn wav_lesen(pfad: &str) -> Vec<f32> {
    let b = std::fs::read(pfad).expect("WAV lesen");
    let kanaele = u16::from_le_bytes([b[22], b[23]]) as usize;
    let i = b.windows(4).position(|w| w == b"data").expect("data") + 8;
    let x: Vec<f32> = b[i..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    x.chunks_exact(kanaele).map(|f| f[0]).collect()
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }
}

fn db(v: f64) -> f64 {
    20.0 * v.max(1e-9).log10()
}

fn rms(it: impl Iterator<Item = f32>) -> f64 {
    let (s, n) = it.fold((0.0f64, 0usize), |(s, n), v| (s + (v as f64).powi(2), n + 1));
    (s / n.max(1) as f64).sqrt()
}

/// Verzögerung des Ausgangs gegen die Vorlage (Kreuzkorrelation, bis 100 ms).
fn verzoegerung(vorlage: &[f32], aus: &[f32]) -> usize {
    let n = vorlage.len().min(RATE * 20);
    (0..RATE / 10)
        .map(|lag| (lag, (0..n - lag).step_by(2).map(|i| vorlage[i] as f64 * aus[i + lag] as f64).sum::<f64>()))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(0, |(l, _)| l)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let vorlage = wav_lesen(&a[0]);
    let ordner = std::path::PathBuf::from(&a[1]);
    std::fs::create_dir_all(&ordner).expect("Ordner");
    let rauschen_db: f32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(-50.0);
    let n = vorlage.len() / BLOCK * BLOCK;
    let vorlage = &vorlage[..n];

    // Blöcke der Vorlage: Stille (Pause) oder Stimme.
    let block_db: Vec<f64> = vorlage.chunks_exact(BLOCK).map(|b| db(rms(b.iter().copied()))).collect();
    let pause: Vec<bool> = (0..block_db.len()).map(|i| (i.saturating_sub(10)..(i + 10).min(block_db.len())).all(|j| block_db[j] < -55.0)).collect();
    let stimme: Vec<bool> = block_db.iter().map(|&d| d > -40.0).collect();

    // Rauschen (weiß + etwas tieferes Brummen) und Klicks in langen Pausen.
    let mut z = Lcg(7);
    let amp = 10f32.powf(rauschen_db / 20.0) * 3f32.sqrt();
    let mut tief = 0.0f32;
    let mut gemischt: Vec<f32> = vorlage
        .iter()
        .map(|&v| {
            let w = z.next();
            tief = 0.98 * tief + 0.02 * z.next() * 6.0;
            v + amp * (0.7 * w + 0.3 * tief)
        })
        .collect();
    let mut klick_bloecke = Vec::new();
    let mut i = 0;
    while i < pause.len() {
        if pause[i] && i % 37 == 0 {
            // Klick: 4 ms abklingendes Rauschen, Spitze ca. -12 dBFS.
            let start = i * BLOCK + 100;
            for k in 0..RATE / 250 {
                gemischt[start + k] += 0.25 * z.next() * (-(k as f32) / 40.0).exp();
            }
            klick_bloecke.push(i);
            i += 30;
        }
        i += 1;
    }
    wav_schreiben(&ordner.join("0_mit_rauschen.wav"), &gemischt).expect("WAV");

    let vorher_pause = rms(pause.iter().enumerate().filter(|(_, p)| **p).flat_map(|(b, _)| gemischt[b * BLOCK..(b + 1) * BLOCK].iter().copied()));
    println!("Datei: {} ({:.0} s), Rauschen {rauschen_db} dBFS, {} Klicks", a[0], n as f64 / RATE as f64, klick_bloecke.len());
    println!("Pause vorher (mit Rauschen): {:.1} dB", db(vorher_pause));
    println!();
    println!("Modell  | Rechenzeit/Block mittel / 99 % / max | Verzögerung | Pause danach | Klick-Rest | Fehler beim Sprechen | Stimme");
    for (modell, name) in [(Modell::Leicht, "leicht"), (Modell::Schnell, "schnell"), (Modell::Normal, "normal")] {
        let mut d = Denoise::laden(modell, 100.0, 0.0).expect("Modell");
        let mut aus = gemischt.clone();
        let mut zeiten: Vec<f64> = Vec::with_capacity(n / BLOCK);
        for b in aus.chunks_exact_mut(BLOCK) {
            let t = Instant::now();
            d.verarbeite(b).expect("Denoise");
            zeiten.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        let mut z = zeiten[100..].to_vec();
        z.sort_by(f64::total_cmp);
        let mittel = z.iter().sum::<f64>() / z.len() as f64;
        let p99 = z[z.len() * 99 / 100];
        let max = z[z.len() - 1];
        let lag = verzoegerung(vorlage, &aus);
        let nach = |b: usize| -> &[f32] {
            let s = (b * BLOCK + lag).min(n - BLOCK);
            &aus[s..s + BLOCK]
        };
        let pause_danach = rms(pause.iter().enumerate().skip(100).filter(|(_, p)| **p).flat_map(|(b, _)| nach(b).iter().copied()));
        // Klick-Rest: Pegel im Klick-Block danach gegen davor (mit Klick).
        let klick_vorher = rms(klick_bloecke.iter().flat_map(|&b| gemischt[b * BLOCK..(b + 1) * BLOCK].iter().copied()));
        let klick_nachher = rms(klick_bloecke.iter().flat_map(|&b| nach(b).iter().copied()));
        // Fehler beim Sprechen: Ausgang gegen Vorlage, Pegel angepasst.
        let sv = rms(stimme.iter().enumerate().filter(|(_, s)| **s).flat_map(|(b, _)| vorlage[b * BLOCK..(b + 1) * BLOCK].iter().copied()));
        let sn = rms(stimme.iter().enumerate().filter(|(_, s)| **s).flat_map(|(b, _)| nach(b).iter().copied()));
        let k = (sn / sv.max(1e-12)) as f32;
        let fehler = rms(stimme.iter().enumerate().filter(|(_, s)| **s).flat_map(|(b, _)| {
            let s = (b * BLOCK + lag).min(n - BLOCK);
            (0..BLOCK).map(move |j| (s, b, j))
        }).map(|(s, b, j)| aus[s + j] - k * vorlage[b * BLOCK + j]));
        println!(
            "{name:<7} | {mittel:6.3} ms / {p99:6.3} ms / {max:6.2} ms      | {:5.1} ms    | {:7.1} dB   | {:+6.1} dB  | {:7.1} dB unter Stimme | {:+5.1} dB",
            lag as f64 * 1000.0 / RATE as f64,
            db(pause_danach),
            db(klick_nachher) - db(klick_vorher),
            db(sn) - db(fehler),
            db(sn) - db(sv),
        );
        wav_schreiben(&ordner.join(format!("{}_{name}.wav", match modell {
            Modell::Leicht => 1,
            Modell::Schnell => 2,
            Modell::Normal => 3,
        })), &aus)
        .expect("WAV");
    }
    println!();
    println!("Ein Block ist 10 ms lang. Rechenzeit: ein Kern, ohne den Rest der Kette.");
}
