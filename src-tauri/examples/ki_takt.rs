//! Prüft die Verzögerung der KI-Stimme genau: Effekt (48 kHz, Blöcke) gegen
//! den Kern ohne Blöcke auf demselben Eingang. Beide nutzen denselben Zufall,
//! also muss die Ausgabe bis auf die Verschiebung gleich sein.
use maskcue_lib::audio::denoise::BLOCK;
use maskcue_lib::audio::ki::{KiKern, KiStimme};
use maskcue_lib::audio::ki::beatrice::Rechner;
use maskcue_lib::audio::verarbeitung::Effekt;
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let modell = std::path::Path::new(&a[0]);
    let bytes = std::fs::read(&a[1]).unwrap();
    let mut x: Vec<f32> = bytes[44..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    x.truncate(48000 * 4 / BLOCK * BLOCK);
    // Kern im Takt (ohne Faden) und Effekt mit Faden: Der Effekt muss genau
    // einen Block später kommen und sonst gleich sein.
    let mut k = KiKern::new(Rechner::laden(modell).unwrap());
    let mut y = x.clone();
    for b in y.chunks_exact_mut(BLOCK) { k.block(b); }
    let mut e = KiStimme::new(Rechner::laden(modell).unwrap());
    let mut y2 = x.clone();
    for b in y2.chunks_exact_mut(BLOCK) { e.verarbeite(b).unwrap(); std::thread::sleep(std::time::Duration::from_millis(10)); }
    let gleich = y[..y.len() - BLOCK] == y2[BLOCK..];
    println!("Faden: genau ein Block später und gleich: {gleich}, zu spät: {}", e.zu_spaet());
    // Ein Impuls durch dieselben Filter: Runter (48→16) und Hoch (24→48) einzeln geht
    // nicht von außen; darum Kern-Ausgabe mit Einheits-Impuls am Ende vergleichen:
    // Effekt-Ausgabe muss = Hoch(Kern) sein. Kern bekommt dafür denselben 16-kHz-Strom.
    let (runter, hoch) = maskcue_lib::audio::ki::filter_fuer_test();
    let mut k16 = vec![0.0f32; 120];
    k16.extend(runter(&x));
    let mut r = Rechner::laden(modell).unwrap();
    let mut k24 = Vec::new();
    r.schieben(&k16, &mut k24);
    let k48 = hoch(&k24);
    // Beste Verschiebung (Samples, 48 kHz) zwischen k48 und y.
    let mut beste = (0usize, f64::MAX);
    for v in 0..3000 {
        let n = 48000 * 3;
        let d: f64 = (24000..n).map(|i| { let a = y[i] as f64; let b = if i >= v { k48.get(i - v).copied().unwrap_or(0.0) as f64 } else { 0.0 }; (a - b) * (a - b) }).sum();
        if d < beste.1 { beste = (v, d); }
    }
    let e2: f64 = (24000..48000 * 3).map(|i| (y[i] as f64).powi(2)).sum();
    println!("Effekt = Kern-Ausgabe um {} Samples verschoben, rel. Fehler {:.2e}", beste.0, (beste.1 / e2).sqrt());
}
