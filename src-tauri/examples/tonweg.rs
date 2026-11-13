//! Test für den Tonweg: Mikrofon → Denoise → ein oder mehrere Ausgänge.
//! Misst Latenz, Puffer und Aussetzer je Ausgang.
//!
//! cargo run --release --example tonweg -- --liste
//! cargo run --release --example tonweg -- --ein "Tischmikrofon" --aus "soundux" --aus "MaskCue Chat" --sekunden 120
//! Optionen: --denoise schnell|normal|aus, --daempfung 100, --nachfilter 0.02, --stumm,
//!           --gate -45|aus (Schwelle dBFS, nur Spiel), --gate-chat aus|-45 (Standard aus),
//!           --kompressor -20:3|aus (Schwelle:Verhältnis),
//!           --hochpass 80|aus (Hz),
//!           --denoise-chat normal|schnell|aus (Standard: normal, wenn --denoise gesetzt),
//!           --pitch-art psola|signalsmith (Standard psola),
//!           --pitch 0 (Halbtöne), --formant 0 (Halbtöne), --fenster 40 (ms). Die Figur-Stimme
//!           gilt nur für den ersten Ausgang (Spiel), nicht für den zweiten (Chat).
//!           --mitschnitt <ordner>: jede Stufe als WAV speichern (1_roh, 2_denoise, 3_gate,
//!           4_kette, 5_figur, 6_chat).
//!           --sound <datei> (nur Spiel), --sound-beide <datei> (Spiel + Chat),
//!           --musik <datei> (Loop, wird beim Sprechen leiser), --sounds-ordner <ordner>
//!           (alle Ton-Dateien im Ordner, nur Spiel). Mehrfach möglich.
//!           Während des Laufs: Nummer + Enter = Sound auslösen, s + Enter = alles stoppen,
//!           q + Enter = beenden.
//!           --hotkeys <datei.json>: Figuren, Profile und Hotkeys (Beispiel:
//!           examples/hotkeys_beispiel.json). Sounds heißen wie ihre Datei.
//!           --tasten: zeigt jede gedrückte Taste mit ihrem Namen (löst nichts aus).
//!           --puffer-ms 10, --vorlage 576, --intervall 10. --aus darf mehrfach vorkommen.

use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use maskcue_lib::audio::denoise::{Denoise, Modell};
use maskcue_lib::audio::dynamik::{Gate, GateEinstellung, Kompressor, KompressorEinstellung, PegelAnzeige, Pegelmesser};
use maskcue_lib::audio::eq::{Band, Equalizer};
use maskcue_lib::audio::figur::{FigurDef, figur as figur_neu};
use maskcue_lib::audio::mitschnitt::Mitschnitt;
use maskcue_lib::audio::figur::{FigurStimme, Verfahren};
use maskcue_lib::audio::sounddatei;
use maskcue_lib::audio::soundboard::SoundEinstellung;
use maskcue_lib::audio::stimme::{Stimme, StimmEinstellung};
use maskcue_lib::audio::geraete;
use maskcue_lib::audio::pipe::{PipeConfig, Stats};
use maskcue_lib::audio::tonweg;
use maskcue_lib::audio::verarbeitung::{Effekt, Kette, KettenBau};
use maskcue_lib::hotkeys::profil::{Aktion, HotkeyEinstellungen};
use maskcue_lib::hotkeys::{Ereignis, Hotkeys};

/// Inhalt der Datei für --hotkeys.
#[derive(serde::Deserialize, Default)]
struct HotkeyDatei {
    #[serde(default)]
    figuren: Vec<FigurDef>,
    #[serde(flatten)]
    hotkeys: HotkeyEinstellungen,
}

/// Eingaben für die Haupt-Schleife: Zeilen aus dem Fenster und Hotkeys.
enum Eingabe {
    Zeile(String),
    Hotkey(Ereignis),
}

