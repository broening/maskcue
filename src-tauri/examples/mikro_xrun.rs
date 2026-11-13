//! Zählt Lücken beim Lesen vom Mikrofon (WASAPI „Data Discontinuity“, in cpal
//! `Xrun`), ganz ohne Verarbeitung. So lässt sich trennen: Liegt eine Meldung
//! „Eingang: A buffer underrun or overrun occurred.“ am Gerät/Treiber oder an
//! MaskCue?
//!
//! cargo run --release --example mikro_xrun -- "Tischmikrofon" [sekunden] [last]
//! `last`: hält zusätzlich alle Prozessorkerne bis auf einen beschäftigt.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, StreamTrait};
use maskcue_lib::audio::geraete;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let name = a.first().expect("Mikrofon-Name fehlt");
    let sekunden: u64 = a.get(1).map_or(30, |s| s.parse().unwrap());
    let last = a.iter().any(|x| x == "last");
    let geraet = geraete::eingang(name).unwrap();
    let cfg = geraet.default_input_config().unwrap();
    println!("Gerät: {geraet}  {cfg:?}");
    let fehler = Arc::new(AtomicU64::new(0));
    let rueckrufe = Arc::new(AtomicU64::new(0));
    let proben = Arc::new(AtomicU64::new(0));
    let rate = cfg.sample_rate() as f64;
    let kanaele = cfg.channels() as u64;
    let groesste_luecke_us = Arc::new(AtomicU64::new(0));
    let (f, r, g, p) = (fehler.clone(), rueckrufe.clone(), groesste_luecke_us.clone(), proben.clone());
    let mut letzt: Option<Instant> = None;
    let protokoll = Arc::new(std::sync::Mutex::new(Vec::<(f64, usize, f64)>::new()));
    let pr = protokoll.clone();
    let t0 = Instant::now();
    let strom = geraet
        .build_input_stream::<f32, _, _>(
            cfg.into(),
            move |data: &[f32], info: &cpal::InputCallbackInfo| {
                r.fetch_add(1, Relaxed);
                let ts = info.timestamp();
                let vz = (ts.callback.as_nanos() as f64 - ts.capture.as_nanos() as f64) / 1e6;
                if let Ok(mut v) = pr.lock() {
                    if v.len() < 400 {
                        v.push((t0.elapsed().as_secs_f64() * 1000.0, data.len() / kanaele as usize, vz));
                    }
                }
                p.fetch_add(data.len() as u64 / kanaele, Relaxed);
                let jetzt = Instant::now();
                if let Some(l) = letzt {
                    g.fetch_max(jetzt.duration_since(l).as_micros() as u64, Relaxed);
                }
                letzt = Some(jetzt);
            },
            move |e| {
                f.fetch_add(1, Relaxed);
                eprintln!("{:>6.1} s  Fehler: {e}", 0.0);
            },
            None,
        )
        .unwrap();
    strom.play().unwrap();
    let stopp = Arc::new(AtomicBool::new(false));
    let mut faeden = Vec::new();
    if last {
        let n = std::thread::available_parallelism().map_or(4, |n| n.get()).saturating_sub(1);
        for _ in 0..n {
            let s = stopp.clone();
            faeden.push(std::thread::spawn(move || {
                let mut x = 1.0f64;
                while !s.load(Relaxed) {
                    x = (x * 1.000001).sin() + 1.0;
                }
                x
            }));
        }
    }
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(sekunden) {
        std::thread::sleep(Duration::from_secs(5));
        println!(
            "{:>4} s  Rückrufe {:>6}  Lücken-Meldungen {:>4}  größter Abstand {:.1} ms  Proben {:.3} s (Uhr {:.3} s)",
            t.elapsed().as_secs(),
            rueckrufe.load(Relaxed),
            fehler.load(Relaxed),
            groesste_luecke_us.load(Relaxed) as f64 / 1000.0,
            proben.load(Relaxed) as f64 / rate,
            t.elapsed().as_secs_f64()
        );
    }
    stopp.store(true, Relaxed);
    let v = protokoll.lock().unwrap();
    let mut groessen = std::collections::BTreeMap::new();
    for &(_, n, _) in v.iter() {
        *groessen.entry(n).or_insert(0) += 1;
    }
    println!("Frames je Rückruf (erste {}): {groessen:?}", v.len());
    let abst: Vec<String> = v.windows(2).take(40).map(|w| format!("{:.1}", w[1].0 - w[0].0)).collect();
    println!("Abstände (ms): {}", abst.join(" "));
    let vzs: Vec<String> = v.iter().take(20).map(|x| format!("{:.1}", x.2)).collect();
    println!("Aufnahme→Rückruf (ms): {}", vzs.join(" "));
    for f in faeden {
        let _ = f.join();
    }
}
