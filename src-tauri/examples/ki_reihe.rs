//! Eine Aufnahme mit allen Stimmen eines Modells rechnen (Tonhöhe automatisch),
//! je Stimme eine WAV. Für den Vergleich der Verständlichkeit.
//!
//! cargo run --release --example ki_reihe -- <modelle-ordner> <modell> <aufnahme-48k.wav> <ausgabe-ordner> [laut_anteil] [fäden] [nr,nr,…]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use maskcue_lib::app::ki_modelle::{liste, tonhoehen_schaetzer, ziel_hz};
use maskcue_lib::audio::ki::{KiAnteil, KiWahl, halbtoene_zu, probe, tonhoehe_messen};
use maskcue_lib::audio::mitschnitt::wav_schreiben;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let basis = Path::new(&a[0]);
    let modell = &a[1];
    let bytes = std::fs::read(&a[2]).unwrap();
    let x: Vec<f32> = bytes[44..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    let aus = PathBuf::from(&a[3]);
    let laut_anteil: f32 = a.get(4).map_or(1.0, |v| v.parse().unwrap());
    let faeden: usize = a.get(5).map_or(5, |v| v.parse().unwrap());
    std::fs::create_dir_all(&aus).unwrap();

    let eigene = tonhoehe_messen(&tonhoehen_schaetzer(basis).unwrap(), &x).unwrap().unwrap();
    println!("Eigene Tonhöhe: {eigene:.0} Hz");
    let nur: Option<Vec<usize>> = a.get(6).map(|v| v.split(',').map(|n| n.parse().unwrap()).collect());
    let mut stimmen = liste(basis).into_iter().find(|m| &m.ordner == modell).unwrap().stimmen;
    if let Some(nur) = &nur {
        stimmen.retain(|s| nur.contains(&s.nr));
    }
    let naechste = AtomicUsize::new(0);
    let t = std::time::Instant::now();
    std::thread::scope(|s| {
        for _ in 0..faeden {
            s.spawn(|| {
                loop {
                    let i = naechste.fetch_add(1, Ordering::Relaxed);
                    let Some(st) = stimmen.get(i) else { break };
                    let anteile = vec![KiAnteil { nr: st.nr, gewicht: 1.0 }];
                    let h = ziel_hz(basis, modell, &anteile).map_or(0.0, |z| halbtoene_zu(eigene, z).clamp(-24.0, 24.0));
                    let wahl = KiWahl { modell: modell.clone(), stimmen: anteile, laut_anteil, tonhoehe_auto: true };
                    let y = probe(&basis.join(modell), &wahl, h, 0.0, &x).unwrap();
                    wav_schreiben(&aus.join(format!("stimme_{:03}.wav", st.nr)), &y).unwrap();
                    println!("{:3} {:+5.1} HT fertig", st.nr, h);
                }
            });
        }
    });
    println!("{} Stimmen in {:.0} s", stimmen.len(), t.elapsed().as_secs_f32());
}
