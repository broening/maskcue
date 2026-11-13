//! KI-Stimme laden wie die App, in einen Test-Ordner.
//! Aufruf: MASKCUE_KI_URL=http://127.0.0.1:8766 cargo run --example ki_download -- thorsten <ziel>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let ziel = std::path::PathBuf::from(&a[2]);
    std::fs::create_dir_all(&ziel).unwrap();
    let mut schritte = 0;
    let r = maskcue_lib::app::ki_modelle::herunterladen(&a[1], &ziel, |_, _| schritte += 1);
    println!("{r:?}, {schritte} Stücke");
    for m in maskcue_lib::app::ki_modelle::liste(&ziel) {
        println!("{} ({})", m.name, m.ordner);
    }
}
