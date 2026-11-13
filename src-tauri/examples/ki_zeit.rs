//! Rechenzeit der Teile der KI-Stimme (6 s Ton).
use std::time::Instant;
use maskcue_lib::audio::ki::gewichte::Leser;
use maskcue_lib::audio::ki::laute::Laute;
use maskcue_lib::audio::ki::tonhoehe::Tonhoehe;
use maskcue_lib::audio::ki::beatrice::Rechner;
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let o = std::path::Path::new(&a[0]);
    let x: Vec<f32> = (0..96000).map(|i| ((i as f32) * 0.03).sin() * 0.2 + ((i * 7919 % 1000) as f32 / 1000.0 - 0.5) * 0.05).collect();
    let mut l = Laute::lesen(&mut Leser::datei(&o.join("phone_extractor.bin")).unwrap()).unwrap();
    let t = Instant::now(); for &s in &x { l.schieben(s); } println!("Laute:    {:5.1} %", t.elapsed().as_secs_f64() / 6.0 * 100.0);
    let mut p = Tonhoehe::lesen(&mut Leser::datei(&o.join("pitch_estimator.bin")).unwrap()).unwrap();
    let t = Instant::now(); for &s in &x { p.schieben(s); } println!("Tonhöhe:  {:5.1} %", t.elapsed().as_secs_f64() / 6.0 * 100.0);
    let mut r = Rechner::laden(o).unwrap(); let mut y = Vec::new();
    let t = Instant::now(); r.schieben(&x, &mut y); println!("Gesamt:   {:5.1} %", t.elapsed().as_secs_f64() / 6.0 * 100.0);
}
