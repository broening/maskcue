//! Alle Einstellungen der App. Liegen als JSON in `%APPDATA%\MaskCue\einstellungen.json`.
//!
//! Fehlt ein Feld in der Datei (ältere Version), gilt der Standard-Wert.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::audio::dynamik::{GateEinstellung, KompressorEinstellung};
use crate::audio::eq::Band;
use crate::audio::figur::{Ausschnitt, FigurDef};
use crate::audio::soundboard::{MischerEinstellung, Nochmal, SoundEinstellung};
use crate::audio::umgebung::Umgebung;
use crate::hotkeys::profil::{Aktion, Belegung, HotkeyEinstellungen, Profil};

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Einstellungen {
    pub version: u32,
    pub geraete: Geraete,
    pub mikro: Mikro,
    /// Figuren. Beim Start ist immer die normale Stimme an.
    pub figuren: Vec<FigurDef>,
    /// Bild der normalen Stimme (wie `FigurDef::bild`). Fehlt es, zeigt die
    /// Oberfläche ein neutrales Mikrofon.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normal_bild: Option<String>,
    /// Ausschnitt des eigenen Bildes der normalen Stimme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normal_ausschnitt: Option<Ausschnitt>,
    pub sounds: Vec<Sound>,
    /// Eigene Reiter der Sounds-Seite, in dieser Reihenfolge.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sound_kategorien: Vec<String>,
    pub mischer: Mischer,
    /// Stärke je Umgebung (0…1). Fehlt eine, gilt ihr Standard.
    pub umgebungen: BTreeMap<Umgebung, f32>,
    pub hotkeys: HotkeyEinstellungen,
    pub oberflaeche: Oberflaeche,
    pub aufnahme: AufnahmeWahl,
    /// KI-Stimme: Probesatz, eigene Tonhöhe, Favoriten.
    #[serde(skip_serializing_if = "KiAllgemein::ist_leer")]
    pub ki: KiAllgemein,
    /// Nur für Tests: Ausgänge geben Stille aus.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stumm: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Geraete {
    /// Mikrofon (leer = noch nicht gewählt).
    pub eingang: String,
    /// Ausgang „Spiel“: Figur-Stimme, Sounds, Musik.
    pub spiel: String,
    /// Ausgang „Chat“: bereinigte echte Stimme (leer = kein Chat-Ausgang).
    pub chat: String,
}

