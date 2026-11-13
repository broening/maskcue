//! Tastatur-Klicks durch das KI-Entrauschen: Kommen die ersten Klicks nach
//! einer Pause durch? (Rückmeldung Dan: nach jeder Pause hört man zwei Klicks.)
//!
//! Erzeugt leises Grundrauschen mit Klick-Gruppen (je 8 Klicks, 150 ms
//! Abstand), dazwischen Pausen von 0,5 s, 3 s und 20 s. Misst je Klick,
//! wie laut er nach dem Entrauschen ist (dB gegenüber dem Eingang).
//!
//! cargo run --release --example klick_analyse

use maskcue_lib::audio::denoise::{BLOCK, Denoise, Modell};

const FS: f32 = 48_000.0;

struct Zufall(u32);
impl Zufall {
    fn n(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// Klick: kurzer Rausch-Stoß und ein abklingender Ton um 2,5 kHz.
fn klick(z: &mut Zufall, ziel: &mut [f32]) {
    for (i, s) in ziel.iter_mut().enumerate().take((0.02 * FS) as usize) {
        let t = i as f32 / FS;
        let stoss = z.n() * (-t / 0.0015).exp();
        let ton = (std::f32::consts::TAU * 2500.0 * t).sin() * (-t / 0.006).exp();
        *s += 0.25 * (stoss + 0.6 * ton);
    }
}

fn main() {
    let mut z = Zufall(7);
    let pausen = [0.5f32, 3.0, 20.0, 0.5];
    let mut x: Vec<f32> = Vec::new();
    let mut klicks: Vec<(usize, usize)> = Vec::new(); // (Gruppe, Start)
    let mut pos = (1.0 * FS) as usize;
    for (g, &pause) in pausen.iter().enumerate() {
        for _ in 0..8 {
            klicks.push((g, pos));
            pos += (0.15 * FS) as usize;
        }
        pos += (pause * FS) as usize;
    }
    x.resize(pos + FS as usize, 0.0);
    // Leises Grundrauschen (ca. −70 dBFS), wie ein ruhiges Zimmer.
    for s in x.iter_mut() {
        *s = z.n() * 0.0004;
    }
    for &(_, start) in &klicks {
        klick(&mut z, &mut x[start..]);
    }
    x.truncate(x.len() / BLOCK * BLOCK);

    let laut = |v: &[f32]| 20.0 * v.iter().fold(0.0f32, |m, s| m.max(s.abs())).max(1e-9).log10();
    let varianten: [(&str, Option<(f32, f32, f32)>, f32); 4] = [
        ("ab Werk (-10/30/20)", None, 0.0),
        ("immer bearbeiten (-10/100/100)", Some((-10.0, 100.0, 100.0)), 0.0),
        ("immer bearbeiten + Nachfilter 0,02", Some((-10.0, 100.0, 100.0)), 0.02),
        ("immer bearbeiten, Stille ab -5", Some((-5.0, 100.0, 100.0)), 0.0),
    ];
    for modell in [Modell::Schnell] {
        for (name, schwellen, pf) in varianten {
            let mut d = Denoise::laden(modell, 100.0, pf).expect("Modell");
            if let Some((a, b, c)) = schwellen {
                d.schwellen(a, b, c);
            }
            let v = d.verzoegerung();
            let mut y = x.clone();
            for b in y.chunks_mut(BLOCK) {
                d.verarbeite(b).unwrap();
            }
            print!("{modell:?}, {name}:");
            let mut gruppe = usize::MAX;
            for &(g, start) in &klicks {
                if g != gruppe {
                    gruppe = g;
                    print!("\n  nach Pause {:>4.1} s:", if g == 0 { 1.0 } else { pausen[g - 1] });
                }
                let ende = start + (0.04 * FS) as usize;
                let rein = laut(&x[start..ende]);
                let raus = laut(&y[start + v..(ende + v).min(y.len())]);
                print!(" {:>6.1}", raus - rein);
            }
            println!();
        }
    }
    println!("(Zahlen: dB je Klick nach dem Entrauschen; 0 = kommt voll durch, -40 = weg)");

    // Echte Stimme: gleich laut und gleich klingend mit „immer bearbeiten“?
    let pfad = std::env::args().nth(1);
    if let Some(p) = pfad {
        let bytes = std::fs::read(&p).expect("Aufnahme");
        let mut roh: Vec<f32> = bytes[44..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        roh.truncate((30.0 * FS) as usize / BLOCK * BLOCK);
        let rechnen = |schwellen: Option<(f32, f32, f32)>| {
            let mut d = Denoise::laden(Modell::Schnell, 100.0, 0.0).expect("Modell");
            if let Some((a, b, c)) = schwellen {
                d.schwellen(a, b, c);
            }
            let mut y = roh.clone();
            for b in y.chunks_mut(BLOCK) {
                d.verarbeite(b).unwrap();
            }
            y
        };
        let a = rechnen(None);
        let b = rechnen(Some((-10.0, 100.0, 100.0)));
        let rms = |v: &[f32]| (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt();
        let diff: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x - y).collect();
        println!(
            "Stimme ({}): ab Werk {:.1} dBFS, immer bearbeiten {:.1} dBFS, Unterschied-Signal {:.1} dB unter der Stimme",
            p,
            20.0 * rms(&a).log10(),
            20.0 * rms(&b).log10(),
            20.0 * (rms(&a) / rms(&diff).max(1e-12)).log10()
        );
    }
}

