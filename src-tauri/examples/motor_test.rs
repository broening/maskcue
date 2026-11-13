//! Prüft den App-Motor ohne Oberfläche: starten, Regler ändern, Sound
//! auslösen, Figur wechseln, Neustart bei Geräte-Wechsel. Stumm.
//!
//! cargo run --release --example motor_test -- <mikrofon> <sound-datei>

use std::sync::mpsc;
use std::time::{Duration, Instant};

use maskcue_lib::app::einstellungen::{DenoiseModell, Einstellungen};
use maskcue_lib::app::motor::{Meldung, MotorGriff, Status};

fn warte_status(rx: &mpsc::Receiver<Meldung>, ms: u64) -> Option<Status> {
    let ende = Instant::now() + Duration::from_millis(ms);
    let mut letzter = None;
    while let Ok(m) = rx.recv_timeout(ende.saturating_duration_since(Instant::now())) {
        match m {
            Meldung::Status(s) => letzter = Some(s),
            Meldung::Hinweis(h) => println!("  Hinweis: {h}"),
            Meldung::Taste(t) => println!("  Taste: {t}"),
            Meldung::AufnahmeFertig(r) => println!("  Aufnahme: {r:?}"),
        }
    }
    letzter
}

fn zeigen(was: &str, s: &Status) {
    let aus: Vec<String> = s.ausgaenge.iter().map(|a| format!("{} {:.0} ms ({} Aussetzer)", a.name, a.latenz_ms.unwrap_or(0.0), a.aussetzer)).collect();
    println!(
        "{was:28} läuft={} fehler={:?} profil={} figur={:?} pegel={:?} sounds={:?} laden={} | {}",
        s.laeuft,
        s.fehler,
        s.profil,
        s.figur,
        s.pegel_db.map(|p| p.round()),
        s.sounds_laufen,
        s.sounds_laden,
        aus.join(", ")
    );
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let datei = std::env::temp_dir().join("glottix_motor_test").join("einstellungen.json");
    let _ = std::fs::remove_file(&datei);
    let (tx, rx) = mpsc::channel();
    let (motor, mut e) = MotorGriff::start(datei.clone(), move |m| {
        let _ = tx.send(m);
    });
    zeigen("Start ohne Mikrofon", &warte_status(&rx, 500).expect("Status"));

    e.stumm = true;
    e.geraete.eingang = a[0].clone();
    e.sounds_hinzufuegen(&[a[1].clone().into()]);
    let t = Instant::now();
    motor.einstellungen(e.clone());
    zeigen("Mikrofon gewählt", &warte_status(&rx, 2500).expect("Status"));
    println!("  (Start dauerte unter {} ms)", t.elapsed().as_millis());

    let name = e.sounds[0].name.clone();
    motor.ausloesen(name.clone());
    zeigen("Sound ausgelöst", &warte_status(&rx, 300).expect("Status"));

    // Regler im Lauf: darf keinen Neustart auslösen (Latenz-Werte laufen weiter).
    e.mikro.gate_spiel.schwelle_db = -50.0;
    e.mikro.bass_db = 4.0;
    e.mikro.kompressor.verstaerkung_db = 6.0;
    motor.einstellungen(e.clone());
    let s = warte_status(&rx, 300).expect("Status");
    zeigen("Regler geändert", &s);
    assert!(s.sounds_laufen.contains(&name), "Sound lief nach Regler-Änderung nicht mehr: Neustart?");

    motor.figur(Some("Opa".into()));
    zeigen("Figur Opa", &warte_status(&rx, 1500).expect("Status"));
    motor.figur(None);
    zeigen("Figur aus", &warte_status(&rx, 1500).expect("Status"));

    motor.alles_stopp();
    zeigen("Alles stoppen", &warte_status(&rx, 300).expect("Status"));

    // Denoise-Modell wechseln: Neustart, Figur und Sounds bleiben verfügbar.
    motor.figur(Some("Kobold".into()));
    e.mikro.denoise_spiel.modell = DenoiseModell::Normal;
    motor.einstellungen(e.clone());
    zeigen("Neustart (Modell)", &warte_status(&rx, 2500).expect("Status"));
    motor.ausloesen(name.clone());
    zeigen("Sound nach Neustart", &warte_status(&rx, 300).expect("Status"));

    // Falsches Gerät: Fehler, kein Absturz.
    e.geraete.spiel = "Gibt es nicht".into();
    motor.einstellungen(e.clone());
    zeigen("Falscher Ausgang", &warte_status(&rx, 1000).expect("Status"));

    drop(motor);
    let (g, _) = Einstellungen::laden(&datei);
    println!("Gespeichert: Gate {} dB, Bass {} dB, Ausgang \"{}\"", g.mikro.gate_spiel.schwelle_db, g.mikro.bass_db, g.geraete.spiel);
    assert_eq!(g.geraete.spiel, "Gibt es nicht");
    println!("ERGEBNIS: OK");
}
