//! Verständlichkeit der KI-Stimme messen: Die Laut-Erkennung (phone
//! extractor) hört einmal den Eingang und einmal die KI-Ausgabe. Je ähnlicher
//! die erkannten Laute (Kosinus, 1 = gleich), desto deutlicher die Aussprache.
//!
//! cargo run --release --example ki_klarheit -- <modell> <eingang-48k.wav> <variante> ...
//! Variante: stimmen:halbtoene[:laut_anteil]
//! z. B. 54:0  37+54:0  54:0:0.5

use std::path::Path;

use maskcue_lib::audio::denoise::BLOCK;
use maskcue_lib::audio::ki::beatrice::Rechner;
use maskcue_lib::audio::ki::gewichte::Leser;
use maskcue_lib::audio::ki::laute::Laute;
use maskcue_lib::audio::ki::{KERN_VERZOEGERUNG, KiKern, filter_fuer_test};

fn laute(modell: &Path, x16: &[f32]) -> Vec<Vec<f32>> {
    let mut l = Laute::lesen(&mut Leser::datei(&modell.join("phone_extractor.bin")).unwrap()).unwrap();
    let mut aus = Vec::new();
    for &s in x16 {
        if l.schieben(s) {
            aus.push(l.bild.clone());
        }
    }
    aus
}

fn kosinus(a: &[f32], b: &[f32]) -> f32 {
    let ab: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let aa: f32 = a.iter().map(|x| x * x).sum();
    let bb: f32 = b.iter().map(|x| x * x).sum();
    ab / (aa * bb).sqrt().max(1e-9)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let modell = Path::new(&a[0]);
    let bytes = std::fs::read(&a[1]).unwrap();
    let mut x: Vec<f32> = bytes[44..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    x.truncate((48000 * 20) / BLOCK * BLOCK);
    let (runter, _) = filter_fuer_test();
    let x16 = runter(&x);
    let ref_laute = laute(modell, &x16);
    // Nur Bilder mit Sprache (Energie im Eingang).
    let laut: Vec<bool> = x16.chunks(160).map(|c| (c.iter().map(|v| v * v).sum::<f32>() / 160.0).sqrt() > 0.01).collect();

    for v in &a[2..] {
        let t: Vec<&str> = v.split(':').collect();
        let mischung: Vec<(usize, f32)> = t[0].split('+').map(|s| (s.parse().unwrap(), 1.0)).collect();
        let mut r = Rechner::laden(modell).unwrap();
        r.mischung_setzen(&mischung);
        r.tonhoehe_setzen(t[1].parse().unwrap());
        r.laut_anteil_setzen(t.get(2).map_or(1.0, |s| s.parse().unwrap()));
        let mut k = KiKern::new(r);
        let mut y = x.clone();
        for b in y.chunks_exact_mut(BLOCK) {
            k.block(b);
        }
        // Verzögerung herausnehmen (48 kHz → 16 kHz: durch 3).
        let y16 = runter(&y);
        let v16 = (KERN_VERZOEGERUNG as f32 / 3.0).round() as usize;
        let y_laute = laute(modell, &y16[v16..]);
        let mut summe = 0.0;
        let mut n = 0;
        for (i, (p, q)) in ref_laute.iter().zip(&y_laute).enumerate() {
            if laut.get(i).copied().unwrap_or(false) {
                summe += kosinus(p, q);
                n += 1;
            }
        }
        println!("{v:<16} Laut-Ähnlichkeit {:.3} ({n} Bilder mit Sprache)", summe / n.max(1) as f32);
    }
}
