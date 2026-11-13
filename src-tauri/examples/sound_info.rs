//! Lädt Sound-Dateien und zeigt, was dabei herauskommt.
//!
//! cargo run --release --example sound_info -- <datei> [<datei> ...]

use maskcue_lib::audio::sounddatei::laden;

fn main() {
    for pfad in std::env::args().skip(1) {
        let t = std::time::Instant::now();
        match laden(std::path::Path::new(&pfad)) {
            Ok(s) => {
                let spitze = s.samples.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                println!(
                    "{}: {} Kanäle, {:.2} s, Spitze {:.1} dBFS, geladen in {:.0} ms",
                    s.name,
                    s.kanaele,
                    s.sekunden(),
                    20.0 * spitze.max(1e-9).log10(),
                    t.elapsed().as_secs_f64() * 1000.0
                );
            }
            Err(e) => println!("FEHLER {e}"),
        }
    }
}
