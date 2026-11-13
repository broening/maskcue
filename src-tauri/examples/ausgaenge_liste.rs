//! Zeigt die Namen aller Ausgänge, wie MaskCue sie sieht (Liste in den
//! Einstellungen und Suche beim Öffnen).
//!
//! cargo run --release --example ausgaenge_liste

fn main() {
    match maskcue_lib::audio::wasapi_aus::ausgaenge() {
        Ok(namen) => {
            for n in namen {
                println!("{n:?}");
            }
        }
        Err(f) => println!("FEHLER: {f}"),
    }
}
