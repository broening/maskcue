//! Alle Einstellungen als ZIP sichern und wieder einspielen (z. B. für einen
//! neuen PC).
//!
//! Inhalt:
//! - `maskcue-einstellungen.json` (früher `glottix-einstellungen.json`, wird weiter gelesen): alle Einstellungen (Figuren, Hotkeys, …).
//! - `bilder/<name>`: eigene Bilder der Figuren.
//! - `sounds/<nummer>.<mp3|wav|ogg|flac>`: Sound-Dateien (nur „mit Sounds“).
//!   Ohne Sounds bleiben die Pfade der Sounds, wie sie sind (gleicher PC),
//!   aber nur lokale (`C:\...` oder im MaskCue-Ordner), nie `\\server\...`.
//!
//! Beim Import gelten dieselben Schutzregeln wie bei Figuren (`paket.rs`):
//! nur erlaubte Namen, Grenzen für die echten Bytes, Bild und Ton am Inhalt
//! geprüft, Ziel-Namen macht MaskCue selbst. Erst wird alles geprüft, dann
//! geschrieben. Die alten Einstellungen bleiben als Kopie liegen.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::einstellungen::{Einstellungen, bild_art};
use crate::audio::figur::{Ausschnitt, farbe_gueltig};
use super::paket::{Grenzen, fnv, lesen_begrenzt, sauber, ton_art, ton_kopf_pruefen};

pub const FORMAT: u32 = 1;
const JSON_NAME: &str = "maskcue-einstellungen.json";
/// Name bis zur Umbenennung in MaskCue (01.11.2026). Wird weiter gelesen.
const JSON_NAME_ALT: &str = "glottix-einstellungen.json";

fn ist_json(name: &str) -> bool {
    name == JSON_NAME || name == JSON_NAME_ALT
}
const TON_ENDUNGEN: [&str; 4] = ["mp3", "wav", "ogg", "flac"];
const BILD_ENDUNGEN: [&str; 4] = ["png", "jpg", "webp", "gif"];

#[derive(Serialize, Deserialize)]
struct SicherungJson {
    format: u32,
    /// Sound-Dateien liegen im ZIP (Pfade sind dann "sounds/<n>.<endung>").
    #[serde(default)]
    mit_sounds: bool,
    einstellungen: Einstellungen,
}

/// Was ein Import ergibt. Die Dateien liegen schon im App-Ordner.
#[derive(Debug)]
pub struct Import {
    pub einstellungen: Einstellungen,
    pub sounds: usize,
    pub bilder: usize,
    pub uebersprungen: usize,
}

