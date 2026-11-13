//! Hörprobe wie in der App: eigene Tonhöhe messen, Ziel-Tonhöhe der Stimme
//! aus `glottix_stimmen.json`, Probe rechnen und als WAV speichern.
//!
//! cargo run --release --example ki_probe -- <modelle-ordner> <modell> <aufnahme-48k.wav> <ausgabe.wav> <nr> ...

use std::path::{Path, PathBuf};

use maskcue_lib::app::ki_modelle::{tonhoehen_schaetzer, ziel_hz};
use maskcue_lib::audio::ki::{KiAnteil, KiWahl, halbtoene_zu, probe, tonhoehe_messen};
use maskcue_lib::audio::mitschnitt::wav_schreiben;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let basis = Path::new(&a[0]);
    let modell = &a[1];
    let bytes = std::fs::read(&a[2]).unwrap();
    let mut x: Vec<f32> = bytes[44..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    x.truncate(48000 * 8);
    let eigene = tonhoehe_messen(&tonhoehen_schaetzer(basis).unwrap(), &x).unwrap().unwrap();
    println!("Eigene Tonhöhe: {eigene:.0} Hz");
    let stimmen: Vec<KiAnteil> = a[4..].iter().map(|n| KiAnteil { nr: n.parse().unwrap(), gewicht: 1.0 }).collect();
    let ziel = ziel_hz(basis, modell, &stimmen).unwrap();
    let h = halbtoene_zu(eigene, ziel);
    println!("Ziel: {ziel:.0} Hz → {h:+.1} Halbtöne");
    let wahl = KiWahl { modell: modell.clone(), stimmen, laut_anteil: 1.0, tonhoehe_auto: true };
    let t = std::time::Instant::now();
    let y = probe(&basis.join(modell), &wahl, h, 0.0, &x).unwrap();
    let rms = (y.iter().map(|v| v * v).sum::<f32>() / y.len() as f32).sqrt();
    println!("Probe: {:.1} s in {:.2} s gerechnet, gleich lang: {}, Pegel {rms:.3}", y.len() as f32 / 48000.0, t.elapsed().as_secs_f32(), y.len() == x.len());
    wav_schreiben(&PathBuf::from(&a[3]), &y).unwrap();
}
