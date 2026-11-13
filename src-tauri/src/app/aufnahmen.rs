//! Aufnahmen: WAV-Dateien in `%APPDATA%\MaskCue\aufnahmen`, und ein Player
//! zum Anhören auf einem fest gewählten Gerät (Kopfhörer).
//!
//! Aufgenommen wird, was aus „Spiel“ kommt (siehe `audio/aufnahme.rs`). Eine
//! Aufnahme kann ins Soundboard: Dann spielt sie per Hotkey ins Spiel (für
//! vorbereitete RP-Sätze).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use crate::audio::mitschnitt::wav_schreiben;
use crate::audio::pipe::{INTERN_RATE, Stats};
use crate::audio::sounddatei::SoundDaten;
use crate::audio::soundboard::{Nochmal, SoundEinstellung, Soundboard, mischer};
use crate::audio::wasapi_aus::{self, WasapiAusgang};

/// `%APPDATA%\MaskCue\aufnahmen`.
pub fn standard_ordner() -> PathBuf {
    super::einstellungen::ordner().join("aufnahmen")
}

/// Längste Aufnahme. Danach stoppt sie von selbst.
pub const MAX_SEKUNDEN: f64 = 600.0;
/// Leiser als das gilt am Anfang und Ende als Stille (ca. −60 dBFS).
const STILLE: f32 = 0.001;
/// So viel Stille bleibt vor und nach dem ersten bzw. letzten Laut (Sekunden).
const RAND_S: f32 = 0.05;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct AufnahmeInfo {
    pub name: String,
    pub datei: String,
    pub sekunden: f64,
    /// Zeitpunkt der letzten Änderung (Sekunden seit 1970).
    pub zeit: u64,
}

/// Der Name wird zum Dateinamen. Erlaubt: keine Pfad- und Sonderzeichen,
/// höchstens 80 Zeichen, keine Windows-Gerätenamen.
pub fn name_pruefen(name: &str) -> Result<String, String> {
    let n: String = name.chars().filter(|c| !c.is_control()).collect::<String>().trim().trim_end_matches('.').to_string();
    if n.is_empty() {
        return Err("Bitte einen Namen eingeben.".into());
    }
    if n.chars().count() > 80 {
        return Err("Der Name ist zu lang (höchstens 80 Zeichen).".into());
    }
    if n.chars().any(|c| "\\/:*?\"<>|".contains(c)) || n.starts_with('.') {
        return Err("Der Name darf diese Zeichen nicht enthalten: \\ / : * ? \" < > |".into());
    }
    let gross = n.to_uppercase();
    let stamm = gross.split('.').next().unwrap_or("");
    let geraete = ["CON", "PRN", "AUX", "NUL"];
    if geraete.contains(&stamm) || ((stamm.starts_with("COM") || stamm.starts_with("LPT")) && stamm.len() == 4) {
        return Err("Diesen Namen erlaubt Windows nicht.".into());
    }
    Ok(n)
}

pub fn pfad(ordner: &Path, name: &str) -> Result<PathBuf, String> {
    Ok(ordner.join(format!("{}.wav", name_pruefen(name)?)))
}

/// Erster freier Name „Aufnahme 1“, „Aufnahme 2“, …
pub fn neuer_name(ordner: &Path) -> String {
    (1..).map(|i| format!("Aufnahme {i}")).find(|n| !ordner.join(format!("{n}.wav")).exists()).unwrap_or_default()
}

/// Schneidet Stille am Anfang und Ende ab (ein kurzer Rand bleibt).
pub fn stille_schneiden(daten: &[f32]) -> &[f32] {
    let rand = (RAND_S * INTERN_RATE as f32) as usize;
    let Some(erster) = daten.iter().position(|s| s.abs() > STILLE) else { return &[] };
    let letzter = daten.iter().rposition(|s| s.abs() > STILLE).unwrap_or(erster);
    &daten[erster.saturating_sub(rand)..(letzter + 1 + rand).min(daten.len())]
}

/// Speichert eine Aufnahme (Stille am Rand abgeschnitten). Gibt den Pfad zurück.
pub fn speichern(ordner: &Path, name: &str, daten: &[f32]) -> Result<PathBuf, String> {
    let teil = stille_schneiden(daten);
    if teil.is_empty() {
        return Err("Die Aufnahme ist still. Nichts gespeichert.".into());
    }
    std::fs::create_dir_all(ordner).map_err(|e| e.to_string())?;
    let p = pfad(ordner, name)?;
    wav_schreiben(&p, teil).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(p)
}

/// Alle Aufnahmen, die neueste zuerst.
pub fn liste(ordner: &Path) -> Vec<AufnahmeInfo> {
    let Ok(lesen) = std::fs::read_dir(ordner) else { return Vec::new() };
    let mut v: Vec<AufnahmeInfo> = lesen
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()).map(str::to_lowercase).as_deref() != Some("wav") {
                return None;
            }
            let meta = e.metadata().ok()?;
            let name = p.file_stem()?.to_string_lossy().into_owned();
            // Eigene Dateien: 44 Bytes Kopf, dann 32-Bit-Float mono 48 kHz.
            let sekunden = meta.len().saturating_sub(44) as f64 / 4.0 / INTERN_RATE as f64;
            let zeit = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
            Some(AufnahmeInfo { name, datei: p.to_string_lossy().into_owned(), sekunden, zeit })
        })
        .collect();
    v.sort_by(|a, b| b.zeit.cmp(&a.zeit).then(a.name.cmp(&b.name)));
    v
}