fn endung(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Name eines eigenen Bildes, wie MaskCue ihn vergibt: 16 Hex-Zeichen + Endung.
fn bild_name_gueltig(name: &str) -> bool {
    match name.split_once('.') {
        Some((stamm, ext)) => stamm.len() == 16 && stamm.bytes().all(|b| b.is_ascii_hexdigit()) && BILD_ENDUNGEN.contains(&ext),
        None => false,
    }
}

/// Alle eigenen Bilder, auf die die Einstellungen zeigen ("datei:<name>").
fn bilder(e: &Einstellungen) -> Vec<String> {
    let mut v: Vec<String> = e
        .figuren
        .iter()
        .filter_map(|f| f.bild.as_deref())
        .chain(e.normal_bild.as_deref())
        .chain(e.sounds.iter().filter_map(|s| s.bild.as_deref()))
        .filter_map(|b| b.strip_prefix("datei:"))
        .map(str::to_string)
        .collect();
    v.sort();
    v.dedup();
    v
}

/// Sichert `e` als ZIP nach `ziel`. `mit_sounds`: Sound-Dateien kommen mit.
/// Fehlt ein Bild oder ein Sound, bricht es ab (sonst wäre die Sicherung lückenhaft).
pub fn exportieren(e: &Einstellungen, bilder_ordner: &Path, mit_sounds: bool, ziel: &Path) -> Result<(), String> {
    let datei = std::fs::File::create(ziel).map_err(|f| format!("{}: {f}", ziel.display()))?;
    let mut zip = ZipWriter::new(datei);
    let gepackt = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let abgelegt = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let fehler = |f: zip::result::ZipError| f.to_string();

    let mut kopie = e.clone();
    kopie.stumm = false;
    for name in bilder(e) {
        if !bild_name_gueltig(&name) {
            continue;
        }
        let daten = std::fs::read(bilder_ordner.join(&name)).map_err(|f| format!("Bild {name}: {f}"))?;
        zip.start_file(format!("bilder/{name}"), abgelegt).map_err(fehler)?;
        zip.write_all(&daten).map_err(|f| f.to_string())?;
    }
    if mit_sounds {
        for (i, s) in kopie.sounds.iter_mut().enumerate() {
            let ext = endung(&s.datei);
            if !TON_ENDUNGEN.contains(&ext.as_str()) {
                return Err(format!("Sound „{}“: Format .{ext} geht nicht", s.name));
            }
            let im_zip = format!("sounds/{i}.{ext}");
            let mut quelle = std::fs::File::open(&s.datei).map_err(|f| format!("Sound „{}“: {f}", s.name))?;
            ton_kopf_pruefen(&mut quelle, &s.name)?;
            zip.start_file(&im_zip, abgelegt).map_err(fehler)?;
            std::io::copy(&mut quelle, &mut zip).map_err(|f| f.to_string())?;
            s.datei = PathBuf::from(im_zip);
        }
    }
    let json = SicherungJson { format: FORMAT, mit_sounds, einstellungen: kopie };
    zip.start_file(JSON_NAME, gepackt).map_err(fehler)?;
    zip.write_all(serde_json::to_string_pretty(&json).map_err(|f| f.to_string())?.as_bytes()).map_err(|f| f.to_string())?;
    zip.finish().map_err(fehler)?;
    Ok(())
}

/// Erlaubte Namen im ZIP. Alles andere wird nicht einmal gelesen.
fn erlaubt(name: &str) -> bool {
    if ist_json(name) {
        return true;
    }
    if let Some(b) = name.strip_prefix("bilder/") {
        return bild_name_gueltig(b);
    }
    if let Some(rest) = name.strip_prefix("sounds/")
        && let Some((nr, ext)) = rest.split_once('.')
    {
        return !nr.is_empty() && nr.len() <= 4 && nr.bytes().all(|b| b.is_ascii_digit()) && TON_ENDUNGEN.contains(&ext);
    }
    false
}

/// Liest eine Sicherung. Bilder kommen nach `bilder_ordner`, Sounds nach
/// `sounds_ordner/<kennung>/`. Die Einstellungen werden nicht gespeichert:
/// Das macht der Aufrufer (nach [`alte_sichern`]).
pub fn importieren(zip_pfad: &Path, bilder_ordner: &Path, sounds_ordner: &Path, g: Grenzen) -> Result<Import, String> {
    let datei = std::fs::File::open(zip_pfad).map_err(|f| format!("{}: {f}", zip_pfad.display()))?;
    let mut zip = ZipArchive::new(datei).map_err(|f| format!("Kein gültiges ZIP: {f}"))?;
    if zip.len() > g.eintraege {
        return Err(format!("Zu viele Dateien in der Sicherung ({}).", zip.len()));
    }
    let mut json: Option<SicherungJson> = None;
    let mut dateien: HashMap<String, Vec<u8>> = HashMap::new();
    let mut gesamt = 0u64;
    let mut uebersprungen = 0;
    for i in 0..zip.len() {
        let mut eintrag = zip.by_index(i).map_err(|f| f.to_string())?;
        let name = eintrag.name().map_err(|f| f.to_string())?.into_owned();
        if eintrag.is_dir() || !erlaubt(&name) {
            uebersprungen += usize::from(!eintrag.is_dir());
            continue;
        }
        let grenze = if ist_json(&name) {
            g.json
        } else if name.starts_with("bilder/") {
            g.bild
        } else {
            g.sound
        };
        let daten = lesen_begrenzt(&mut eintrag, grenze.min(g.gesamt - gesamt), &name)?;
        gesamt += daten.len() as u64;
        if ist_json(&name) {
            let mut v: serde_json::Value = serde_json::from_slice(&daten).map_err(|f| format!("{JSON_NAME}: {f}"))?;
            if let Some(e) = v.get_mut("einstellungen") {
                super::einstellungen::alte_werte_entfernen(e);
            }
            json = Some(serde_json::from_value::<SicherungJson>(v).map_err(|f| format!("{JSON_NAME}: {f}"))?);
        } else {
            dateien.insert(name, daten);
        }
    }
    let json = json.ok_or("In der Datei fehlt maskcue-einstellungen.json. Ist das eine MaskCue-Sicherung?")?;
    if json.format > FORMAT {
        return Err("Die Sicherung ist von einer neueren MaskCue-Version.".into());
    }
    let mut e = json.einstellungen;
    e.stumm = false;

    // Erst alles prüfen, dann schreiben: Bei einem Fehler bleibt nichts liegen.
    let mut bilder_neu: Vec<(String, Vec<u8>)> = Vec::new();
    let mut umbenannt: HashMap<String, Option<String>> = HashMap::new();
    for name in bilder(&e) {
        let neu = match dateien.remove(&format!("bilder/{name}")) {
            Some(daten) => {
                let art = bild_art(&daten).ok_or_else(|| format!("Bild {name} ist kein Bild"))?;
                let datei = format!("{:016x}.{art}", fnv(&daten));
                bilder_neu.push((datei.clone(), daten));
                Some(datei)
            }
            // Nicht dabei: nur behalten, wenn der Name gültig ist und das Bild hier schon liegt.
            None => (bild_name_gueltig(&name) && bilder_ordner.join(&name).is_file()).then(|| name.clone()),
        };
        umbenannt.insert(name, neu);
    }
    let neu_bild = |b: &Option<String>| -> Option<String> {
        match b.as_deref() {
            Some(x) if x.starts_with("datei:") => umbenannt.get(&x[6..]).cloned().flatten().map(|n| format!("datei:{n}")),
            Some(x) if x.starts_with("avatar:") => {
                let id = sauber(&x[7..], 30);
                id.bytes().all(|c| c.is_ascii_lowercase()).then(|| format!("avatar:{id}"))
            }
            Some(x) if farbe_gueltig(x) => Some(x.to_string()),
            _ => None,
        }
    };
    for f in &mut e.figuren {
        f.bild = neu_bild(&f.bild);
        f.bild_ausschnitt = f.bild_ausschnitt.and_then(Ausschnitt::sauber);
        f.name = sauber(&f.name, 60);
    }
    e.normal_bild = neu_bild(&e.normal_bild);
    e.normal_ausschnitt = e.normal_ausschnitt.and_then(Ausschnitt::sauber);

    let mut sounds_neu: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    let kennung = format!("sicherung-{:016x}", fnv(&gesamt.to_le_bytes()) ^ fnv(zip_pfad.to_string_lossy().as_bytes()));
    let ziel_ordner = sounds_ordner.join(&kennung);
    for s in &mut e.sounds {
        s.bild = neu_bild(&s.bild);
        s.bild_ausschnitt = s.bild_ausschnitt.and_then(Ausschnitt::sauber);
        s.name = sauber(&s.name, 80);
        s.lautstaerke = if s.lautstaerke.is_finite() { s.lautstaerke.clamp(0.0, 2.0) } else { 1.0 };
        let relativ = s.datei.to_string_lossy().replace('\\', "/");
        if json.mit_sounds || relativ.starts_with("sounds/") {
            let daten = dateien.remove(&relativ).ok_or_else(|| format!("Sound „{}“ fehlt in der Sicherung", s.name))?;
            let art = ton_art(&daten).ok_or_else(|| format!("Sound „{}“ ist keine Ton-Datei", s.name))?;
            let nr = relativ.trim_start_matches("sounds/").split('.').next().unwrap_or("0").to_string();
            let pfad = ziel_ordner.join(format!("{nr}.{art}"));
            s.datei = pfad.clone();
            sounds_neu.push((pfad, daten));
        }
    }
    // Pfade, die nicht aus dem ZIP kommen, stammen vom Ersteller der Sicherung.
    // MaskCue lädt jeden Sound sofort und bei jedem Start: Nur lokale Pfade bleiben.
    let eigener_ordner = sounds_ordner.parent().unwrap_or(sounds_ordner);
    let vorher = e.sounds.len();
    e.sounds.retain(|s| s.datei.starts_with(&ziel_ordner) || pfad_erlaubt(&s.datei, eigener_ordner));
    uebersprungen += vorher - e.sounds.len();
    e.aufraeumen();
    e.sanieren();

    for (datei, daten) in &bilder_neu {
        std::fs::create_dir_all(bilder_ordner).map_err(|f| f.to_string())?;
        std::fs::write(bilder_ordner.join(datei), daten).map_err(|f| f.to_string())?;
    }
    if !sounds_neu.is_empty() {
        std::fs::create_dir_all(&ziel_ordner).map_err(|f| f.to_string())?;
    }
    for (pfad, daten) in &sounds_neu {
        std::fs::write(pfad, daten).map_err(|f| f.to_string())?;
    }
    Ok(Import { einstellungen: e, sounds: sounds_neu.len(), bilder: bilder_neu.len(), uebersprungen })
}

/// Darf ein Sound-Pfad aus einer Sicherung ohne Sound-Dateien bleiben?
/// Nur ein Pfad auf einem Laufwerk (`C:\...`) oder im eigenen MaskCue-Ordner,
/// nie mit `..`. Kein Netzwerk- oder Geräte-Pfad (`\\server\...`, `\??\UNC\...`,
/// `\\?\GLOBALROOT\...`): Windows meldet sich dort beim Laden mit dem eigenen
/// Konto an (NTLM). Erlaubt wird, was passt, nicht verboten, was auffällt.
fn pfad_erlaubt(p: &Path, eigener_ordner: &Path) -> bool {
    use std::path::{Component, Prefix};
    if p.components().any(|k| matches!(k, Component::ParentDir | Component::CurDir)) {
        return false;
    }
    let mut c = p.components();
    let laufwerk = matches!(c.next(), Some(Component::Prefix(x)) if matches!(x.kind(), Prefix::Disk(_)))
        && matches!(c.next(), Some(Component::RootDir));
    laufwerk || p.starts_with(eigener_ordner)
}

/// Kopiert die jetzige Einstellungs-Datei neben sich, mit Uhrzeit im Namen.
/// Gibt den Pfad der Kopie zurück (`None`, wenn es noch keine Datei gibt).
pub fn alte_sichern(datei: &Path) -> Result<Option<PathBuf>, String> {
    if !datei.is_file() {
        return Ok(None);
    }
    let sekunden = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let kopie = datei.with_file_name(format!("einstellungen.vor-import-{sekunden}.json"));
    std::fs::copy(datei, &kopie).map_err(|f| format!("Kopie der alten Einstellungen: {f}"))?;
    Ok(Some(kopie))
}

#[cfg(test)]
mod tests {
    #[test]
    fn alter_json_name_ist_erlaubt() {
        assert!(erlaubt(JSON_NAME) && erlaubt(JSON_NAME_ALT));
        assert!(!erlaubt("../glottix-einstellungen.json"));
    }

    use super::*;
    use crate::app::einstellungen::Sound;

    fn ordner(name: &str) -> PathBuf {
        let o = std::env::temp_dir().join(format!("maskcue-sicherung-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&o);
        std::fs::create_dir_all(&o).unwrap();
        o
    }

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
    const WAV: &[u8] = b"RIFF\x24\x00\x00\x00WAVEfmt ";

    fn beispiel(o: &Path) -> Einstellungen {
        let mut e = Einstellungen::default();
        std::fs::create_dir_all(o.join("bilder")).unwrap();
        std::fs::write(o.join("bilder").join("00000000000000ab.png"), PNG).unwrap();
        e.figuren[0].bild = Some("datei:00000000000000ab.png".into());
        e.figuren[1].bild = Some("farbe:#22d3ee".into());
        std::fs::write(o.join("wind.wav"), WAV).unwrap();
        e.sounds.push(Sound { name: "Wind".into(), datei: o.join("wind.wav"), ..Default::default() });
        e
    }

    #[test]
    fn hin_und_zurueck_mit_sounds() {
        let a = ordner("a");
        let e = beispiel(&a);
        let zip = a.join("s.zip");
        exportieren(&e, &a.join("bilder"), true, &zip).unwrap();
        let b = ordner("b");
        let imp = importieren(&zip, &b.join("bilder"), &b.join("sounds"), Grenzen::default()).unwrap();
        assert_eq!((imp.sounds, imp.bilder, imp.uebersprungen), (1, 1, 0));
        let n = imp.einstellungen;
        assert_eq!(n.figuren.len(), e.figuren.len());
        assert_eq!(n.hotkeys, e.hotkeys);
        assert_eq!(n.figuren[1].bild.as_deref(), Some("farbe:#22d3ee"));
        // Bild neu benannt (Inhalt), liegt im neuen Ordner.
        let bild = n.figuren[0].bild.clone().unwrap();
        let datei = bild.strip_prefix("datei:").unwrap();
        assert_eq!(std::fs::read(b.join("bilder").join(datei)).unwrap(), PNG);
        // Sound liegt im neuen Ordner, Pfad zeigt dorthin.
        assert!(n.sounds[0].datei.starts_with(b.join("sounds")));
        assert_eq!(std::fs::read(&n.sounds[0].datei).unwrap(), WAV);
    }

    #[test]
    fn ohne_sounds_bleiben_die_pfade() {
        let a = ordner("c");
        let e = beispiel(&a);
        let zip = a.join("s.zip");
        exportieren(&e, &a.join("bilder"), false, &zip).unwrap();
        let imp = importieren(&zip, &a.join("bilder2"), &a.join("sounds"), Grenzen::default()).unwrap();
        assert_eq!(imp.sounds, 0);
        assert_eq!(imp.einstellungen.sounds[0].datei, a.join("wind.wav"));
    }

    fn zip_mit(pfad: &Path, eintraege: &[(&str, &[u8])]) {
        let mut z = ZipWriter::new(std::fs::File::create(pfad).unwrap());
        for (n, d) in eintraege {
            z.start_file(*n, SimpleFileOptions::default()).unwrap();
            z.write_all(d).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn fremde_namen_und_falsche_dateien() {
        let o = ordner("d");
        let mut e = Einstellungen::default();
        e.sounds.push(Sound { name: "Böse".into(), datei: "sounds/0.wav".into(), ..Default::default() });
        e.figuren[0].bild = Some("datei:../../boese.png".into());
        let json = serde_json::to_vec(&SicherungJson { format: 1, mit_sounds: true, einstellungen: e }).unwrap();
        // Zip Slip und fremde Dateien werden übersprungen, nicht geschrieben.
        let zip = o.join("a.zip");
        zip_mit(&zip, &[(JSON_NAME, &json), ("../../evil.exe", b"MZ"), ("bilder/../x.png", PNG), ("sounds/0.wav", b"MZ kein Ton")]);
        let f = importieren(&zip, &o.join("bilder"), &o.join("sounds"), Grenzen::default()).unwrap_err();
        assert!(f.contains("keine Ton-Datei"), "{f}");
        assert!(!o.join("bilder").exists() && !o.join("sounds").exists(), "nichts halb geschrieben");
        assert!(!o.parent().unwrap().join("evil.exe").exists());
        // Ohne den kaputten Sound geht es; das Bild mit fremdem Namen fällt weg.
        let mut e = Einstellungen::default();
        e.figuren[0].bild = Some("datei:../../boese.png".into());
        let json = serde_json::to_vec(&SicherungJson { format: 1, mit_sounds: false, einstellungen: e }).unwrap();
        zip_mit(&zip, &[(JSON_NAME, &json), ("../../evil.exe", b"MZ")]);
        let imp = importieren(&zip, &o.join("bilder"), &o.join("sounds"), Grenzen::default()).unwrap();
        assert_eq!(imp.uebersprungen, 1);
        assert_eq!(imp.einstellungen.figuren[0].bild, None);
    }

    #[test]
    fn zu_grosse_einstellungen() {
        let o = ordner("e");
        let zip = o.join("a.zip");
        zip_mit(&zip, &[(JSON_NAME, &vec![b' '; 2000])]);
        let g = Grenzen { json: 1000, ..Grenzen::default() };
        assert!(importieren(&zip, &o.join("b"), &o.join("s"), g).unwrap_err().contains("zu groß"));
    }

    #[test]
    fn alte_einstellungen_bleiben_als_kopie() {
        let o = ordner("f");
        let d = o.join("einstellungen.json");
        assert_eq!(alte_sichern(&d).unwrap(), None);
        std::fs::write(&d, "{}").unwrap();
        let k = alte_sichern(&d).unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(k).unwrap(), "{}");
    }

    /// Fremde Sicherung ohne Sound-Dateien: Netzwerk- und Geraete-Pfade
    /// fliegen raus (Windows meldet sich sonst beim Laden mit dem eigenen Konto
    /// am fremden Server an). Lokale Pfade bleiben (gleicher PC).
    #[test]
    fn fremde_sound_pfade_fliegen_raus() {
        let a = ordner("fremd");
        let lokal = a.join("wind.wav");
        let boese = [
            r"\\127.0.0.1\maskcue-share\a.wav",
            r"//127.0.0.1/maskcue-share/b.wav",
            r"\\?\UNC\127.0.0.1\maskcue-share\c.wav",
            r"\??\UNC\127.0.0.1\maskcue-share\d.wav",
            r"\\.\pipe\maskcue\e.wav",
            r"\\?\GLOBALROOT\Device\Mup\127.0.0.1\s\f.wav",
            r"\\127.0.0.1@80\s\g.wav",
            r"C:\x\..\..\h.wav",
            r"C:i.wav",
            r"j.wav",
        ];
        let mut e = Einstellungen::default();
        e.sounds.clear();
        e.sounds.push(Sound { name: "Lokal".into(), datei: lokal.clone(), ..Default::default() });
        e.sounds.push(Sound { name: "Laufwerk".into(), datei: PathBuf::from("D:/Sounds/Wind.wav"), ..Default::default() });
        for (i, p) in boese.iter().enumerate() {
            e.sounds.push(Sound { name: format!("Harmlos {i}"), datei: PathBuf::from(p), ..Default::default() });
        }
        let json = serde_json::to_vec(&SicherungJson { format: FORMAT, mit_sounds: false, einstellungen: e }).unwrap();
        let zip = a.join("fremd.zip");
        zip_mit(&zip, &[(JSON_NAME, &json)]);
        let imp = importieren(&zip, &a.join("bilder"), &a.join("sounds"), Grenzen::default()).unwrap();
        let pfade: Vec<&Path> = imp.einstellungen.sounds.iter().map(|s| s.datei.as_path()).collect();
        assert_eq!(pfade, vec![lokal.as_path(), Path::new("D:/Sounds/Wind.wav")], "nur lokale Pfade bleiben");
        assert_eq!(imp.uebersprungen, boese.len());
        // Nichts davon wird geladen (gleicher Weg wie Motor::sounds_abgleichen).
        let mut v = crate::app::sounds::SoundVerwaltung::default();
        let zu_laden = v.abgleichen(&imp.einstellungen.sounds, None);
        assert!(zu_laden.iter().all(|p| !p.to_string_lossy().contains("127.0.0.1")), "{zu_laden:?}");
    }

    /// Export „mit Sounds“: Eine Datei mit Ton-Endung, die kein Ton ist, kommt
    /// nicht ins ZIP (sonst ließe sich jede Datei mit passendem Namen mitnehmen).
    #[test]
    fn export_nimmt_nur_echte_toene() {
        let a = ordner("export_kein_ton");
        let falsch = a.join("notizen.wav");
        std::fs::write(&falsch, b"GEHEIM: kein Ton").unwrap();
        let mut e = Einstellungen::default();
        e.sounds.clear();
        e.sounds.push(Sound { name: "Lachen".into(), datei: falsch, ..Default::default() });
        let f = exportieren(&e, &a.join("bilder"), true, &a.join("raus.zip")).unwrap_err();
        assert!(f.contains("keine Ton-Datei"), "{f}");
    }

    /// Liegt der eigene MaskCue-Ordner auf einem Netzlaufwerk (umgeleitetes
    /// AppData), bleiben Sounds darin erlaubt.
    #[test]
    fn eigener_ordner_auf_netzlaufwerk_bleibt() {
        let eigen = Path::new(r"\\firma\profile\nutzer\MaskCue");
        assert!(pfad_erlaubt(&eigen.join("sounds").join("applaus.ogg"), eigen));
        assert!(pfad_erlaubt(&eigen.join("aufnahmen").join("a.wav"), eigen));
        assert!(!pfad_erlaubt(&eigen.join("..").join("..").join("x.wav"), eigen));
        assert!(!pfad_erlaubt(Path::new(r"\\firma\anderes\x.wav"), eigen));
    }

    /// Sicherung mit Einstellungen als JSON-Wert (zum Verfälschen).
    fn sicherung_mit(a: &Path, e: serde_json::Value) -> Import {
        let json = format!(r#"{{"format":1,"mit_sounds":false,"einstellungen":{e}}}"#);
        let zip = a.join("s.zip");
        zip_mit(&zip, &[(JSON_NAME, json.as_bytes())]);
        importieren(&zip, &a.join("bilder"), &a.join("sounds"), Grenzen::default()).unwrap()
    }

    /// Fremde Sicherung mit unsinnigen Zahlen: 1e39 wird beim Lesen
    /// „unendlich“, gespeichert als `null`, und beim nächsten Start wären alle
    /// Einstellungen weg. Nach dem Import liegt alles im Bereich der Regler.
    #[test]
    fn fremde_zahlen_landen_im_bereich() {
        let a = ordner("zahlen");
        let gross: serde_json::Value = serde_json::from_str("1e39").unwrap();
        let mut e = serde_json::to_value(Einstellungen::default()).unwrap();
        e["geraete"]["eingang"] = "Fremdes Mikro".into();
        e["mikro"]["kompressor"]["verstaerkung_db"] = gross.clone();
        e["mikro"]["kompressor"]["schwelle_db"] = gross.clone();
        e["mikro"]["hochpass_hz"] = gross.clone();
        e["mikro"]["bass_db"] = 500.into();
        e["mikro"]["hoehen_db"] = (-500).into();
        e["mikro"]["denoise_spiel"]["daempfung_db"] = gross.clone();
        e["mikro"]["gate_spiel"]["schwelle_db"] = gross.clone();
        e["mischer"]["ducking_db"] = serde_json::from_str("-1e39").unwrap();
        e["figuren"][0]["formant"] = gross.clone();
        e["figuren"][1]["hell"] = gross.clone();
        e["figuren"][1]["umgebung_staerke"] = gross;
        e["umgebungen"] = serde_json::json!({"Halle": 1e30});
        e["ki"] = serde_json::json!({"eigene_hz": 1e30});
        let n = sicherung_mit(&a, e).einstellungen;
        let m = &n.mikro;
        assert!((0.0..=18.0).contains(&m.kompressor.verstaerkung_db), "verstaerkung {}", m.kompressor.verstaerkung_db);
        assert!(m.kompressor.schwelle_db.is_finite() && m.kompressor.schwelle_db <= 0.0);
        assert!((0.0..=200.0).contains(&m.hochpass_hz));
        assert!((-12.0..=12.0).contains(&m.bass_db) && (-12.0..=12.0).contains(&m.hoehen_db));
        assert!((0.0..=100.0).contains(&m.denoise_spiel.daempfung_db));
        assert!(m.gate_spiel.schwelle_db <= 0.0);
        assert!((-30.0..=0.0).contains(&n.mischer.ducking_db));
        assert!((-4.0..=12.0).contains(&n.figuren[0].formant), "formant {}", n.figuren[0].formant);
        assert!((-6.0..=6.0).contains(&n.figuren[1].hell));
        assert!(n.figuren[1].umgebung_staerke.is_none_or(|s| (0.0..=1.0).contains(&s)));
        assert!(n.umgebungen.values().all(|s| (0.0..=1.0).contains(s)));
        assert!(n.ki.eigene_hz.is_none_or(|h| (50.0..=1000.0).contains(&h)));
        // Speichern und neu laden: nichts geht verloren.
        let d = a.join("einstellungen.json");
        n.speichern(&d).unwrap();
        let (g, meldung) = Einstellungen::laden(&d);
        assert_eq!(meldung, None);
        assert_eq!(g.geraete.eingang, "Fremdes Mikro");
    }

    /// Fremde Sicherung mit 49 000 KI-Stimmen in einer Figur: Jede kostet beim
    /// Wählen der Figur 256 KiB. Nach dem Import sind es höchstens 8 (wie beim
    /// Figuren-Paket), ein ungültiger Modell-Name fällt weg.
    #[test]
    fn fremde_ki_mischung_wird_begrenzt() {
        let a = ordner("ki-mischung");
        let mut e = serde_json::to_value(Einstellungen::default()).unwrap();
        let stimmen: Vec<serde_json::Value> = (0..49_000).map(|_| serde_json::json!({"nr": 0, "gewicht": 1})).collect();
        e["figuren"][0]["ki"] = serde_json::json!({"modell": "thorsten", "stimmen": stimmen});
        e["figuren"][0]["ki_an"] = true.into();
        e["figuren"][1]["ki"] = serde_json::json!({"modell": "..\\..\\x", "stimmen": []});
        let n = sicherung_mit(&a, e).einstellungen;
        let k = n.figuren[0].ki.as_ref().unwrap();
        assert!(k.stimmen.len() <= 8, "stimmen {}", k.stimmen.len());
        assert!(n.figuren[1].ki.is_none(), "ungültiger Modell-Name fällt weg");
    }
}
