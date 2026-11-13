//! Öffnet das Gerät zum Anhören von Aufnahmen (spielt nichts ab) und sagt,
//! welches Gerät wirklich benutzt wird.
//!
//! cargo run --release --example abhoeren_probe -- "Kopfhörer (EDIFIER WH950NB)"

use maskcue_lib::app::aufnahmen::Abhoeren;

fn main() {
    let name = std::env::args().nth(1).expect("Gerätename fehlt");
    match Abhoeren::start(&name) {
        Ok(a) => println!("OK: {:?}", a.geraet),
        Err(f) => println!("FEHLER: {f}"),
    }
}