/// Nummer des Sounds mit diesem Datei-Namen (groß/klein egal).
fn sound_nr(namen: &[String], name: &str) -> Option<usize> {
    namen.iter().position(|n| n.to_lowercase() == name.to_lowercase())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let wert = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let alle = |name: &str| args.windows(2).filter(|w| w[0] == name).map(|w| w[1].clone()).collect::<Vec<_>>();

    if args.iter().any(|a| a == "--liste") || args.is_empty() {
        geraete_zeigen(false);
        return;
    }

    let ein = wert("--ein").unwrap_or_else(|| fehler("--ein fehlt"));
    let aus = alle("--aus");
    if aus.is_empty() {
        fehler("--aus fehlt");
    }
    let sekunden: u64 = wert("--sekunden").map_or(60, |s| s.parse().unwrap_or_else(|_| fehler("--sekunden: Zahl erwartet")));
    let mut config = PipeConfig::default();
    if let Some(ms) = wert("--puffer-ms") {
        config.puffer_ms = ms.parse().unwrap_or_else(|_| fehler("--puffer-ms: Zahl erwartet"));
    }
    if let Some(b) = wert("--vorlage") {
        config.aus_vorlage = Some(b.parse().unwrap_or_else(|_| fehler("--vorlage: Zahl erwartet")));
    }
    config.stumm = args.iter().any(|a| a == "--stumm");

    let modell_aus = |name: &str, standard: Option<Modell>| match wert(name).as_deref() {
        None => standard,
        Some("aus") => None,
        Some("schnell") => Some(Modell::Schnell),
        Some("normal") => Some(Modell::Normal),
        _ => fehler(&format!("{name}: schnell, normal oder aus")),
    };
    // Spiel (A1): schnell, wenig Verzögerung. Chat (A2): normal, nimmt die
    // Tastatur sofort weg (Entscheidung 2026-10-14).
    let modell = modell_aus("--denoise", None);
    let modell_chat = modell_aus("--denoise-chat", modell.map(|_| Modell::Normal));
    let daempfung: f32 = wert("--daempfung").map_or(100.0, |d| d.parse().unwrap_or_else(|_| fehler("--daempfung: Zahl (dB) erwartet")));
    let nachfilter: f32 = wert("--nachfilter").map_or(0.0, |d| d.parse().unwrap_or_else(|_| fehler("--nachfilter: Zahl erwartet, z. B. 0.02")));
    let aus_oder = |name: &str, standard: &str| wert(name).unwrap_or(standard.into());
    let gate = match aus_oder("--gate", "-45").as_str() {
        "aus" => None,
        v => Some(v.parse::<f32>().unwrap_or_else(|_| fehler("--gate: Schwelle in dBFS oder aus"))),
    };
    // Chat ohne Gate (Entscheidung 2026-10-14): „Normal“ nimmt das Rauschen schon
    // weg. Das Gate schnitt danach leise Laute (s, f, t) ab, gemessen mit chat_analyse.
    let gate_chat = match aus_oder("--gate-chat", "aus").as_str() {
        "aus" => None,
        v => Some(v.parse::<f32>().unwrap_or_else(|_| fehler("--gate-chat: Schwelle in dBFS oder aus"))),
    };
    let kompressor = match aus_oder("--kompressor", "-20:3").as_str() {
        "aus" => None,
        v => {
            let (t, r) = v.split_once(':').unwrap_or_else(|| fehler("--kompressor: Schwelle:Verhältnis, z. B. -20:3"));
            Some((t.parse::<f32>().unwrap_or_else(|_| fehler("--kompressor: Schwelle")), r.parse::<f32>().unwrap_or_else(|_| fehler("--kompressor: Verhältnis"))))
        }
    };
    let hochpass = match aus_oder("--hochpass", "80").as_str() {
        "aus" => None,
        v => Some(v.parse::<f32>().unwrap_or_else(|_| fehler("--hochpass: Hz oder aus"))),
    };
    let zahl = |name: &str, standard: f32| wert(name).map_or(standard, |v| v.parse().unwrap_or_else(|_| fehler(&format!("{name}: Zahl erwartet"))));
    let figur = StimmEinstellung { halbtoene: zahl("--pitch", 0.0), formant: zahl("--formant", 0.0), fenster_ms: zahl("--fenster", 40.0) };
    let mit_figur = wert("--pitch").is_some() || wert("--formant").is_some();
    let psola = match wert("--pitch-art").as_deref() {
        None | Some("psola") => true,
        Some("signalsmith") => false,
        _ => fehler("--pitch-art: psola oder signalsmith"),
    };
    let hotkey_datei: Option<HotkeyDatei> = wert("--hotkeys").map(|pfad| {
        let text = std::fs::read_to_string(&pfad).unwrap_or_else(|e| fehler(&format!("--hotkeys {pfad}: {e}")));
        serde_json::from_str(&text).unwrap_or_else(|e| fehler(&format!("--hotkeys {pfad}: {e}")))
    });
    let tasten_zeigen = args.iter().any(|a| a == "--tasten");
    if hotkey_datei.is_some() && !psola {
        fehler("--hotkeys: Figuren-Wechsel geht nur mit PSOLA");
    }
    // Figur-Stimme zum Umschalten (PSOLA). Startet mit --pitch/--formant oder aus.
    let umschaltbar = psola && (mit_figur || hotkey_datei.is_some());
    let (mut figur_effekt, mut figur_steuerung) = if umschaltbar {
        let start = mit_figur.then_some(FigurStimme { halbtoene: figur.halbtoene, formant: figur.formant, verfahren: Verfahren::Psola, laut_anteil: 1.0, hell: 0.0, ki: None });
        let (f, st) = figur_neu(start);
        (Some(f), Some(st))
    } else {
        (None, None)
    };
    let pegel = PegelAnzeige::new();
    let pegel_kette = pegel.clone();
    let mitschnitt = wert("--mitschnitt").map(std::path::PathBuf::from);
    let mitschnitt_ordner = mitschnitt.clone();
    if let Some(m) = &mitschnitt {
        std::fs::create_dir_all(m).unwrap_or_else(|e| fehler(&format!("--mitschnitt: {e}")));
    }
    let bau: KettenBau = Box::new(move || {
        let mut kette = Kette::default();
        let ms = |name: &str| -> Option<Box<dyn Effekt>> { mitschnitt.as_ref().map(|m| Box::new(Mitschnitt::new(m.join(name))) as Box<dyn Effekt>) };
        // Gemeinsam: Hochpass (Brummen raus). Danach je Ausgang eigene Kette:
        // Denoise (Modell je Ausgang), Gate, Kompressor, beim Spiel die Figur.
        kette.gemeinsam.extend(ms("1_roh.wav"));
        if let Some(hz) = hochpass {
            kette.gemeinsam.push(Box::new(Equalizer::new(&[Band::Hochpass { hz }])));
        }
        let mut pegel_kette = Some(pegel_kette);
        for (nr, m) in [modell, modell_chat].into_iter().enumerate() {
            let spiel = nr == 0;
            let mut weg: Vec<Box<dyn Effekt>> = Vec::new();
            if let Some(m) = m {
                weg.push(Box::new(Denoise::laden(m, daempfung, nachfilter)?));
            }
            if spiel {
                weg.extend(ms("2_denoise.wav"));
                weg.push(Box::new(Pegelmesser(pegel_kette.take().expect("einmal"))));
            }
            if let Some(schwelle_db) = if spiel { gate } else { gate_chat } {
                weg.push(Box::new(Gate::new(GateEinstellung { schwelle_db, ..Default::default() })));
            }
            if let Some((schwelle_db, verhaeltnis)) = kompressor {
                weg.push(Box::new(Kompressor::new(KompressorEinstellung { schwelle_db, verhaeltnis, ..Default::default() })));
            }
            if spiel {
                weg.extend(ms("4_kette.wav"));
                if let Some(f) = figur_effekt.take() {
                    weg.push(Box::new(f));
                    weg.extend(ms("5_figur.wav"));
                } else if mit_figur {
                    weg.push(Box::new(Stimme::new(figur)));
                    weg.extend(ms("5_figur.wav"));
                }
            } else {
                weg.extend(ms("6_chat.wav"));
            }
            kette.pro_ausgang.push(weg);
        }
        Ok(kette)
    });

    let namen: Vec<&str> = aus.iter().map(|s| s.as_str()).collect();
    let mut tw = tonweg::start(&ein, &namen, config, bau, true).unwrap_or_else(|e| {
        geraete_zeigen(true);
        fehler(&e)
    });
    let i = &tw.info;
    println!("Eingang: {} ({} Hz, {} Kanäle, {}, Block {:?})", i.eingang, i.ein_rate, i.ein_kanaele, i.ein_format, i.ein_block);
    for (n, a) in tw.ausgaenge.iter().enumerate() {
        let ai = &a.info;
        println!(
            "Ausgang A{}: {} ({} Kanäle, Block {}, Vorlage {}), Effekte {:.1} ms",
            n + 1,
            ai.name,
            ai.kanaele,
            ai.periode_frames,
            ai.vorlage_frames,
            a.effekt_verzoegerung_ms
        );
    }
    println!(
        "Hochpass: {}. Denoise Spiel: {:?}, Chat: {:?}. Gate Spiel: {}, Chat: {}. Kompressor: {}.",
        hochpass.map_or("aus".into(), |h| format!("{h} Hz")),
        modell,
        modell_chat,
        gate.map_or("aus".into(), |g| format!("ab {g} dBFS")),
        gate_chat.map_or("aus".into(), |g| format!("ab {g} dBFS")),
        kompressor.map_or("aus".into(), |(t, r)| format!("ab {t} dBFS, {r}:1"))
    );
    if umschaltbar && !mit_figur {
        println!("Figur-Stimme auf A1: aus (ein Hotkey schaltet sie ein). PSOLA.");
    }
    if mit_figur {
        if psola {
            println!("Figur-Stimme auf A1: {:+} Halbtöne, Formant {:+}, PSOLA.", figur.halbtoene, figur.formant);
        } else {
            println!("Figur-Stimme auf A1: {:+} Halbtöne, Formant {:+}, Signalsmith mit {} ms Fenster.", figur.halbtoene, figur.formant, figur.fenster_ms);
        }
    }
    println!(
        "Puffer-Ziel ab: {} ms (passt sich an). Läuft {} s.{}",
        config.puffer_ms,
        sekunden,
        if config.stumm { " STUMM: Ausgänge geben nur Stille aus." } else { "" }
    );
    println!();
    println!("  Zeit | Aus | Latenz min / mittel / max | Eingang | Puffer | Ausgang | Ziel   | Drift   | Reserve | Lücke  | Aussetzer Aus-leer Überlauf Kürzung Fehler");

    // Anlauf: 2 s warten, dann Zähler neu starten.
    std::thread::sleep(Duration::from_secs(2));
    let alle_stats: Vec<&Stats> = std::iter::once(&*tw.eingang_stats).chain(tw.ausgaenge.iter().map(|a| &*a.stats)).collect();
    pegel.fenster();
    for s in &alle_stats {
        let anlauf = s.zaehler_zuruecksetzen();
        s.latenz_fenster();
        s.fuell_min_fenster();
        s.ein_luecke_fenster();
        if let Some(text) = s.letzter_fehler.lock().ok().and_then(|mut f| f.take()) {
            println!("Anlauf: {} Ereignisse (zuletzt: {text}). Zählt nicht.", anlauf.iter().sum::<u64>());
        }
    }

    // Sounds laden.
    let mut sound_ids = Vec::new();
    let mut sound_namen: Vec<String> = Vec::new();
    let mut alle_sounds: Vec<(String, SoundEinstellung)> = Vec::new();
    for w in args.windows(2) {
        let e = match w[0].as_str() {
            "--sound" => SoundEinstellung::default(),
            "--sound-beide" => SoundEinstellung { ziele: [true, true, false, false], ..Default::default() },
            "--musik" => SoundEinstellung { musik: true, schleife: true, lautstaerke: 0.5, ..Default::default() },
            _ => continue,
        };
        alle_sounds.push((w[1].clone(), e));
    }
    // --sounds-ordner: jede Ton-Datei im Ordner wird ein Sound (nur Spiel).
    for ordner in alle("--sounds-ordner") {
        let mut dateien: Vec<String> = std::fs::read_dir(&ordner)
            .unwrap_or_else(|e| fehler(&format!("--sounds-ordner {ordner}: {e}")))
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|x| x.to_str()).is_some_and(|x| ["mp3", "wav", "ogg", "flac"].contains(&x.to_lowercase().as_str())))
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        dateien.sort();
        alle_sounds.extend(dateien.into_iter().map(|d| (d, SoundEinstellung::default())));
    }
    for (pfad, e) in alle_sounds {
        match sounddatei::laden(std::path::Path::new(&pfad)) {
            Ok(d) => {
                let nr = sound_ids.len() + 1;
                println!(
                    "Sound {nr}: {} ({:.1} s, {} Kanäle){}{}",
                    d.name,
                    d.sekunden(),
                    d.kanaele,
                    if e.ziele[1] { ", Spiel + Chat" } else { ", nur Spiel" },
                    if e.musik { ", Musik mit Ducking, Loop" } else { "" }
                );
                sound_namen.push(d.name.clone());
                sound_ids.push(tw.soundboard.hinzufuegen(d, e));
            }
            Err(e) => println!("Sound nicht geladen: {e}"),
        }
    }
    if !sound_ids.is_empty() {
        println!("Bedienung: Nummer + Enter = Sound an/aus, s + Enter = alles stoppen, q + Enter = beenden.");
    }
    let (tasten_tx, tasten) = std::sync::mpsc::channel::<Eingabe>();
    let hotkey_tx = tasten_tx.clone();
    std::thread::spawn(move || {
        for zeile in std::io::stdin().lines().map_while(Result::ok) {
            if tasten_tx.send(Eingabe::Zeile(zeile.trim().to_string())).is_err() {
                break;
            }
        }
    });

    let figuren: Vec<FigurDef> = hotkey_datei.as_ref().map(|d| d.figuren.clone()).unwrap_or_default();
    let _hotkeys = if hotkey_datei.is_some() || tasten_zeigen {
        let e = hotkey_datei.map(|d| d.hotkeys).unwrap_or_default();
        for p in &e.profile {
            let programme = if p.programme.is_empty() { "(nur von Hand)".to_string() } else { p.programme.join(", ") };
            println!("Profil „{}“ für {programme}: {} Hotkeys.", p.name, p.belegungen.len());
        }
        for b in &e.global {
            println!("Global: {} = {}", b.taste, b.aktion);
        }
        for w in e.pruefen() {
            println!("Hotkeys, Warnung: {w}");
        }
        for b in e.global.iter().chain(e.profile.iter().flat_map(|p| &p.belegungen)) {
            match &b.aktion {
                Aktion::Sound(n) if sound_nr(&sound_namen, n).is_none() => println!("Hotkeys, Warnung: {}: Sound „{n}“ ist nicht geladen.", b.taste),
                Aktion::Figur(n) if !figuren.iter().any(|f| f.name.eq_ignore_ascii_case(n)) => println!("Hotkeys, Warnung: {}: Figur „{n}“ gibt es nicht.", b.taste),
                _ => {}
            }
        }
        let h = Hotkeys::start(e, move |e| {
            let _ = hotkey_tx.send(Eingabe::Hotkey(e));
        })
        .unwrap_or_else(|e| fehler(&e));
        if tasten_zeigen {
            h.lernen(true);
            println!("Tasten-Anzeige an: Jede Taste erscheint mit ihrem Namen. Hotkeys lösen nicht aus.");
        }
        Some(h)
    } else {
        None
    };

    let intervall = Duration::from_secs(wert("--intervall").map_or(10, |s| s.parse().unwrap_or(10)));
    let start = Instant::now();
    let ende = start + Duration::from_secs(sekunden);
    let ein_s = &tw.eingang_stats;
    let mut gesamt_min = f64::MAX;
    let mut gesamt_max: f64 = 0.0;
    let mut beenden = false;
    while Instant::now() < ende && !beenden {
        // Warten bis zum nächsten Bericht, dabei Tasten annehmen.
        let bericht = Instant::now() + intervall.min(ende.saturating_duration_since(Instant::now()));
        while Instant::now() < bericht {
            match tasten.recv_timeout(bericht.saturating_duration_since(Instant::now()).min(Duration::from_millis(100))) {
                Ok(Eingabe::Hotkey(e)) => {
                    let zeit = start.elapsed().as_secs_f32();
                    match e {
                        Ereignis::Aktion { aktion, taste, profil } => {
                            println!("{zeit:7.1}s | Hotkey {taste} ({profil}): {aktion}");
                            match aktion {
                                Aktion::Sound(n) => match sound_nr(&sound_namen, &n) {
                                    Some(i) => tw.soundboard.ausloesen(sound_ids[i]),
                                    None => println!("          Sound „{n}“ ist nicht geladen."),
                                },
                                Aktion::AllesStopp => tw.soundboard.alles_stopp(),
                                Aktion::Figur(n) => match (figuren.iter().find(|f| f.name.eq_ignore_ascii_case(&n)), figur_steuerung.as_mut()) {
                                    (Some(f), Some(st)) => {
                                        st.setzen(Some(f.einstellung()));
                                        println!("          Figur „{}“: {:+} Halbtöne, Formant {:+}.", f.name, f.halbtoene, f.formant);
                                    }
                                    _ => println!("          Figur „{n}“ gibt es nicht."),
                                },
                                Aktion::FigurAus => {
                                    if let Some(st) = figur_steuerung.as_mut() {
                                        st.setzen(None);
                                    }
                                }
                                // Gibt es in diesem Test-Programm nicht (nur in der App).
                                Aktion::Profil(_)
                                | Aktion::ProfilAutomatisch
                                | Aktion::StummUmschalten
                                | Aktion::Umgebung(_)
                                | Aktion::UmgebungAus
                                | Aktion::AufnahmeUmschalten => {}
                            }
                        }
                        Ereignis::Profil { name, von_hand, programm, admin_problem } => {
                            let art = if von_hand { "von Hand" } else { "automatisch" };
                            println!("{zeit:7.1}s | Profil: {name} ({art}, Programm im Vordergrund: {programm})");
                            if admin_problem {
                                println!("          Achtung: {programm} läuft vielleicht als Administrator. Dann sieht MaskCue dort keine Tasten.");
                            }
                        }
                        Ereignis::Gedrueckt(k) => println!("{zeit:7.1}s | Taste: {k}"),
                        Ereignis::Fehler(f) => println!("{zeit:7.1}s | Hotkey-Fehler: {f}"),
                    }
                }
                Ok(Eingabe::Zeile(t)) if t == "q" => {
                    beenden = true;
                    break;
                }
                Ok(Eingabe::Zeile(t)) if t == "s" => tw.soundboard.alles_stopp(),
                Ok(Eingabe::Zeile(t)) => match t.parse::<usize>().ok().and_then(|n| sound_ids.get(n.wrapping_sub(1))) {
                    Some(&id) => tw.soundboard.ausloesen(id),
                    None if sound_ids.is_empty() => println!("Keine Sounds geladen. Start mit --sounds-ordner <ordner> oder --sound <datei>."),
                    None => println!("Unbekannt: \"{t}\". Nummern 1 bis {}, s = alles stoppen, q = beenden.", sound_ids.len()),
                },
                Err(_) => {}
            }
            tw.soundboard.aufraeumen();
        }
        for n in 0..tw.ausgaenge.len() {
            if let Some(a) = tw.soundboard.anzeige(n) {
                let sp = a.spuren.load(Relaxed);
                let lim = a.limiter_cdb.load(Relaxed) as f32 / 100.0;
                let duck = a.ducking_cdb.load(Relaxed) as f32 / 100.0;
                if sp > 0 || lim > 0.1 || duck > 0.1 {
                    println!("{:5}s | A{} | Sounds: {sp} laufen, Ducking -{duck:.1} dB, Limiter -{lim:.1} dB", start.elapsed().as_secs(), n + 1);
                }
            }
        }
        if let Some((leise, laut)) = pegel.fenster() {
            println!("{:5}s | Pegel vor dem Gate: leisester Block {leise:6.1} dBFS, lautester {laut:6.1} dBFS", start.elapsed().as_secs());
        }
        for (n, a) in tw.ausgaenge.iter().enumerate() {
            let s = &a.stats;
            let text = match s.latenz_fenster() {
                Some(l) => {
                    gesamt_min = gesamt_min.min(l.min_ms);
                    gesamt_max = gesamt_max.max(l.max_ms);
                    format!("{:5.1} / {:5.1} / {:5.1} ms", l.min_ms, l.mittel_ms, l.max_ms)
                }
                None => "      keine Daten       ".into(),
            };
            println!(
                "{:5}s | A{} | {text} | {:5.1}ms | {:4.1}ms | {:5.1}ms | {:4.1}ms | {:+5}ppm | {:5.1}ms | {:4.1}ms | {:9} {:8} {:8} {:7} {:6}",
                start.elapsed().as_secs(),
                n + 1,
                ein_s.ein_verzoegerung_us.load(Relaxed) as f64 / 1000.0,
                s.fuellstand_us.load(Relaxed) as f64 / 1000.0,
                s.aus_verzoegerung_us.load(Relaxed) as f64 / 1000.0,
                s.ziel_us.load(Relaxed) as f64 / 1000.0,
                s.korrektur_ppm.load(Relaxed),
                s.fuell_min_fenster().unwrap_or(0.0),
                s.ein_luecke_fenster(),
                s.aussetzer.load(Relaxed),
                s.ausgang_leer.load(Relaxed),
                s.ueberlaeufe.load(Relaxed) + ein_s.ueberlaeufe.load(Relaxed),
                s.kuerzungen.load(Relaxed),
                s.strom_fehler.load(Relaxed) + ein_s.strom_fehler.load(Relaxed),
            );
        }
    }

    println!();
    let mut probleme = 0;
    for s in &alle_stats {
        probleme += s.zaehler_zuruecksetzen().iter().sum::<u64>();
        if let Some(f) = s.letzter_fehler.lock().ok().and_then(|f| f.clone()) {
            println!("Letzter Fehler: {f}");
        }
    }
    for (n, a) in tw.ausgaenge.iter().enumerate() {
        let i = &a.stats.aussetzer_info;
        if i[0].load(Relaxed) > 0 {
            println!(
                "A{}: letzter Aussetzer nach {:.1} s Laufzeit. Ziel {:.1} ms, Füllstand davor {:.1} ms, gemerkte Lücke {:.1} ms.",
                n + 1,
                i[0].load(Relaxed) as f64 / 1000.0,
                i[1].load(Relaxed) as f64 / 1000.0,
                i[2].load(Relaxed) as f64 / 1000.0,
                i[3].load(Relaxed) as f64 / 1000.0
            );
        }
    }
    if gesamt_max > 0.0 {
        println!("Latenz gesamt: {gesamt_min:.1} bis {gesamt_max:.1} ms");
    }
    // Tonweg beenden, bevor das Programm endet: dann schreiben die Mitschnitte
    // ihre Dateien (process::exit würde das überspringen).
    drop(alle_stats);
    drop(tw);
    if let Some(m) = &mitschnitt_ordner {
        println!("Mitschnitt gespeichert in {}", m.display());
    }
    if probleme == 0 {
        println!("ERGEBNIS: OK. Keine Aussetzer, kein Überlauf, keine Fehler.");
    } else {
        println!("ERGEBNIS: PROBLEM. {probleme} Ereignisse (siehe Tabelle).");
        std::process::exit(1);
    }
}

/// Zeigt, welche Geräte Windows kennt. `fehler`: auf stderr.
fn geraete_zeigen(fehler_ausgabe: bool) {
    let zeile = |t: String| if fehler_ausgabe { eprintln!("{t}") } else { println!("{t}") };
    match geraete::liste() {
        Ok((ein, aus)) => {
            zeile("Eingänge, die Windows kennt:".into());
            ein.iter().for_each(|n| zeile(format!("  {n}")));
            zeile("Ausgänge, die Windows kennt:".into());
            aus.iter().for_each(|n| zeile(format!("  {n}")));
        }
        Err(e) => fehler(&e),
    }
}

fn fehler(text: &str) -> ! {
    eprintln!("Fehler: {text}");
    std::process::exit(2);
}
