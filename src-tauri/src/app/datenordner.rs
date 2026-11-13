//! Daten-Ordner der App. Seit der Umbenennung (Dan, 01.11.2026) heißt er
//! `%APPDATA%\MaskCue`. Früher hieß er `%APPDATA%\GlottiX`.
//!
//! Beim Start zieht der alte Ordner einmal um (Umbenennen, kein Kopieren):
//! - Gibt es `MaskCue` schon, gilt `MaskCue`. Der alte Ordner bleibt unberührt.
//! - Gibt es nur `GlottiX`, wird er umbenannt. Danach stehen in
//!   `einstellungen.json` noch alte Pfade (Sounds). Die werden auf den neuen
//!   Ordner umgeschrieben.
//! - Geht das Umbenennen nicht (z. B. hält ein Programm eine Datei offen) oder
//!   das Umschreiben, bleibt alles beim alten Ordner. Beim nächsten Start gibt es
//!   einen neuen Versuch. Es geht nie etwas verloren.

use std::path::{Path, PathBuf};

pub const NEU: &str = "MaskCue";
pub const ALT: &str = "GlottiX";

/// Welcher Ordner gilt, und was beim Umzug passiert ist.
#[derive(Debug, PartialEq)]
pub struct Wahl {
    pub ordner: PathBuf,
    /// Meldung für das Protokoll (leer, wenn nichts Besonderes war).
    pub meldung: Option<String>,
}

fn ist_leer(o: &Path) -> bool {
    std::fs::read_dir(o).map(|mut d| d.next().is_none()).unwrap_or(false)
}

/// Schreibt in allen Texten der JSON-Datei den Pfad-Anfang `alt` auf `neu` um.
/// Groß/Klein und `\` gegen `/` sind egal. Gibt zurück, wie viele Texte sich
/// geändert haben.
pub fn pfade_umschreiben(v: &mut serde_json::Value, alt: &Path, neu: &Path) -> usize {
    let alt_text = alt.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    let neu_text = neu.to_string_lossy().into_owned();
    fn lauf(v: &mut serde_json::Value, alt: &str, neu: &str) -> usize {
        match v {
            serde_json::Value::String(s) => {
                let norm = s.replace('\\', "/");
                let kopf_ok = norm.len() >= alt.len()
                    && norm.is_char_boundary(alt.len())
                    && norm[..alt.len()].eq_ignore_ascii_case(alt)
                    && (norm.len() == alt.len() || norm.as_bytes()[alt.len()] == b'/');
                if kopf_ok {
                    // Rest nach dem Ordner, Trenner wie im alten Text behalten.
                    let rest = &s[alt.len()..];
                    *s = format!("{neu}{rest}");
                    1
                } else {
                    0
                }
            }
            serde_json::Value::Array(a) => a.iter_mut().map(|x| lauf(x, alt, neu)).sum(),
            serde_json::Value::Object(o) => o.values_mut().map(|x| lauf(x, alt, neu)).sum(),
            _ => 0,
        }
    }
    lauf(v, &alt_text, &neu_text)
}

/// Schreibt `einstellungen.json` im Ordner `neu` um (sicher: erst neue Datei,
/// dann ersetzen).
fn einstellungen_umschreiben(alt: &Path, neu: &Path) -> Result<usize, String> {
    let datei = neu.join("einstellungen.json");
    let text = match std::fs::read_to_string(&datei) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(format!("{}: {e}", datei.display())),
    };
    let mut v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", datei.display()))?;
    let n = pfade_umschreiben(&mut v, alt, neu);
    if n == 0 {
        return Ok(0);
    }
    let tmp = datei.with_extension("json.neu");
    let neu_text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, neu_text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &datei).map_err(|e| format!("{}: {e}", datei.display()))?;
    Ok(n)
}

