//! KI-Modelle in `%APPDATA%\MaskCue\ki-modelle`: je Ordner ein Beatrice-2-Modell.
//!
//! Neben den Dateien des Trainers kann `glottix_stimmen.json` liegen. Darin
//! stehen Merkmale je Stimme (Sprache, Geschlecht, Größe, hell, Hauch), die
//! MaskCue selbst gemessen hat. Fehlt die Datei, zeigt die Oberfläche nur
//! Nummer und Name.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::audio::ki::{KiAnteil, gewichte, modell_name_ok};

pub fn ordner() -> PathBuf {
    super::einstellungen::ordner().join("ki-modelle")
}

/// Merkmale einer Stimme aus `glottix_stimmen.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Merkmale {
    pub nr: usize,
    /// "de", "es", "en" …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprache: Option<String>,
    /// "m" oder "w".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geschlecht: Option<String>,
    /// Rang 0…1: groß/tief (1) bis klein (0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groesse: Option<f32>,
    /// Rang 0…1: dunkel (0) bis hell (1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hell: Option<f32>,
    /// Rang 0…1: klar (0) bis hauchig (1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hauch: Option<f32>,
    /// Mittlere Tonhöhe der Stimme (Hz).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hz: Option<f32>,
    /// Dialekt, von Dan beim Hören eingetragen ("bayerisch" …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dialekt: Option<String>,
    /// Verständlichkeit 0…100 (Vorlesetest mit Whisper, grob).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verstaendlich: Option<u8>,
}

/// Übliche Tonhöhe, wenn ein Modell keine eigene kennt.
fn hz_geschaetzt(m: &Merkmale) -> Option<f32> {
    m.hz.or(match m.geschlecht.as_deref() {
        Some("m") => Some(120.0),
        Some("w") => Some(210.0),
        _ => None,
    })
}

/// Mittlere Tonhöhe einer Mischung (geometrisch gewichtet).
pub fn ziel_hz(basis: &Path, modell: &str, stimmen: &[KiAnteil]) -> Option<f32> {
    if !modell_name_ok(modell) {
        return None;
    }
    let m = merkmale(&basis.join(modell));
    let (mut summe, mut gewicht) = (0.0f32, 0.0f32);
    for a in stimmen {
        let hz = m.iter().find(|x| x.nr == a.nr).and_then(hz_geschaetzt)?;
        summe += a.gewicht * hz.ln();
        gewicht += a.gewicht;
    }
    (gewicht > 0.0).then(|| (summe / gewicht).exp())
}

/// Pfad zum Tonhöhen-Schätzer (gleich in allen Beatrice-Modellen des Trainers).
pub fn tonhoehen_schaetzer(basis: &Path) -> Option<PathBuf> {
    liste(basis).into_iter().next().map(|m| basis.join(&m.ordner).join("pitch_estimator.bin"))
}

#[derive(Clone, Debug, Serialize)]
pub struct KiStimmeInfo {
    pub nr: usize,
    pub name: String,
    #[serde(flatten)]
    pub merkmale: Merkmale,
}

#[derive(Clone, Debug, Serialize)]
pub struct KiModellInfo {
    /// Ordner-Name (wird in der Figur gespeichert).
    pub ordner: String,
    pub name: String,
    pub stimmen: Vec<KiStimmeInfo>,
}