impl Default for Geraete {
    fn default() -> Self {
        Self { eingang: String::new(), spiel: "MaskCue".into(), chat: "MaskCue Chat".into() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DenoiseModell {
    Aus,
    /// Früher gab es noch „schnell“ (DeepFilterNet3 ll). Es brauchte mehr CPU
    /// als „normal“ und filterte kaum besser als „leicht“ (gemessen mit
    /// examples/denoise_vergleich.rs). Alte Einstellungen bekommen „leicht“:
    /// gleiche Verzögerung (10 ms).
    #[serde(alias = "schnell")]
    Leicht,
    Normal,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DenoiseWahl {
    pub modell: DenoiseModell,
    /// Wie stark Rauschen höchstens gedämpft wird (dB, 100 = ganz weg).
    pub daempfung_db: f32,
}

impl Default for DenoiseWahl {
    fn default() -> Self {
        Self { modell: DenoiseModell::Leicht, daempfung_db: 100.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GateWahl {
    pub an: bool,
    /// Empfindlichkeit: ab diesem Pegel ist das Mikrofon offen (dBFS).
    pub schwelle_db: f32,
}

impl Default for GateWahl {
    fn default() -> Self {
        Self { an: true, schwelle_db: -45.0 }
    }
}

impl GateWahl {
    pub fn einstellung(&self) -> GateEinstellung {
        GateEinstellung { schwelle_db: self.schwelle_db, ..Default::default() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KompressorWahl {
    pub an: bool,
    pub schwelle_db: f32,
    pub verhaeltnis: f32,
    /// Verstärkung danach (Boost, dB).
    pub verstaerkung_db: f32,
}

impl Default for KompressorWahl {
    fn default() -> Self {
        let k = KompressorEinstellung::default();
        Self { an: true, schwelle_db: k.schwelle_db, verhaeltnis: k.verhaeltnis, verstaerkung_db: k.aufholen_db }
    }
}

impl KompressorWahl {
    pub fn einstellung(&self) -> KompressorEinstellung {
        KompressorEinstellung { schwelle_db: self.schwelle_db, verhaeltnis: self.verhaeltnis.max(1.0), aufholen_db: self.verstaerkung_db, ..Default::default() }
    }
}

/// Grenzen der Klangregler (Bass, Höhen).
pub const BASS_HZ: f32 = 200.0;
pub const HOEHEN_HZ: f32 = 4000.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mikro {
    /// Hochpass gegen Brummen (Hz, 0 = aus).
    pub hochpass_hz: f32,
    pub denoise_spiel: DenoiseWahl,
    pub denoise_chat: DenoiseWahl,
    pub gate_spiel: GateWahl,
    /// Im Chat aus: Nach „Normal“ schnitt es leise Laute ab (2026-10-14).
    pub gate_chat: GateWahl,
    pub kompressor: KompressorWahl,
    pub bass_db: f32,
    pub hoehen_db: f32,
}

impl Default for Mikro {
    fn default() -> Self {
        Self {
            hochpass_hz: 80.0,
            denoise_spiel: DenoiseWahl::default(),
            denoise_chat: DenoiseWahl { modell: DenoiseModell::Normal, ..Default::default() },
            gate_spiel: GateWahl::default(),
            gate_chat: GateWahl { an: false, ..Default::default() },
            kompressor: KompressorWahl::default(),
            bass_db: 0.0,
            hoehen_db: 0.0,
        }
    }
}

impl Mikro {
    /// Hochpass als EQ-Bänder (leer = aus).
    pub fn hochpass(&self) -> Vec<Band> {
        if self.hochpass_hz > 0.0 { vec![Band::Hochpass { hz: self.hochpass_hz }] } else { Vec::new() }
    }
    /// Bass und Höhen. Immer zwei Bänder (0 dB = neutral), damit der
    /// Filter-Zustand beim Verstellen bleibt.
    pub fn klang(&self) -> Vec<Band> {
        vec![Band::Tiefen { hz: BASS_HZ, db: self.bass_db }, Band::Hoehen { hz: HOEHEN_HZ, db: self.hoehen_db }]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NochmalWahl {
    Stopp,
    Neu,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sound {
    /// Eindeutiger Name, Hotkeys verweisen darauf.
    pub name: String,
    pub datei: PathBuf,
    /// 1.0 = unverändert.
    pub lautstaerke: f32,
    pub schleife: bool,
    /// Musik: wird beim Sprechen leiser.
    pub musik: bool,
    pub nochmal: NochmalWahl,
    pub spiel: bool,
    pub chat: bool,
    /// Reiter auf der Sounds-Seite (leer = nur unter „Alle“).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kategorie: String,
    /// Eigenes Bild der Kachel (wie `FigurDef::bild`). Fehlt es, bekommt die
    /// Kachel eine Farbe aus ihrem Namen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bild: Option<String>,
    /// Ausschnitt des eigenen Bildes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bild_ausschnitt: Option<Ausschnitt>,
}

impl Default for Sound {
    fn default() -> Self {
        Self {
            name: String::new(),
            datei: PathBuf::new(),
            lautstaerke: 1.0,
            schleife: false,
            musik: false,
            nochmal: NochmalWahl::Stopp,
            spiel: true,
            chat: false,
            kategorie: String::new(),
            bild: None,
            bild_ausschnitt: None,
        }
    }
}

impl Sound {
    pub fn einstellung(&self) -> SoundEinstellung {
        SoundEinstellung {
            lautstaerke: self.lautstaerke.clamp(0.0, 4.0),
            schleife: self.schleife,
            musik: self.musik,
            nochmal: match self.nochmal {
                NochmalWahl::Stopp => Nochmal::Stopp,
                NochmalWahl::Neu => Nochmal::NeuStarten,
            },
            ziele: [self.spiel, self.chat, false, false],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mischer {
    /// Musik so viel leiser beim Sprechen (dB, negativ).
    pub ducking_db: f32,
}

impl Default for Mischer {
    fn default() -> Self {
        Self { ducking_db: MischerEinstellung::default().ducking_db }
    }
}

impl Mischer {
    pub fn einstellung(&self) -> MischerEinstellung {
        MischerEinstellung { ducking_db: self.ducking_db.min(0.0), ..Default::default() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Oberflaeche {
    /// "de" oder "en".
    pub sprache: String,
    /// Klick-Töne der Oberfläche. Standard aus (PLAN.md).
    pub ui_toene: bool,
    /// Sound-Ausgang (Kopfhörer): Klick-Töne und Anhören der Aufnahmen. Nie
    /// das Windows-Standardgerät, das kann ein Stream-Kanal sein.
    pub ui_toene_geraet: String,
    /// Overlay über dem Spiel (Figur, Sounds, Musik). Standard aus.
    pub overlay: bool,
    /// Ecke des Overlays: "oben-rechts", "oben-links", "unten-rechts", "unten-links".
    pub overlay_ecke: String,
    /// Updates: "auto" (beim Start laden und installieren), "hinweis" (nur
    /// melden) oder "aus" (nicht nachsehen).
    pub updates: String,
}

impl Default for Oberflaeche {
    fn default() -> Self {
        Self { sprache: "de".into(), ui_toene: false, ui_toene_geraet: String::new(), overlay: false, overlay_ecke: "oben-rechts".into(), updates: "auto".into() }
    }
}

/// Aufnahme-Bereich.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AufnahmeWahl {
    /// Alt: eigenes Gerät zum Anhören. Wird beim Laden zum Sound-Ausgang
    /// (`Oberflaeche::ui_toene_geraet`) und ist danach leer.
    pub abhoeren_geraet: String,
}

/// KI-Stimme, für alle Figuren gleich.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KiAllgemein {
    /// Aufnahme (Name im Ordner Aufnahmen) für Hörproben: ein Satz mit der
    /// normalen Stimme.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub probesatz: String,
    /// Eigene mittlere Tonhöhe (Hz), gemessen am Probesatz. Für die
    /// automatische Tonhöhe der KI-Stimme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eigene_hz: Option<f32>,
    /// Lieblings-Stimmen je Modell (Ordner-Name → Nummern).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub favoriten: BTreeMap<String, Vec<usize>>,
    /// Datum, an dem der Spieler den Regeln für KI-Stimmen zugestimmt hat
    /// (keine echten Personen nachmachen, niemanden täuschen). Erst dann lädt
    /// MaskCue Stimmen herunter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regeln_ok: Option<String>,
}

impl KiAllgemein {
    fn ist_leer(&self) -> bool {
        *self == Self::default()
    }
}

fn figur(name: &str, halbtoene: f32, formant: f32, avatar: &str) -> FigurDef {
    FigurDef { name: name.into(), halbtoene, formant, bild: Some(format!("avatar:{avatar}")), bild_ausschnitt: None, umgebung: None, umgebung_staerke: None, verfahren: Default::default(), ki: None, ki_an: false, hell: 0.0 }
}

impl Default for Einstellungen {
    fn default() -> Self {
        let taste = |t: &str| t.parse().expect("fester Hotkey");
        Self {
            version: VERSION,
            geraete: Geraete::default(),
            mikro: Mikro::default(),
            normal_bild: None,
            normal_ausschnitt: None,
            // Start-Figuren einer neuen Installation (Dan, 01.11.2026).
            figuren: vec![
                figur("Dämon", -7.0, -4.0, "daemon"),
                figur("Goblin", 5.0, 4.0, "kobold"),
                FigurDef { umgebung: Some(Umgebung::Roboter), ..figur("Cyborg", -2.0, 0.0, "roboter") },
            ],
            sounds: Vec::new(),
            sound_kategorien: Vec::new(),
            mischer: Mischer::default(),
            umgebungen: BTreeMap::new(),
            hotkeys: HotkeyEinstellungen {
                ohne_spiel: None,
                global: vec![Belegung { taste: taste("Pause"), aktion: Aktion::AllesStopp }],
                profile: vec![
                    Profil { name: "FiveM".into(), programme: vec!["FiveM*.exe".into()], belegungen: Vec::new() },
                    Profil { name: "RedM".into(), programme: vec!["RedM*.exe".into()], belegungen: Vec::new() },
                ],
            },
            oberflaeche: Oberflaeche::default(),
            aufnahme: AufnahmeWahl::default(),
            ki: KiAllgemein::default(),
            stumm: false,
        }
    }
}

impl Einstellungen {
    /// Stärke einer Umgebung (0…1).
    pub fn umgebung_staerke(&self, u: Umgebung) -> f32 {
        self.umgebungen.get(&u).copied().unwrap_or_else(|| u.staerke_standard()).clamp(0.0, 1.0)
    }
}

/// `%APPDATA%\MaskCue`. Ein alter Ordner `%APPDATA%\MaskCue` zieht beim ersten
/// Aufruf um (siehe `datenordner`). Der Ordner gilt dann bis zum Ende des Laufs.
/// In Tests wird nichts umgezogen.
pub fn ordner() -> PathBuf {
    static ORDNER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    ORDNER
        .get_or_init(|| {
            let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
            if cfg!(test) {
                return appdata.join(super::datenordner::NEU);
            }
            let w = super::datenordner::waehlen(&appdata);
            if let Some(m) = &w.meldung {
                eprintln!("{m}");
            }
            w.ordner
        })
        .clone()
}

pub fn datei() -> PathBuf {
    ordner().join("einstellungen.json")
}

/// Eigene Bilder der Figuren.
pub fn bilder_ordner() -> PathBuf {
    ordner().join("bilder")
}

/// Start-Sounds einer neuen Installation (Dan, 01.11.2026): Datei, Name.
/// Liegen im Installer unter `sounds/`, Herkunft und Lizenz in
/// `src-tauri/sounds/HERKUNFT.txt`.
const START_SOUNDS: &[(&str, &str)] = &[
    ("applaus.ogg", "Applaus"),
    ("jubel.ogg", "Jubel"),
    ("lachen.ogg", "Lachen"),
    ("staunen.ogg", "Staunen"),
    ("trommelwirbel.ogg", "Trommelwirbel"),
    ("gong.ogg", "Gong"),
    ("buzzer.ogg", "Falsch"),
    ("klopfen.ogg", "Klopfen"),
    ("kasse.ogg", "Kasse"),
    ("explosion.ogg", "Explosion"),
];
const START_KATEGORIE: &str = "Sound-Effects";

/// Neue Installation (noch keine Einstellungen unter `pfad`): Start-Sounds aus
/// `ressourcen` nach `sounds_ziel` kopieren, eintragen und speichern. Bestehende
/// Einstellungen bleiben unberührt. Gibt zurück, ob neu eingerichtet wurde.
pub fn neu_einrichten(pfad: &Path, ressourcen: &Path, sounds_ziel: &Path) -> Result<bool, String> {
    if pfad.exists() {
        return Ok(false);
    }
    let mut e = Einstellungen::default();
    for &(datei, name) in START_SOUNDS {
        let quelle = ressourcen.join(datei);
        if !quelle.is_file() {
            continue;
        }
        std::fs::create_dir_all(sounds_ziel).map_err(|f| format!("{}: {f}", sounds_ziel.display()))?;
        let ziel = sounds_ziel.join(datei);
        std::fs::copy(&quelle, &ziel).map_err(|f| format!("{}: {f}", quelle.display()))?;
        e.sounds.push(Sound { name: name.into(), datei: ziel, kategorie: START_KATEGORIE.into(), ..Default::default() });
    }
    if !e.sounds.is_empty() {
        e.sound_kategorien = vec![START_KATEGORIE.into()];
    }
    if let Some(o) = pfad.parent() {
        std::fs::create_dir_all(o).map_err(|f| format!("{}: {f}", o.display()))?;
    }
    e.speichern(pfad)?;
    Ok(true)
}

/// Größtes erlaubtes eigenes Bild.
pub const BILD_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// Bild-Art aus den ersten Bytes (nicht aus der Endung): PNG, JPEG, WebP, GIF.
pub fn bild_art(kopf: &[u8]) -> Option<&'static str> {
    match kopf {
        [0x89, b'P', b'N', b'G', ..] => Some("png"),
        [0xFF, 0xD8, 0xFF, ..] => Some("jpg"),
        [b'G', b'I', b'F', b'8', ..] => Some("gif"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("webp"),
        _ => None,
    }
}

/// Kopiert ein eigenes Bild in den Bilder-Ordner. Gibt "datei:<name>" zurück.
/// Prüft Größe und Art am Inhalt. Der Name entsteht aus dem Inhalt (gleiches
/// Bild = gleiche Datei), nie aus dem Pfad des Nutzers.
pub fn bild_uebernehmen(quelle: &Path, ziel_ordner: &Path) -> Result<String, String> {
    let groesse = std::fs::metadata(quelle).map_err(|e| e.to_string())?.len();
    if groesse > BILD_MAX_BYTES {
        return Err(format!("Bild ist zu groß ({} MB, höchstens 8 MB).", groesse / 1024 / 1024));
    }
    let daten = std::fs::read(quelle).map_err(|e| e.to_string())?;
    let art = bild_art(&daten).ok_or("Das ist kein Bild (PNG, JPG, WebP oder GIF).")?;
    // FNV-1a über den Inhalt: kurz, stabil, ohne zusätzliche Bibliothek.
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in &daten {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let name = format!("{hash:016x}.{art}");
    std::fs::create_dir_all(ziel_ordner).map_err(|e| e.to_string())?;
    std::fs::write(ziel_ordner.join(&name), &daten).map_err(|e| e.to_string())?;
    Ok(format!("datei:{name}"))
}

/// Umgebungen, die es nicht mehr gibt (Dan, 21.10.2026: Geistform und Zombie raus).
const ALTE_UMGEBUNGEN: [&str; 2] = ["Geist", "Zombie"];

/// Wandelt Werte aus alten Dateien um oder entfernt sie, wenn es sie nicht
/// mehr gibt. Sonst wäre die
/// ganze Datei unlesbar. Gilt für die Einstellungen und für Sicherungen.
pub fn alte_werte_entfernen(v: &mut serde_json::Value) {
    let alt = |x: &serde_json::Value| x.as_str().is_some_and(|u| ALTE_UMGEBUNGEN.contains(&u));
    if let Some(figuren) = v.get_mut("figuren").and_then(|f| f.as_array_mut()) {
        for f in figuren.iter_mut().filter_map(|f| f.as_object_mut()) {
            if f.get("umgebung").is_some_and(alt) {
                f.remove("umgebung");
                f.remove("umgebung_staerke");
            }
            // Früher war die KI-Stimme ein eigenes Verfahren. Jetzt läuft sie
            // vor dem Verfahren; „ki“ heißt KI an und kein weiteres Verfahren.
            if f.get("verfahren").and_then(|v| v.as_str()) == Some("ki") {
                f.insert("verfahren".into(), "ohne".into());
                f.insert("ki_an".into(), true.into());
            }
        }
    }
    // Umbenennung GlottiX -> MaskCue (01.11.2026): Wave-Link-Kanäle heißen jetzt
    // MaskCue. Gespeicherte Ausgangs-Namen mit altem Anfang folgen.
    if let Some(g) = v.get_mut("geraete").and_then(|g| g.as_object_mut()) {
        for feld in ["spiel", "chat"] {
            if let Some(serde_json::Value::String(name)) = g.get_mut(feld)
                && name.len() >= 7
                && name.is_char_boundary(7)
                && name[..7].eq_ignore_ascii_case("glottix")
            {
                name.replace_range(..7, "MaskCue");
            }
        }
    }
    if let Some(m) = v.get_mut("umgebungen").and_then(|m| m.as_object_mut()) {
        m.retain(|k, _| !ALTE_UMGEBUNGEN.contains(&k.as_str()));
    }
    let belegung_alt = |b: &serde_json::Value| b.get("aktion").and_then(|a| a.get("Umgebung")).is_some_and(alt);
    if let Some(h) = v.get_mut("hotkeys") {
        if let Some(g) = h.get_mut("global").and_then(|g| g.as_array_mut()) {
            g.retain(|b| !belegung_alt(b));
        }
        if let Some(p) = h.get_mut("profile").and_then(|p| p.as_array_mut()) {
            for b in p.iter_mut().filter_map(|p| p.get_mut("belegungen")).filter_map(|b| b.as_array_mut()) {
                b.retain(|x| !belegung_alt(x));
            }
        }
    }
}

impl Einstellungen {
    /// Lädt die Datei. Fehlt sie, gelten die Standard-Werte. Ist sie kaputt,
    /// wird sie als `.kaputt` beiseitegelegt und es gelten die Standard-Werte.
    /// Der zweite Wert ist eine Meldung für die Anzeige.
    pub fn laden(pfad: &Path) -> (Einstellungen, Option<String>) {
        let text = match std::fs::read_to_string(pfad) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Self::default(), None),
            Err(e) => return (Self::default(), Some(format!("Einstellungen nicht lesbar: {e}"))),
        };
        let gelesen = serde_json::from_str::<serde_json::Value>(&text).and_then(|mut v| {
            alte_werte_entfernen(&mut v);
            serde_json::from_value::<Einstellungen>(v)
        });
        match gelesen {
            Ok(mut e) => {
                e.aufraeumen();
                e.sanieren();
                (e, None)
            }
            Err(f) => {
                let kaputt = pfad.with_extension("json.kaputt");
                let _ = std::fs::rename(pfad, &kaputt);
                (Self::default(), Some(format!("Einstellungen kaputt ({f}). Gesichert als {}. Standard-Werte geladen.", kaputt.display())))
            }
        }
    }

    /// Speichert sicher: erst in eine neue Datei, dann umbenennen. So bleibt
    /// bei einem Absturz die alte Datei ganz.
    pub fn speichern(&self, pfad: &Path) -> Result<(), String> {
        if let Some(o) = pfad.parent() {
            std::fs::create_dir_all(o).map_err(|e| format!("{}: {e}", o.display()))?;
        }
        let neu = pfad.with_extension("json.neu");
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&neu, text).map_err(|e| format!("{}: {e}", neu.display()))?;
        std::fs::rename(&neu, pfad).map_err(|e| format!("{}: {e}", pfad.display()))
    }

    /// Doppelte Sound-Namen eindeutig machen ("Wind" → "Wind (2)").
    pub fn aufraeumen(&mut self) {
        self.version = VERSION;
        // Ein Sound-Ausgang für Klick-Töne und Anhören: altes Anhör-Gerät übernehmen.
        let anhoeren = std::mem::take(&mut self.aufnahme.abhoeren_geraet);
        if !anhoeren.trim().is_empty() {
            self.oberflaeche.ui_toene_geraet = anhoeren.trim().to_string();
        }
        let mut gesehen: Vec<String> = Vec::new();
        for s in &mut self.sounds {
            let basis = s.name.clone();
            let mut n = 2;
            while gesehen.iter().any(|g| g.eq_ignore_ascii_case(&s.name)) || s.name.is_empty() {
                s.name = format!("{} ({n})", if basis.is_empty() { "Sound" } else { &basis });
                n += 1;
            }
            gesehen.push(s.name.clone());
        }
        // Kategorien: ohne Leerzeichen am Rand, keine doppelten (Groß/klein egal).
        let mut kategorien: Vec<String> = Vec::new();
        for k in self.sound_kategorien.iter().map(|k| k.trim().to_string()) {
            if !k.is_empty() && !kategorien.iter().any(|g| g.eq_ignore_ascii_case(&k)) {
                kategorien.push(k);
            }
        }
        // Ein Sound zeigt auf eine Kategorie, die fehlt: anlegen (gleiche Schreibweise wie vorhanden).
        for s in &mut self.sounds {
            s.kategorie = s.kategorie.trim().to_string();
            if s.kategorie.is_empty() {
                continue;
            }
            match kategorien.iter().find(|g| g.eq_ignore_ascii_case(&s.kategorie)) {
                Some(g) => s.kategorie = g.clone(),
                None => kategorien.push(s.kategorie.clone()),
            }
        }
        self.sound_kategorien = kategorien;
    }

    /// Fügt Sound-Dateien hinzu. Name = Dateiname ohne Endung. Gibt die neuen Namen zurück.
    pub fn sounds_hinzufuegen(&mut self, dateien: &[PathBuf]) -> Vec<String> {
        let vorher = self.sounds.len();
        for d in dateien {
            let name = d.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            self.sounds.push(Sound { name, datei: d.clone(), ..Default::default() });
        }
        self.aufraeumen();
        self.sounds[vorher..].iter().map(|s| s.name.clone()).collect()
    }

    /// Sound umbenennen. Hotkeys mit dem alten Namen zeigen danach auf den neuen.
    pub fn sound_umbenennen(&mut self, alt: &str, neu: &str) -> Result<(), String> {
        let neu = neu.trim();
        if neu.is_empty() {
            return Err("Name ist leer".into());
        }
        if self.sounds.iter().any(|s| s.name.eq_ignore_ascii_case(neu) && !s.name.eq_ignore_ascii_case(alt)) {
            return Err(format!("„{neu}“ gibt es schon"));
        }
        let s = self.sounds.iter_mut().find(|s| s.name == alt).ok_or_else(|| format!("Sound „{alt}“ nicht gefunden"))?;
        s.name = neu.to_string();
        let alle = self.hotkeys.global.iter_mut().chain(self.hotkeys.profile.iter_mut().flat_map(|p| p.belegungen.iter_mut()));
        for b in alle {
            if b.aktion == Aktion::Sound(alt.to_string()) {
                b.aktion = Aktion::Sound(neu.to_string());
            }
        }
        Ok(())
    }
}

/// Zahl in den Bereich bringen. NaN und unendlich werden zum Standard-Wert.
fn bereich(v: f32, min: f32, max: f32, standard: f32) -> f32 {
    if v.is_finite() { v.clamp(min, max) } else { standard }
}

impl Einstellungen {
    /// Alle Zahlen in den Bereich der Regler bringen, KI-Mischung begrenzen.
    /// Gilt für Datei, Sicherung und Oberfläche. Unendlich (z. B. `1e39` aus
    /// einer fremden Sicherung) würde als `null` gespeichert, und die Datei
    /// wäre beim nächsten Start kaputt. Grenzen wie in `paket.rs`.
    pub fn sanieren(&mut self) {
        let std_m = Mikro::default();
        let m = &mut self.mikro;
        m.hochpass_hz = bereich(m.hochpass_hz, 0.0, 200.0, std_m.hochpass_hz);
        for d in [&mut m.denoise_spiel, &mut m.denoise_chat] {
            d.daempfung_db = bereich(d.daempfung_db, 0.0, 100.0, 100.0);
        }
        for g in [&mut m.gate_spiel, &mut m.gate_chat] {
            g.schwelle_db = bereich(g.schwelle_db, -100.0, 0.0, -45.0);
        }
        let k = &mut m.kompressor;
        k.schwelle_db = bereich(k.schwelle_db, -60.0, 0.0, std_m.kompressor.schwelle_db);
        k.verhaeltnis = bereich(k.verhaeltnis, 1.0, 20.0, std_m.kompressor.verhaeltnis);
        k.verstaerkung_db = bereich(k.verstaerkung_db, 0.0, 18.0, std_m.kompressor.verstaerkung_db);
        m.bass_db = bereich(m.bass_db, -12.0, 12.0, 0.0);
        m.hoehen_db = bereich(m.hoehen_db, -12.0, 12.0, 0.0);
        self.mischer.ducking_db = bereich(self.mischer.ducking_db, -30.0, 0.0, Mischer::default().ducking_db);
        self.umgebungen.retain(|_, s| s.is_finite());
        for s in self.umgebungen.values_mut() {
            *s = s.clamp(0.0, 1.0);
        }
        for f in &mut self.figuren {
            f.halbtoene = bereich(f.halbtoene, crate::audio::wsola::HALBTOENE_MIN, crate::audio::wsola::HALBTOENE_MAX, 0.0);
            f.formant = bereich(f.formant, -4.0, 12.0, 0.0);
            f.hell = bereich(f.hell, -6.0, 6.0, 0.0);
            f.umgebung_staerke = f.umgebung_staerke.filter(|s| s.is_finite()).map(|s| s.clamp(0.0, 1.0));
            f.bild_ausschnitt = f.bild_ausschnitt.and_then(Ausschnitt::sauber);
            f.ki = f.ki.take().and_then(crate::audio::ki::KiWahl::sauber);
        }
        self.normal_ausschnitt = self.normal_ausschnitt.and_then(Ausschnitt::sauber);
        for s in &mut self.sounds {
            s.lautstaerke = bereich(s.lautstaerke, 0.0, 2.0, 1.0);
            s.bild_ausschnitt = s.bild_ausschnitt.and_then(Ausschnitt::sauber);
        }
        self.ki.eigene_hz = self.ki.eigene_hz.filter(|h| h.is_finite()).map(|h| h.clamp(50.0, 1000.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let o = std::env::temp_dir().join(format!("maskcue_test_einst_{name}"));
        let _ = std::fs::remove_dir_all(&o);
        std::fs::create_dir_all(&o).unwrap();
        o.join("einstellungen.json")
    }

    #[test]
    fn bild_pruefen_und_uebernehmen() {
        let o = std::env::temp_dir().join("maskcue_test_bilder");
        let _ = std::fs::remove_dir_all(&o);
        std::fs::create_dir_all(&o).unwrap();
        let png = o.join("ich.png");
        std::fs::write(&png, [0x89, b'P', b'N', b'G', 1, 2, 3]).unwrap();
        let ziel = o.join("bilder");
        let r = bild_uebernehmen(&png, &ziel).unwrap();
        assert!(r.starts_with("datei:") && r.ends_with(".png"), "{r}");
        assert!(ziel.join(r.trim_start_matches("datei:")).exists());
        // Gleiches Bild nochmal: gleicher Name.
        assert_eq!(bild_uebernehmen(&png, &ziel).unwrap(), r);
        // Endung lügt: Text mit .png wird abgelehnt.
        let falsch = o.join("text.png");
        std::fs::write(&falsch, b"kein bild").unwrap();
        assert!(bild_uebernehmen(&falsch, &ziel).is_err());
        assert_eq!(bild_art(b"RIFF\0\0\0\0WEBPxx"), Some("webp"));
    }

    #[test]
    fn speichern_und_laden() {
        let p = temp("rund");
        let mut e = Einstellungen::default();
        e.geraete.eingang = "Tischmikrofon".into();
        e.sounds_hinzufuegen(&[PathBuf::from("C:/x/Wind.wav")]);
        e.speichern(&p).unwrap();
        let (g, meldung) = Einstellungen::laden(&p);
        assert_eq!(meldung, None);
        assert_eq!(g, e);
        assert!(!p.with_extension("json.neu").exists());
    }

    #[test]
    fn fehlende_datei_gibt_standard() {
        let (e, m) = Einstellungen::laden(&temp("fehlt"));
        assert_eq!(e, Einstellungen::default());
        assert_eq!(m, None);
        // Standard: Chat ohne Gate, UI-Töne aus, Notbremse auf Pause.
        assert!(!e.mikro.gate_chat.an);
        assert!(!e.oberflaeche.ui_toene);
        assert_eq!(e.hotkeys.global[0].aktion, Aktion::AllesStopp);
        assert!(e.hotkeys.pruefen().is_empty());
    }

    #[test]
    fn alte_datei_mit_wenigen_feldern() {
        let p = temp("alt");
        std::fs::write(&p, r#"{ "geraete": { "eingang": "Mikro" }, "mikro": { "bass_db": 3 } }"#).unwrap();
        let (e, m) = Einstellungen::laden(&p);
        assert_eq!(m, None);
        assert_eq!(e.geraete.eingang, "Mikro");
        assert_eq!(e.geraete.spiel, "MaskCue");
        assert_eq!(e.mikro.bass_db, 3.0);
        assert_eq!(e.mikro.hochpass_hz, 80.0);
    }

    #[test]
    fn kaputte_datei_wird_gesichert() {
        let p = temp("kaputt");
        std::fs::write(&p, "{ das ist kein json").unwrap();
        let (e, m) = Einstellungen::laden(&p);
        assert_eq!(e, Einstellungen::default());
        assert!(m.unwrap().contains("kaputt"));
        assert!(p.with_extension("json.kaputt").exists());
    }

    /// Zahlen außerhalb der Regler (z. B. von Hand oder aus einer alten
    /// Sicherung) werden beim Laden in den Bereich gebracht, nicht übernommen.
    #[test]
    fn laden_bringt_werte_in_den_bereich() {
        let p = temp("bereich");
        std::fs::write(&p, r#"{"geraete":{"eingang":"Mikro"},"mikro":{"kompressor":{"verstaerkung_db":1000}},"figuren":[{"name":"X","halbtoene":0,"formant":500}]}"#).unwrap();
        let (e, m) = Einstellungen::laden(&p);
        assert_eq!(m, None);
        assert_eq!(e.geraete.eingang, "Mikro");
        assert!(e.mikro.kompressor.verstaerkung_db <= 18.0, "verstaerkung {}", e.mikro.kompressor.verstaerkung_db);
        assert!(e.figuren[0].formant <= 12.0, "formant {}", e.figuren[0].formant);
    }

    #[test]
    fn altes_schnell_wird_leicht() {
        let e: Einstellungen = serde_json::from_str(r#"{"mikro":{"denoise_spiel":{"modell":"schnell","daempfung_db":80}}}"#).expect("JSON");
        assert_eq!(e.mikro.denoise_spiel.modell, DenoiseModell::Leicht);
        assert_eq!(e.mikro.denoise_spiel.daempfung_db, 80.0);
        assert!(serde_json::to_string(&e).unwrap().contains(r#""modell":"leicht""#));
    }

    /// So schickt die Oberfläche die Einstellungen (aus src/api.ts, Vorschau-Daten).
    /// Bricht hier etwas, passen TypeScript und Rust nicht mehr zusammen.
    #[test]
    fn json_der_oberflaeche() {
        let j = r#"{"version":1,"geraete":{"eingang":"Tischmikrofon (2- USB Audio Device)","spiel":"MaskCue","chat":"MaskCue Chat"},
            "mikro":{"hochpass_hz":80,"denoise_spiel":{"modell":"leicht","daempfung_db":100},"denoise_chat":{"modell":"normal","daempfung_db":100},
            "gate_spiel":{"an":true,"schwelle_db":-45},"gate_chat":{"an":false,"schwelle_db":-45},
            "kompressor":{"an":true,"schwelle_db":-20,"verhaeltnis":3,"verstaerkung_db":3},"bass_db":0,"hoehen_db":0},
            "figuren":[{"name":"Opa","halbtoene":-4,"formant":-3},{"name":"Dämon","halbtoene":-7,"formant":-4,"umgebung":"Hoehle","umgebung_staerke":0.25}],
            "sounds":[{"name":"Wind","datei":"C:/x/Wind.wav","lautstaerke":1,"schleife":false,"musik":false,"nochmal":"stopp","spiel":true,"chat":false},
                      {"name":"Grim","datei":"C:/x/Grim.mp3","lautstaerke":0.5,"schleife":true,"musik":true,"nochmal":"neu","spiel":true,"chat":true}],
            "mischer":{"ducking_db":-12},
            "umgebungen":{"Funk":0.75,"Hoehle":0.5},
            "hotkeys":{"global":[{"taste":"Pause","aktion":"AllesStopp"},{"taste":"Strg+Alt+0","aktion":"ProfilAutomatisch"},{"taste":"Strg+Alt+1","aktion":{"Profil":"FiveM"}}],
              "profile":[{"name":"FiveM","programme":["FiveM*.exe"],"belegungen":[{"taste":"Num1","aktion":{"Sound":"Wind"}},
                {"taste":"Maus4","aktion":{"Figur":"Opa"}},{"taste":"Num0","aktion":"FigurAus"},
                {"taste":"Num7","aktion":{"Umgebung":"Funk"}},{"taste":"Num8","aktion":"UmgebungAus"}]}]},
            "oberflaeche":{"sprache":"de","ui_toene":false,"ui_toene_geraet":"","overlay":false,"overlay_ecke":"oben-rechts","updates":"auto"},
            "aufnahme":{"abhoeren_geraet":"Kopfhörer (Arctis 7)"}}"#;
        let e: Einstellungen = serde_json::from_str(j).expect("JSON der Oberfläche");
        assert_eq!(e.sounds[1].nochmal, NochmalWahl::Neu);
        assert_eq!(e.hotkeys.profile[0].belegungen[1].aktion, Aktion::Figur("Opa".into()));
        assert!(!e.stumm);
        assert_eq!(e.figuren[1].umgebung, Some(Umgebung::Hoehle));
        assert_eq!(e.hotkeys.profile[0].belegungen[3].aktion, Aktion::Umgebung(Umgebung::Funk));
        assert_eq!(e.umgebung_staerke(Umgebung::Funk), 0.75);
        assert_eq!(e.umgebung_staerke(Umgebung::Halle), Umgebung::Halle.staerke_standard());
        // Zurück an die Oberfläche: gleiche Form, `stumm` fehlt (nur für Tests).
        // Zahlen gleich behandeln: Rust schreibt -3.0, JavaScript -3.
        fn gleich(v: serde_json::Value) -> serde_json::Value {
            use serde_json::Value::*;
            match v {
                Number(n) => serde_json::json!(n.as_f64()),
                Array(a) => Array(a.into_iter().map(gleich).collect()),
                Object(o) => Object(o.into_iter().map(|(k, v)| (k, gleich(v))).collect()),
                x => x,
            }
        }
        let zurueck = gleich(serde_json::to_value(&e).unwrap());
        let original = gleich(serde_json::from_str(j).unwrap());
        assert_eq!(zurueck, original);
    }

    #[test]
    fn neue_installation_bekommt_start_werte() {
        let t = std::env::temp_dir().join(format!("maskcue_neu_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let pfad = t.join("appdata").join("einstellungen.json");
        let res = Path::new(env!("CARGO_MANIFEST_DIR")).join("sounds");
        assert!(neu_einrichten(&pfad, &res, &t.join("appdata").join("sounds")).unwrap());
        let (e, f) = Einstellungen::laden(&pfad);
        assert!(f.is_none());
        // Alle 10 Start-Sounds kopiert, in „Sound-Effects“, Datei da.
        assert_eq!(e.sounds.len(), 10);
        assert_eq!(e.sound_kategorien, ["Sound-Effects"]);
        assert!(e.sounds.iter().all(|s| s.kategorie == "Sound-Effects" && s.datei.is_file()));
        let namen: Vec<_> = e.figuren.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(namen, ["Dämon", "Goblin", "Cyborg"]);
        let spiele: Vec<_> = e.hotkeys.profile.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(spiele, ["FiveM", "RedM"]);
        // Zweiter Start: nichts anfassen.
        std::fs::write(&pfad, "{}").unwrap();
        assert!(!neu_einrichten(&pfad, &t.join("res"), &t.join("appdata").join("sounds")).unwrap());
        assert_eq!(std::fs::read_to_string(&pfad).unwrap(), "{}");
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn sound_kategorien_aufraeumen() {
        let mut e = Einstellungen { sound_kategorien: vec!["RP".into(), " Musik ".into(), "rp".into(), "".into()], ..Default::default() };
        e.sounds.push(Sound { name: "Funk".into(), kategorie: "musik".into(), ..Default::default() });
        e.sounds.push(Sound { name: "Lachen".into(), kategorie: "Lustig".into(), ..Default::default() });
        e.sounds.push(Sound { name: "Wind".into(), ..Default::default() });
        e.aufraeumen();
        assert_eq!(e.sound_kategorien, vec!["RP", "Musik", "Lustig"]);
        assert_eq!(e.sounds[0].kategorie, "Musik");
        assert_eq!(e.sounds[2].kategorie, "");
        // Ohne Kategorie steht das Feld nicht in der Datei (ältere Versionen lesen sie weiter).
        let j = serde_json::to_string(&e.sounds[2]).unwrap();
        assert!(!j.contains("kategorie"), "{j}");
    }

    #[test]
    fn sound_namen_eindeutig_und_umbenennen() {
        let mut e = Einstellungen::default();
        let neu = e.sounds_hinzufuegen(&[PathBuf::from("a/Wind.wav"), PathBuf::from("b/wind.mp3"), PathBuf::from("c/Glocke.ogg")]);
        assert_eq!(neu, vec!["Wind", "wind (2)", "Glocke"]);
        e.hotkeys.profile[0].belegungen.push(Belegung { taste: "Num1".parse().unwrap(), aktion: Aktion::Sound("Wind".into()) });
        e.sound_umbenennen("Wind", "Sturm").unwrap();
        assert_eq!(e.hotkeys.profile[0].belegungen[0].aktion, Aktion::Sound("Sturm".into()));
        assert!(e.sound_umbenennen("Sturm", "Glocke").is_err());
        assert!(e.sound_umbenennen("Sturm", " ").is_err());
    }

    /// Früher hatte das Anhören der Aufnahmen ein eigenes Gerät. Jetzt gibt es
    /// einen Sound-Ausgang für Klick-Töne und Anhören (Dan, 21.10.2026). Das
    /// gewählte Anhör-Gerät wird zum Sound-Ausgang: Es war der Kopfhörer.
    #[test]
    fn anhoer_geraet_wird_sound_ausgang() {
        let mut e = Einstellungen::default();
        e.oberflaeche.ui_toene_geraet = "System (Elgato Virtual Audio)".into();
        e.aufnahme.abhoeren_geraet = "Kopfhörer (EDIFIER WH950NB)".into();
        e.aufraeumen();
        assert_eq!(e.oberflaeche.ui_toene_geraet, "Kopfhörer (EDIFIER WH950NB)");
        assert_eq!(e.aufnahme.abhoeren_geraet, "");
        // Ohne Anhör-Gerät bleibt der Sound-Ausgang.
        e.aufraeumen();
        assert_eq!(e.oberflaeche.ui_toene_geraet, "Kopfhörer (EDIFIER WH950NB)");
    }

    /// Geistform und Zombie gibt es nicht mehr (Dan, 21.10.2026). Alte
    /// Dateien müssen trotzdem laden: Die Umgebung fällt nur weg.
    #[test]
    fn alte_umgebungen_fallen_weg() {
        let d = std::env::temp_dir().join(format!("maskcue-alt-umg-{}.json", std::process::id()));
        std::fs::write(
            &d,
            r#"{"figuren":[{"name":"Geist","halbtoene":2,"umgebung":"Geist"},{"name":"Opa","halbtoene":-4,"umgebung":"Halle"}],
                "umgebungen":{"Zombie":0.5,"Halle":0.3},
                "hotkeys":{"global":[{"taste":"F1","aktion":{"Umgebung":"Zombie"}},{"taste":"F2","aktion":{"Umgebung":"Raumhelm"}}],"profile":[]}}"#,
        )
        .unwrap();
        let (e, meldung) = Einstellungen::laden(&d);
        let _ = std::fs::remove_file(&d);
        assert_eq!(meldung, None);
        assert_eq!(e.figuren[0].umgebung, None);
        assert_eq!(e.figuren[1].umgebung, Some(Umgebung::Halle));
        assert_eq!(e.umgebungen.len(), 1);
        assert_eq!(e.hotkeys.global.len(), 1);
        assert_eq!(e.hotkeys.global[0].aktion, Aktion::Umgebung(Umgebung::Raumhelm));
    }

    /// Wave-Link-Kanäle heißen jetzt MaskCue: alte gespeicherte Namen folgen.
    #[test]
    fn ausgangsnamen_glottix_werden_maskcue() {
        let mut v: serde_json::Value = serde_json::from_str(r#"{"geraete":{"eingang":"GlottiX Mikro","spiel":"GlottiX","chat":"glottix Chat (Elgato Virtual Audio)"}}"#).unwrap();
        alte_werte_entfernen(&mut v);
        let e: Einstellungen = serde_json::from_value(v).unwrap();
        assert_eq!(e.geraete.spiel, "MaskCue");
        assert_eq!(e.geraete.chat, "MaskCue Chat (Elgato Virtual Audio)");
        assert_eq!(e.geraete.eingang, "GlottiX Mikro", "Eingang bleibt");
    }

    /// Alte KI-Figur (Verfahren „ki“): KI an, kein weiteres Verfahren.
    #[test]
    fn altes_ki_verfahren_wird_ki_an() {
        let mut v: serde_json::Value = serde_json::from_str(r#"{"figuren":[{"name":"Ottis","halbtoene":-6,"verfahren":"ki","ki":{"modell":"thorsten","stimmen":[{"nr":0,"gewicht":1}]}},{"name":"Cyborg","halbtoene":9,"verfahren":"wsola","ki":{"modell":"thorsten"}}]}"#).unwrap();
        alte_werte_entfernen(&mut v);
        let e: Einstellungen = serde_json::from_value(v).unwrap();
        assert_eq!(e.figuren[0].verfahren, crate::audio::figur::Verfahren::Ohne);
        assert!(e.figuren[0].ki_an);
        assert_eq!(e.figuren[1].verfahren, crate::audio::figur::Verfahren::Wsola);
        assert!(!e.figuren[1].ki_an);
    }
}
