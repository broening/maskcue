//! Sound-Dateien laden wie das Soundboard. Aufruf: cargo run --example sound_laden -- <datei>...
fn main() {
    for p in std::env::args().skip(1) {
        match maskcue_lib::audio::sounddatei::laden(std::path::Path::new(&p)) {
            Ok(d) => println!("ok {:5.1} s  {p}", d.sekunden()),
            Err(f) => println!("FEHLER  {p}: {f}"),
        }
    }
}