fn merkmale(o: &Path) -> Vec<Merkmale> {
    std::fs::read_to_string(crate::audio::ki::beatrice::modelldatei(o, "stimmen.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Alle Modelle im Ordner. Ordner, die kein gültiges Modell sind, fehlen.
pub fn liste(basis: &Path) -> Vec<KiModellInfo> {
    let Ok(rd) = std::fs::read_dir(basis) else { return Vec::new() };
    let mut v: Vec<KiModellInfo> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !modell_name_ok(&name) {
                return None;
            }
            let i = gewichte::info(&e.path()).ok()?;
            let m = merkmale(&e.path());
            let stimmen = i
                .stimmen
                .iter()
                .map(|s| {
                    let nr = s.nummer;
                    KiStimmeInfo { nr, name: s.name.clone(), merkmale: m.iter().find(|x| x.nr == nr).cloned().unwrap_or(Merkmale { nr, ..Default::default() }) }
                })
                .collect();
            // Angebotene und frühere Stimmen heißen wie hier, nicht wie im Modell.
            let anzeige = ANGEBOT.iter().map(|a| (a.0, a.1)).chain(FRUEHERE.iter().copied()).find(|a| a.0 == name).map_or(i.name, |a| a.1.to_string());
            Some(KiModellInfo { ordner: name, name: anzeige, stimmen })
        })
        .collect();
    v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    v
}

/// Adresse der Stimmen-Pakete (`<ordner>.zip`). Hochladen mit
/// `ki-werkzeuge/stimmen_pakete.py`.
const PAKET_URL: &str = "https://github.com/broening/maskcue/releases/download/stimmen-v2";

/// Stimmen zum Herunterladen: Ordner, Anzeigename, Größe (Bytes), SHA-256 der ZIP.
/// Seit 0.1.2 nur noch Thorsten (Dan, 01.11.2026).
/// Seit 01.11.2026 nicht mehr im Installer (Dan): Der Spieler lädt sie selbst
/// und stimmt vorher den Regeln zu.
const ANGEBOT: &[(&str, &str, u64, &str)] = &[
    ("thorsten", "Thorsten", 17_663_301, "53983278d7804bd7a67ce5dcc5772442e7e0ef8224f9a3a4a86e167ffc1505ec"),
];

/// Anzeigenamen für Stimmen, die früher angeboten wurden und bei manchen
/// Spielern noch installiert sind (Ordner, Name). Werden nicht mehr geladen.
const FRUEHERE: &[(&str, &str)] = &[("stephanie", "Stimme W1")];

/// Eine Stimme zum Herunterladen, für die Oberfläche.
#[derive(Clone, Debug, Serialize)]
pub struct KiAngebot {
    pub ordner: String,
    pub name: String,
    pub bytes: u64,
    pub installiert: bool,
}

pub fn angebot(basis: &Path) -> Vec<KiAngebot> {
    ANGEBOT
        .iter()
        .map(|&(ordner, name, bytes, _)| KiAngebot { ordner: ordner.into(), name: name.into(), bytes, installiert: gewichte::info(&basis.join(ordner)).is_ok() })
        .collect()
}

/// Stimme `ordner` laden, prüfen (Größe, SHA-256) und nach `basis/<ordner>`
/// entpacken. `fortschritt(geladen, gesamt)` nach jedem Stück.
pub fn herunterladen(ordner: &str, basis: &Path, mut fortschritt: impl FnMut(u64, u64)) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let &(_, _, bytes, sha) = ANGEBOT.iter().find(|a| a.0 == ordner).ok_or_else(|| format!("Unbekannte Stimme „{ordner}“"))?;
    // MASKCUE_KI_URL: andere Adresse zum Testen (z. B. ein Server auf dem eigenen PC).
    // GLOTTIX_KI_URL gilt noch als alter Name.
    let url = std::env::var("MASKCUE_KI_URL").or_else(|_| std::env::var("GLOTTIX_KI_URL")).unwrap_or_else(|_| PAKET_URL.into());
    let mut antwort = ureq::get(&format!("{url}/{ordner}.zip")).call().map_err(|f| format!("Download ging nicht: {f}"))?;
    let mut leser = antwort.body_mut().with_config().limit(bytes + 1).reader();
    let mut daten = Vec::with_capacity(bytes as usize);
    let mut stueck = [0u8; 64 * 1024];
    loop {
        let n = leser.read(&mut stueck).map_err(|f| format!("Download abgebrochen: {f} ({} von {bytes})", daten.len()))?;
        if n == 0 {
            break;
        }
        daten.extend_from_slice(&stueck[..n]);
        if daten.len() as u64 > bytes {
            return Err("Download ist größer als erwartet".into());
        }
        fortschritt(daten.len() as u64, bytes);
    }
    let summe: String = Sha256::digest(&daten).iter().map(|b| format!("{b:02x}")).collect();
    if daten.len() as u64 != bytes || summe != sha {
        return Err("Download ist beschädigt. Bitte noch einmal versuchen.".into());
    }
    zip_einrichten(&daten, basis, ordner)
}