/// Benennt um. Gibt (alter Pfad, neuer Pfad) zurück.
pub fn umbenennen(ordner: &Path, alt: &str, neu: &str) -> Result<(PathBuf, PathBuf), String> {
    let a = pfad(ordner, alt)?;
    let n = pfad(ordner, neu)?;
    if !a.is_file() {
        return Err(format!("Aufnahme „{alt}“ gibt es nicht."));
    }
    // Nur Groß/klein geändert: Windows sieht das als dieselbe Datei.
    if n.exists() && !alt.eq_ignore_ascii_case(neu) {
        return Err(format!("„{neu}“ gibt es schon."));
    }
    std::fs::rename(&a, &n).map_err(|e| e.to_string())?;
    Ok((a, n))
}

pub fn loeschen(ordner: &Path, name: &str) -> Result<PathBuf, String> {
    let p = pfad(ordner, name)?;
    std::fs::remove_file(&p).map_err(|e| format!("{name}: {e}"))?;
    Ok(p)
}

/// Spielt Aufnahmen auf einem fest gewählten Gerät ab (nicht ins Spiel).
pub struct Abhoeren {
    _strom: WasapiAusgang,
    sb: Soundboard,
    id: Option<u32>,
    pub geraet: String,
}

impl Abhoeren {
    pub fn start(geraet: &str) -> Result<Abhoeren, String> {
        let (mut m, st) = mischer();
        let strom = wasapi_aus::start(geraet, None, Arc::new(Stats::new()), move |data, kanaele, _| {
            data.fill(0.0);
            m.mische(data, kanaele);
        })?;
        Ok(Abhoeren { _strom: strom, sb: Soundboard::new(vec![st]), id: None, geraet: geraet.to_string() })
    }

    pub fn spielen(&mut self, daten: Arc<SoundDaten>) {
        self.stopp();
        let id = self.sb.hinzufuegen_geteilt(daten, SoundEinstellung { nochmal: Nochmal::NeuStarten, ..Default::default() });
        self.sb.ausloesen(id);
        self.id = Some(id);
    }

    pub fn stopp(&mut self) {
        if let Some(id) = self.id.take() {
            self.sb.entfernen(id);
        }
        self.sb.aufraeumen();
    }

    pub fn laeuft(&mut self) -> bool {
        self.sb.aufraeumen();
        !self.sb.laufende().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ordner(n: &str) -> PathBuf {
        let o = std::env::temp_dir().join(format!("maskcue-aufnahmen-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&o);
        o
    }

    #[test]
    fn namen_ohne_pfad_tricks() {
        assert_eq!(name_pruefen("  Funkspruch 1 ").unwrap(), "Funkspruch 1");
        assert!(name_pruefen("../böse").is_err());
        assert!(name_pruefen("a\\b").is_err());
        assert!(name_pruefen("C:x").is_err());
        assert!(name_pruefen("con").is_err());
        assert!(name_pruefen("COM1.txt").is_err());
        assert!(name_pruefen("").is_err());
        assert!(name_pruefen(".versteckt").is_err());
        assert_eq!(name_pruefen("Ende...").unwrap(), "Ende");
    }

    #[test]
    fn stille_am_rand_wird_geschnitten() {
        let mut d = vec![0.0; 48_000];
        d.extend(vec![0.5; 4800]);
        d.extend(vec![0.0; 48_000]);
        let t = stille_schneiden(&d);
        assert_eq!(t.len(), 4800 + 2 * 2400);
        assert!(stille_schneiden(&[0.0; 100]).is_empty());
    }

    #[test]
    fn speichern_liste_umbenennen_loeschen() {
        let o = ordner("a");
        assert_eq!(neuer_name(&o), "Aufnahme 1");
        let mut d = vec![0.0; 4800];
        d.extend(vec![0.3; 48_000]);
        let p = speichern(&o, "Aufnahme 1", &d).unwrap();
        assert!(p.is_file());
        assert_eq!(neuer_name(&o), "Aufnahme 2");
        let l = liste(&o);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].name, "Aufnahme 1");
        assert!((l[0].sekunden - 1.05).abs() < 0.01, "{}", l[0].sekunden);
        assert!(speichern(&o, "Leer", &[0.0; 4800]).is_err());
        let (_, neu) = umbenennen(&o, "Aufnahme 1", "Funkspruch").unwrap();
        assert!(neu.ends_with("Funkspruch.wav") && neu.is_file());
        speichern(&o, "Zweite", &d).unwrap();
        assert!(umbenennen(&o, "Zweite", "funkspruch").is_err(), "Name schon vergeben");
        loeschen(&o, "Funkspruch").unwrap();
        assert_eq!(liste(&o).iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), vec!["Zweite"]);
        assert!(loeschen(&o, "../einstellungen").is_err());
    }
}
