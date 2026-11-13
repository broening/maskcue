//! Vergleicht die eigene Beatrice-Umsetzung mit dem Original-Trainer.
//!
//! Aufruf: cargo run --release --example ki_vergleich -- <modell-ordner> <vergleich-ordner>
//! Der Vergleich-Ordner kommt aus `export.py` (Python mit dem Trainer):
//! x.f32 (16 kHz Eingang), Zwischenwerte je Stufe und y.f32 (24 kHz).

use std::path::Path;
use std::time::Instant;

use maskcue_lib::audio::ki::beatrice::{Mitschnitt, Rechner};
use maskcue_lib::audio::ki::klang::{VORLAUF, Zufall};

fn lesen(p: &Path) -> Vec<f32> {
    let b = std::fs::read(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

struct Liste {
    werte: Vec<f32>,
    pos: usize,
    erster: bool,
}

impl Zufall for Liste {
    fn naechste(&mut self) -> f32 {
        if !self.erster {
            self.erster = true;
            return 0.0;
        }
        let v = self.werte[self.pos];
        self.pos += 1;
        v
    }
}

/// Referenz liegt als [Kanäle][Bilder] vor; meine Werte als [Bild][Kanäle].
fn vergleich(name: &str, referenz: &[f32], kanaele: usize, meine: &[Vec<f32>], von: usize) {
    let bilder = referenz.len() / kanaele;
    let bis = meine.len().min(bilder).saturating_sub(4);
    let (mut diff2, mut ref2, mut max) = (0.0f64, 0.0f64, 0.0f32);
    let mut schlimmstes = (0, 0.0f32);
    for t in von..bis {
        let mut d_bild = 0.0f32;
        for c in 0..kanaele {
            let r = referenz[c * bilder + t];
            let d = meine[t][c] - r;
            diff2 += (d * d) as f64;
            ref2 += (r * r) as f64;
            max = max.max(d.abs());
            d_bild = d_bild.max(d.abs());
        }
        if d_bild > schlimmstes.1 {
            schlimmstes = (t, d_bild);
        }
    }
    println!(
        "{name:<9} Bilder {von}..{bis}: rel. Fehler {:.2e}, max {:.2e} (schlimmstes Bild {} mit {:.2e})",
        (diff2 / ref2.max(1e-30)).sqrt(),
        max,
        schlimmstes.0,
        schlimmstes.1
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let modell = Path::new(&args[1]);
    let vgl = Path::new(&args[2]);
    let formen: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(vgl.join("formen.json")).unwrap()).unwrap();
    let sprecher = formen["sprecher"].as_u64().unwrap() as usize;
    let halbtoene = formen["halbtoene"].as_f64().unwrap() as f32;

    let t0 = Instant::now();
    let mut r = Rechner::laden(modell).expect("Modell laden");
    println!("Laden: {:.0} ms, {} Stimmen", t0.elapsed().as_secs_f64() * 1000.0, r.info.stimmen.len());
    r.sprecher_setzen(sprecher);
    r.tonhoehe_setzen(halbtoene);
    r.zufall_setzen(Box::new(Liste { werte: lesen(&vgl.join("anregung.f32")), pos: 0, erster: false }));
    r.mitschnitt = Some(Mitschnitt::default());

    let x = lesen(&vgl.join("x.f32"));
    let mut y = Vec::new();
    let t0 = Instant::now();
    let mut laengster = 0.0f64;
    for stueck in x.chunks(160) {
        let t = Instant::now();
        r.schieben(stueck, &mut y);
        laengster = laengster.max(t.elapsed().as_secs_f64());
    }
    let dauer = t0.elapsed().as_secs_f64();
    let sekunden = x.len() as f64 / 16000.0;
    println!(
        "Rechenzeit: {:.2} s für {:.1} s Ton = {:.1} % eines Kerns, längstes 10-ms-Stück {:.2} ms",
        dauer,
        sekunden,
        dauer / sekunden * 100.0,
        laengster * 1000.0
    );

    let m = r.mitschnitt.take().unwrap();
    vergleich("laute", &lesen(&vgl.join("phone.f32")), 128, &m.laute, 0);
    let energie: Vec<Vec<f32>> = m.energie.iter().map(|&e| vec![e]).collect();
    vergleich("energie", &lesen(&vgl.join("energy.f32")), 1, &energie, 0);
    vergleich("logits", &lesen(&vgl.join("pitch.f32")), 448, &m.logits, 0);
    vergleich("vorstufe", &lesen(&vgl.join("prenet.f32")), 256, &m.vorstufe, 50);
    vergleich("ir", &lesen(&vgl.join("ir.f32")), 512, &m.ir, 50);
    vergleich("ap", &lesen(&vgl.join("ap.f32")), 240, &m.ap, 50);
    vergleich("nf", &lesen(&vgl.join("pf.f32")), 512, &m.nf, 50);

    let ref_y = lesen(&vgl.join("y.f32"));
    let n = ref_y.len().min(y.len().saturating_sub(VORLAUF)).saturating_sub(2400);
    let von = 24000; // ab 1 s
    let (mut d2, mut r2) = (0.0f64, 0.0f64);
    for i in von..n {
        let d = y[i + VORLAUF] - ref_y[i];
        d2 += (d * d) as f64;
        r2 += (ref_y[i] * ref_y[i]) as f64;
    }
    println!("ausgabe   Samples {von}..{n}: rel. Fehler {:.2e}", (d2 / r2).sqrt());
    if let Some(p) = args.get(3) {
        let mut w = hound_lite(&y);
        std::fs::write(p, &mut w).unwrap();
        println!("geschrieben: {p}");
    }
}

/// WAV (f32, 24 kHz, mono) ohne zusätzliche Bibliothek.
fn hound_lite(y: &[f32]) -> Vec<u8> {
    let daten = (y.len() * 4) as u32;
    let mut v = Vec::new();
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + daten).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&3u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&24000u32.to_le_bytes());
    v.extend_from_slice(&(24000u32 * 4).to_le_bytes());
    v.extend_from_slice(&4u16.to_le_bytes());
    v.extend_from_slice(&32u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&daten.to_le_bytes());
    for s in y {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}