/// Erlaubter Dateiname im Stimmen-Paket: flach, nur Buchstaben, Ziffern, `_ - .`.
fn paket_name_ok(n: &str) -> bool {
    !n.is_empty() && n.len() <= 80 && !n.starts_with('.') && n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// ZIP einer Stimme nach `basis/<ordner>` entpacken. Erst daneben fertig
/// schreiben, dann tauschen: Ein Abbruch lässt nie einen halben Ordner zurück.
fn zip_einrichten(daten: &[u8], basis: &Path, ordner: &str) -> Result<(), String> {
    use std::io::Read;
    if !modell_name_ok(ordner) {
        return Err(format!("Ungültiger Ordner „{ordner}“"));
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(daten)).map_err(|f| format!("Kein gültiges ZIP: {f}"))?;
    let tmp = basis.join(format!("{ordner}.neu"));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|f| format!("{}: {f}", tmp.display()))?;
    let schreiben = |zip: &mut zip::ZipArchive<_>| -> Result<(), String> {
        for i in 0..zip.len() {
            let mut e = zip.by_index(i).map_err(|f| f.to_string())?;
            let name = e.name().map_err(|f| f.to_string())?.to_string();
            if e.is_dir() || !paket_name_ok(&name) {
                return Err(format!("Unerwartete Datei im Paket: {name}"));
            }
            let mut inhalt = Vec::new();
            (&mut e).take(64 * 1024 * 1024).read_to_end(&mut inhalt).map_err(|f| f.to_string())?;
            std::fs::write(tmp.join(&name), inhalt).map_err(|f| f.to_string())?;
        }
        Ok(())
    };
    if let Err(f) = schreiben(&mut zip) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(f);
    }
    let z = basis.join(ordner);
    if z.exists() {
        std::fs::remove_dir_all(&z).map_err(|f| format!("{}: {f}", z.display()))?;
    }
    std::fs::rename(&tmp, &z).map_err(|f| format!("{}: {f}", z.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stimmen kommen seit 01.11.2026 als ZIP aus dem Netz. Das Entpacken
    /// darf nichts außerhalb des Ordners schreiben und nichts Halbes lassen.
    #[test]
    fn stimmen_paket_entpacken() {
        use std::io::Write;
        let t = std::env::temp_dir().join(format!("maskcue_ki_paket_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        let zip_aus = |eintraege: &[(&str, &str)]| {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            for (n, inhalt) in eintraege {
                w.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
                w.write_all(inhalt.as_bytes()).unwrap();
            }
            w.finish().unwrap().into_inner()
        };
        let toml = "[model]
version = \"2.0.0-rc.0\"
name = \"Thorsten\"
[voice.0]
name = \"A\"
average_pitch = 50.0
";
        zip_einrichten(&zip_aus(&[("beatrice_paraphernalia_x.toml", toml), ("glottix_version.txt", "v1")]), &t, "thorsten").unwrap();
        assert_eq!(liste(&t).len(), 1);
        assert!(angebot(&t).iter().find(|a| a.ordner == "thorsten").unwrap().installiert);
        // Nur Thorsten wird angeboten. Eine früher geladene Stimme heißt weiter
        // neutral, nicht wie im Modell.
        assert_eq!(angebot(&t).len(), 1);
        zip_einrichten(&zip_aus(&[("beatrice_paraphernalia_x.toml", &toml.replace("Thorsten", "Sprecherin")), ("glottix_version.txt", "v1")]), &t, FRUEHERE[0].0).unwrap();
        assert_eq!(liste(&t).iter().find(|m| m.ordner == FRUEHERE[0].0).unwrap().name, "Stimme W1");
        assert_eq!(liste(&t).iter().find(|m| m.ordner == "thorsten").unwrap().name, "Thorsten");
        // Pfad nach außen: abgelehnt, alter Stand bleibt, kein halber Ordner.
        assert!(zip_einrichten(&zip_aus(&[("../boese.txt", "x")]), &t, "thorsten").is_err());
        assert!(!t.join("boese.txt").exists());
        assert!(!t.join("thorsten.neu").exists());
        assert_eq!(liste(&t).len(), 2);
        assert!(zip_einrichten(b"kein zip", &t, "thorsten").is_err());
        assert!(zip_einrichten(&zip_aus(&[("a.txt", "x")]), &t, "../raus").is_err());
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn ziel_tonhoehe_einer_mischung() {
        let t = std::env::temp_dir().join(format!("maskcue_ki_hz_{}", std::process::id()));
        let m = t.join("m");
        std::fs::create_dir_all(&m).unwrap();
        std::fs::write(m.join("glottix_stimmen.json"), r#"[{"nr":0,"hz":100},{"nr":1,"hz":400},{"nr":2,"geschlecht":"w"},{"nr":3}]"#).unwrap();
        let a = |nr, gewicht| KiAnteil { nr, gewicht };
        let hz = |s: &[KiAnteil]| ziel_hz(&t, "m", s);
        assert!((hz(&[a(0, 1.0)]).unwrap() - 100.0).abs() < 0.01);
        // Geometrisch: halbe-halbe zwischen 100 und 400 Hz = 200 Hz.
        assert!((hz(&[a(0, 0.5), a(1, 0.5)]).unwrap() - 200.0).abs() < 0.1);
        // Ohne Hz: geschätzt nach Geschlecht.
        assert!((hz(&[a(2, 1.0)]).unwrap() - 210.0).abs() < 0.01);
        // Gar nichts bekannt: keine automatische Tonhöhe.
        assert_eq!(hz(&[a(3, 1.0)]), None);
        assert_eq!(ziel_hz(&t, "../m", &[a(0, 1.0)]), None);
        std::fs::remove_dir_all(&t).unwrap();
        assert!((crate::audio::ki::halbtoene_zu(120.0, 240.0) - 12.0).abs() < 1e-4);
    }

    #[test]
    fn liste_liest_modell_und_merkmale() {
        let t = std::env::temp_dir().join(format!("maskcue_ki_test_{}", std::process::id()));
        let m = t.join("test-modell");
        std::fs::create_dir_all(&m).unwrap();
        std::fs::create_dir_all(t.join("kein modell")).unwrap();
        std::fs::create_dir_all(t.join("..bad")).unwrap();
        std::fs::write(
            m.join("beatrice_paraphernalia_x.toml"),
            "[model]\nversion = \"2.0.0-rc.0\"\nname = \"Test\"\n[voice.0]\nname = \"A\"\naverage_pitch = 50.0\n[voice.1]\nname = \"B\"\naverage_pitch = 62.0\n",
        )
        .unwrap();
        std::fs::write(m.join("glottix_stimmen.json"), r#"[{"nr":1,"sprache":"de","geschlecht":"w","hell":0.8,"dialekt":"bayerisch","verstaendlich":87}]"#).unwrap();
        let l = liste(&t);
        let schaetzer = tonhoehen_schaetzer(&t);
        std::fs::remove_dir_all(&t).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(schaetzer, Some(t.join("test-modell").join("pitch_estimator.bin")));
        assert_eq!(l[0].ordner, "test-modell");
        assert_eq!(l[0].stimmen.len(), 2);
        assert_eq!(l[0].stimmen[0].merkmale.geschlecht, None);
        assert_eq!(l[0].stimmen[1].merkmale.geschlecht.as_deref(), Some("w"));
        assert_eq!(l[0].stimmen[1].merkmale.sprache.as_deref(), Some("de"));
        assert_eq!(l[0].stimmen[1].merkmale.dialekt.as_deref(), Some("bayerisch"));
        assert_eq!(l[0].stimmen[1].merkmale.verstaendlich, Some(87));
        assert_eq!(l[0].stimmen[0].merkmale.verstaendlich, None);
    }
}