/// Entscheidet unter `appdata`, welcher Ordner gilt, und zieht wenn nötig um.
pub fn waehlen(appdata: &Path) -> Wahl {
    let alt = appdata.join(ALT);
    let neu = appdata.join(NEU);
    let gilt = |ordner: &Path, meldung: Option<String>| Wahl { ordner: ordner.to_path_buf(), meldung };

    if !alt.is_dir() {
        return gilt(&neu, None);
    }
    if neu.is_dir() {
        if neu.join("einstellungen.json").exists() {
            return gilt(&neu, None);
        }
        if ist_leer(&neu) {
            // Leeres Überbleibsel: Umbenennen auf einen vorhandenen Ordner ginge nicht.
            if std::fs::remove_dir(&neu).is_err() {
                return gilt(&alt, Some(format!("{} ist da, aber nicht entfernbar. Es gilt weiter {}.", neu.display(), alt.display())));
            }
        } else {
            return gilt(&alt, Some(format!("Beide Ordner {} und {} haben Inhalt, {} ohne Einstellungen. Es gilt weiter {}.", alt.display(), neu.display(), neu.display(), alt.display())));
        }
    }
    if let Err(e) = std::fs::rename(&alt, &neu) {
        return gilt(&alt, Some(format!("Umzug nach {} ging nicht ({e}). Es gilt weiter der alte Ordner, nächster Versuch beim nächsten Start.", neu.display())));
    }
    match einstellungen_umschreiben(&alt, &neu) {
        Ok(n) => gilt(&neu, Some(format!("Daten-Ordner umgezogen nach {} ({n} Pfade angepasst).", appdata.join(NEU).display()))),
        Err(f) => {
            // Zurück, damit Pfade und Ordner wieder zusammenpassen.
            match std::fs::rename(&neu, &alt) {
                Ok(()) => gilt(&alt, Some(format!("Umzug zurückgenommen: {f}"))),
                Err(e) => gilt(&neu, Some(format!("Umzug halb fertig: {f}; Zurück ging nicht ({e}). Alte Pfade zeigen auf {}.", alt.display()))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let o = std::env::temp_dir().join(format!("maskcue_test_daten_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&o);
        std::fs::create_dir_all(&o).unwrap();
        o
    }

    fn alt_anlegen(a: &Path) -> PathBuf {
        let alt = a.join(ALT);
        std::fs::create_dir_all(alt.join("sounds")).unwrap();
        std::fs::create_dir_all(alt.join("bilder")).unwrap();
        std::fs::write(alt.join("sounds").join("wind.wav"), b"RIFF").unwrap();
        std::fs::write(alt.join("bilder").join("b.png"), b"png").unwrap();
        let json = serde_json::json!({
            "version": 1,
            "figuren": [{"name": "Funk", "bild": "datei:b.png"}],
            "sounds": [
                {"name": "Wind", "datei": alt.join("sounds").join("wind.wav")},
                {"name": "Fremd", "datei": "D:\\Musik\\GlottiX\\x.wav"},
                {"name": "Klein", "datei": alt.to_string_lossy().to_ascii_lowercase().replace('\\', "/") + "/sounds/wind.wav"}
            ]
        });
        std::fs::write(alt.join("einstellungen.json"), serde_json::to_string_pretty(&json).unwrap()).unwrap();
        alt
    }

    #[test]
    fn nichts_da_gilt_neu() {
        let a = temp("leer");
        let w = waehlen(&a);
        assert_eq!(w.ordner, a.join(NEU));
        assert!(w.meldung.is_none());
        assert!(!a.join(NEU).exists(), "darf nichts anlegen");
    }

    #[test]
    fn alter_ordner_zieht_um_mit_allen_dateien_und_pfaden() {
        let a = temp("umzug");
        let alt = alt_anlegen(&a);
        let w = waehlen(&a);
        let neu = a.join(NEU);
        assert_eq!(w.ordner, neu);
        assert!(!alt.exists(), "alter Ordner ist weg");
        assert_eq!(std::fs::read(neu.join("sounds").join("wind.wav")).unwrap(), b"RIFF");
        assert_eq!(std::fs::read(neu.join("bilder").join("b.png")).unwrap(), b"png");
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(neu.join("einstellungen.json")).unwrap()).unwrap();
        let s = &v["sounds"];
        let erwartet = neu.join("sounds").join("wind.wav");
        assert_eq!(s[0]["datei"].as_str().unwrap(), erwartet.to_string_lossy());
        assert_eq!(s[1]["datei"].as_str().unwrap(), "D:\\Musik\\GlottiX\\x.wav", "fremde Pfade bleiben");
        // Kleinschreibung und / im alten Pfad: gleicher Ordner, wird auch umgeschrieben.
        assert!(s[2]["datei"].as_str().unwrap().starts_with(&*neu.to_string_lossy()));
        assert_eq!(v["figuren"][0]["bild"], "datei:b.png");
        assert!(w.meldung.unwrap().contains("umgezogen"));
    }

    #[test]
    fn zweiter_start_aendert_nichts() {
        let a = temp("zweimal");
        alt_anlegen(&a);
        waehlen(&a);
        let vorher = std::fs::read(a.join(NEU).join("einstellungen.json")).unwrap();
        let w = waehlen(&a);
        assert_eq!(w.ordner, a.join(NEU));
        assert!(w.meldung.is_none());
        assert_eq!(std::fs::read(a.join(NEU).join("einstellungen.json")).unwrap(), vorher);
    }

    #[test]
    fn neuer_ordner_mit_einstellungen_gewinnt_alter_bleibt() {
        let a = temp("beide");
        let alt = alt_anlegen(&a);
        std::fs::create_dir_all(a.join(NEU)).unwrap();
        std::fs::write(a.join(NEU).join("einstellungen.json"), "{}").unwrap();
        let w = waehlen(&a);
        assert_eq!(w.ordner, a.join(NEU));
        assert!(alt.join("einstellungen.json").exists(), "alter Ordner bleibt unberührt");
    }

    #[test]
    fn leerer_neuer_ordner_stoppt_den_umzug_nicht() {
        let a = temp("leerneu");
        alt_anlegen(&a);
        std::fs::create_dir_all(a.join(NEU)).unwrap();
        let w = waehlen(&a);
        assert_eq!(w.ordner, a.join(NEU));
        assert!(a.join(NEU).join("sounds").join("wind.wav").exists());
        assert!(!a.join(ALT).exists());
    }

    #[test]
    fn neuer_ordner_mit_anderem_inhalt_ohne_einstellungen_bleibt_beim_alten() {
        let a = temp("fremdneu");
        let alt = alt_anlegen(&a);
        std::fs::create_dir_all(a.join(NEU)).unwrap();
        std::fs::write(a.join(NEU).join("x.txt"), "x").unwrap();
        let w = waehlen(&a);
        assert_eq!(w.ordner, alt);
        assert!(w.meldung.is_some());
        assert!(alt.join("einstellungen.json").exists());
    }

    #[test]
    fn kaputte_einstellungen_nehmen_den_umzug_zurueck() {
        let a = temp("kaputt");
        let alt = a.join(ALT);
        std::fs::create_dir_all(&alt).unwrap();
        std::fs::write(alt.join("einstellungen.json"), "{ kaputt").unwrap();
        std::fs::write(alt.join("daten.bin"), b"1").unwrap();
        let w = waehlen(&a);
        assert_eq!(w.ordner, alt, "zurück beim alten Ordner");
        assert!(alt.join("daten.bin").exists());
        assert_eq!(std::fs::read_to_string(alt.join("einstellungen.json")).unwrap(), "{ kaputt");
        assert!(!a.join(NEU).exists());
    }

    #[test]
    fn umzug_ohne_einstellungen_klappt() {
        let a = temp("ohnejson");
        std::fs::create_dir_all(a.join(ALT).join("aufnahmen")).unwrap();
        std::fs::write(a.join(ALT).join("aufnahmen").join("a.wav"), b"w").unwrap();
        let w = waehlen(&a);
        assert_eq!(w.ordner, a.join(NEU));
        assert!(a.join(NEU).join("aufnahmen").join("a.wav").exists());
    }

    #[test]
    fn pfad_nur_am_anfang_und_nur_ganze_ordner() {
        let mut v = serde_json::json!(["C:\\A\\GlottiX\\x.wav", "C:\\A\\GlottiX2\\x.wav", "C:\\A\\GlottiX", "dort C:\\A\\GlottiX\\x"]);
        let n = pfade_umschreiben(&mut v, Path::new("C:\\A\\GlottiX"), Path::new("C:\\A\\MaskCue"));
        assert_eq!(n, 2);
        assert_eq!(v[0], "C:\\A\\MaskCue\\x.wav");
        assert_eq!(v[1], "C:\\A\\GlottiX2\\x.wav");
        assert_eq!(v[2], "C:\\A\\MaskCue");
        assert_eq!(v[3], "dort C:\\A\\GlottiX\\x");
    }
}
