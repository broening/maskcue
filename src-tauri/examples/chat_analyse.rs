//! Sucht verschluckte Laute im Chat-Weg. Rechnet 1_roh.wav offline in
//! mehreren Varianten durch und zählt 10-ms-Blöcke, in denen Sprache im
//! Mikrofon ist, im Ergebnis aber fast nichts mehr.
//!
//! cargo run --release --example chat_analyse -- <ordner> [von_s] [bis_s]

use maskcue_lib::audio::denoise::{BLOCK, Denoise, Modell};
use maskcue_lib::audio::dynamik::{Gate, GateEinstellung, Kompressor, KompressorEinstellung};
use maskcue_lib::audio::eq::{Band, Equalizer};
use maskcue_lib::audio::mitschnitt::wav_schreiben;
use maskcue_lib::audio::verarbeitung::Effekt;

fn lesen(p: &std::path::Path) -> Vec<f32> {
    let bytes = std::fs::read(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let mut x: Vec<f32> = bytes[44..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    x.truncate(x.len() / BLOCK * BLOCK);
    x
}

fn pegel(b: &[f32]) -> f32 {
    let e = b.iter().map(|s| s * s).sum::<f32>() / b.len() as f32;
    10.0 * e.max(1e-20).log10()
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let ordner = std::path::PathBuf::from(&a[0]);
    let von: f32 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let bis: f32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(f32::MAX);
    let mut roh = lesen(&ordner.join("1_roh.wav"));
    let mut hp = Equalizer::new(&[Band::Hochpass { hz: 80.0 }]);
    for b in roh.chunks_exact_mut(BLOCK) {
        hp.verarbeite(b).unwrap();
    }

    // (Name, Modell, Dämpfung dB, Gate-Schwelle dBFS)
    let varianten: Vec<(&str, Modell, f32, Option<f32>)> = vec![
        ("normal_live", Modell::Normal, 100.0, Some(-45.0)),
        ("normal_ohne_gate", Modell::Normal, 100.0, None),
        ("normal_gate55", Modell::Normal, 100.0, Some(-55.0)),
        ("normal_d30_ohne_gate", Modell::Normal, 30.0, None),
        ("normal_d30_gate55", Modell::Normal, 30.0, Some(-55.0)),
        ("normal_d20_ohne_gate", Modell::Normal, 20.0, None),
        ("schnell_ohne_gate", Modell::Schnell, 100.0, None),
        ("schnell_live", Modell::Schnell, 100.0, Some(-45.0)),
    ];
    // Sprache = Block mindestens 20 dB über dem Grundrauschen (10 % leisester Blöcke).
    let mut p: Vec<f32> = roh.chunks_exact(BLOCK).map(pegel).collect();
    let roh_pegel = p.clone();
    p.sort_by(|a, b| a.total_cmp(b));
    let grund = p[p.len() / 10];
    println!("Grundrauschen {grund:.1} dBFS, Sprache ab {:.1} dBFS. Bereich {von} bis {bis} s.", grund + 20.0);

    let mut laeufe: Vec<(String, Vec<f32>, usize)> = Vec::new();
    if ordner.join("6_chat.wav").exists() {
        // Echter Mitschnitt. Verzögerung: Denoise Normal = 3 Blöcke.
        laeufe.push(("LIVE 6_chat.wav".into(), lesen(&ordner.join("6_chat.wav")), 3));
    }
    for (name, modell, daempfung, gate) in varianten {
        let mut y = roh.clone();
        let mut kette: Vec<Box<dyn Effekt>> = vec![Box::new(Denoise::laden(modell, daempfung, 0.0).unwrap())];
        if let Some(schwelle_db) = gate {
            kette.push(Box::new(Gate::new(GateEinstellung { schwelle_db, ..Default::default() })));
            kette.push(Box::new(Kompressor::new(KompressorEinstellung::default())));
        }
        for b in y.chunks_exact_mut(BLOCK) {
            for e in kette.iter_mut() {
                e.verarbeite(b).unwrap();
            }
        }
        let v: usize = kette.iter().map(|e| e.verzoegerung()).sum::<usize>() / BLOCK;
        wav_schreiben(&ordner.join(format!("analyse_{name}.wav")), &y).unwrap();
        laeufe.push((name.to_string(), y, v));
    }
    // Pegel je Block, auf das Mikrofon ausgerichtet.
    let n = roh_pegel.len();
    let ausgerichtet = |y: &[f32], v: usize| -> Vec<f32> { (0..n).map(|i| if (i + v + 1) * BLOCK <= y.len() { pegel(&y[(i + v) * BLOCK..(i + v + 1) * BLOCK]) } else { -200.0 }).collect() };
    // Tastatur-Blöcke: keine Sprache, und „Schnell“ lässt etwas durch, das „Normal“ wegnimmt.
    let finde = |name: &str| laeufe.iter().find(|l| l.0 == name).map(|l| ausgerichtet(&l.1, l.2)).unwrap();
    let (sn, no) = (finde("schnell_ohne_gate"), finde("normal_ohne_gate"));
    let tastatur: Vec<usize> = (0..n).filter(|&i| roh_pegel[i] < grund + 20.0 && sn[i] > grund + 10.0 && sn[i] - no[i] > 10.0).collect();
    println!("Tastatur-/Geräusch-Blöcke ohne Sprache, die „Schnell“ durchlässt: {}", tastatur.len());
    for (name, y, v) in laeufe {
        let mut sprache = 0;
        let mut weg = Vec::new();
        for (i, &rp) in roh_pegel.iter().enumerate() {
            let t = i as f32 * BLOCK as f32 / 48_000.0;
            if t < von || t > bis || rp < grund + 20.0 || i + v >= y.len() / BLOCK {
                continue;
            }
            sprache += 1;
            let ap = pegel(&y[(i + v) * BLOCK..(i + v + 1) * BLOCK]);
            if rp - ap > 20.0 {
                weg.push(t);
            }
        }
        let pg = ausgerichtet(&y, v);
        let laut = tastatur.iter().filter(|&&i| pg[i] > grund + 10.0).count();
        let stellen: Vec<String> = weg.iter().take(12).map(|t| format!("{t:.2}")).collect();
        println!(
            "{name:20}: Sprache {sprache}, verschluckt {:3} = {:.1} %, Geräusch durch {laut:3}. Erste verschluckt: {}",
            weg.len(),
            100.0 * weg.len() as f32 / sprache.max(1) as f32,
            stellen.join(" ")
        );
    }
}
