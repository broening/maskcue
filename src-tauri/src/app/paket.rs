//! Figuren als ZIP exportieren und importieren.
//!
//! Inhalt eines Pakets:
//! - `maskcue-figur.json` (früher `glottix-figur.json`, wird weiter gelesen): Figur (Name, Stimme, Bild) und ihre Sounds.
//! - `bild.<png|jpg|webp|gif>`: eigenes Bild (optional).
//! - `sounds/<nummer>.<mp3|wav|ogg|flac>`: Sounds der Figur (optional).
//!
//! Schutz beim Import (PLAN.md, „ZIP-Import sicher machen“):
//! - Nur Dateien mit genau diesen Namen werden gelesen, alles andere wird
//!   übersprungen. Ziel-Namen erzeugt MaskCue selbst, nie aus dem ZIP. Ein
//!   Name wie `../../evil.exe` kann also nichts außerhalb anlegen (Zip Slip).
//! - Grenzen je Datei und gesamt. Gezählt werden die wirklich entpackten
//!   Bytes, nicht die Angaben im ZIP (die kann ein Angreifer fälschen).
//!   So bricht eine ZIP-Bombe nach der Grenze ab.
//! - Bild und Ton werden am Inhalt geprüft (erste Bytes), nicht an der Endung.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::audio::figur::{Ausschnitt, FigurDef, Verfahren, farbe_gueltig};
use crate::audio::ki::KiWahl;
use crate::audio::wsola;

use super::einstellungen::{Einstellungen, NochmalWahl, Sound, bild_art};

pub const FORMAT: u32 = 1;
const JSON_NAME: &str = "maskcue-figur.json";
/// Name bis zur Umbenennung in MaskCue (01.11.2026). Wird weiter gelesen.
const JSON_NAME_ALT: &str = "glottix-figur.json";

fn ist_json(name: &str) -> bool {
    name == JSON_NAME || name == JSON_NAME_ALT
}

/// Grenzen beim Import. Im Test kleiner, damit Tests schnell bleiben.
#[derive(Clone, Copy, Debug)]
pub struct Grenzen {
    pub json: u64,
    pub bild: u64,
    pub sound: u64,
    pub gesamt: u64,
    pub eintraege: usize,
}

impl Default for Grenzen {
    fn default() -> Self {
        Self { json: 1024 * 1024, bild: 8 * 1024 * 1024, sound: 100 * 1024 * 1024, gesamt: 500 * 1024 * 1024, eintraege: 300 }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct PaketSound {
    name: String,
    /// Pfad im ZIP, z. B. "sounds/0.mp3".
    datei: String,
    #[serde(default = "eins")]
    lautstaerke: f32,
    #[serde(default)]
    schleife: bool,
    #[serde(default)]
    musik: bool,
    #[serde(default)]
    nochmal_neu: bool,
    #[serde(default = "wahr")]
    spiel: bool,
    #[serde(default)]
    chat: bool,
}

fn eins() -> f32 {
    1.0
}
fn wahr() -> bool {
    true
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct PaketJson {
    format: u32,
    name: String,
    halbtoene: f32,
    formant: f32,
    /// "avatar:<id>", "farbe:#rrggbb" oder "datei:<pfad im zip>".
    #[serde(default)]
    bild: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bild_ausschnitt: Option<Ausschnitt>,
    /// Umgebung der Figur, z. B. "Hoehle". Unbekannt = keine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    umgebung: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    umgebung_staerke: Option<f32>,
    /// "psola", "signalsmith", "wsola" oder "ohne"; alt auch "ki" (= ohne mit
    /// KI-Stimme). Unbekannt = PSOLA.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    verfahren: Option<String>,
    /// KI-Stimme (Modell muss beim Empfänger vorhanden sein).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ki: Option<KiWahl>,
    /// KI-Stimme vor dem Verfahren an.
    #[serde(default)]
    ki_an: bool,
    /// Hell/Dunkel in dB.
    #[serde(default)]
    hell: f32,
    #[serde(default)]
    sounds: Vec<PaketSound>,
}

fn endung(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Exportiert eine Figur mit den genannten Sounds als ZIP nach `ziel`.
pub fn exportieren(figur: &FigurDef, sounds: &[Sound], bilder_ordner: &Path, ziel: &Path) -> Result<(), String> {
    let datei = std::fs::File::create(ziel).map_err(|e| format!("{}: {e}", ziel.display()))?;
    let mut zip = ZipWriter::new(datei);
    let gepackt = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    // Ton-Dateien sind schon gepackt: nur ablegen.
    let abgelegt = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let fehler = |e: zip::result::ZipError| e.to_string();

    let mut bild = figur.bild.clone();
    if let Some(name) = figur.bild.as_deref().and_then(|b| b.strip_prefix("datei:")) {
        let quelle = bilder_ordner.join(name);
        let daten = std::fs::read(&quelle).map_err(|e| format!("Bild {}: {e}", quelle.display()))?;
        let art = bild_art(&daten).ok_or("Bild der Figur ist kaputt")?;
        let im_zip = format!("bild.{art}");
        zip.start_file(&im_zip, abgelegt).map_err(fehler)?;
        zip.write_all(&daten).map_err(|e| e.to_string())?;
        bild = Some(format!("datei:{im_zip}"));
    }

    let mut liste = Vec::new();
    for (i, s) in sounds.iter().enumerate() {
        let ext = endung(&s.datei);
        if !["mp3", "wav", "ogg", "flac"].contains(&ext.as_str()) {
            return Err(format!("Sound „{}“: Format .{ext} geht nicht", s.name));
        }
        let im_zip = format!("sounds/{i}.{ext}");
        let mut quelle = std::fs::File::open(&s.datei).map_err(|e| format!("Sound „{}“: {e}", s.name))?;
        ton_kopf_pruefen(&mut quelle, &s.name)?;
        zip.start_file(&im_zip, abgelegt).map_err(fehler)?;
        std::io::copy(&mut quelle, &mut zip).map_err(|e| e.to_string())?;
        liste.push(PaketSound {
            name: s.name.clone(),
            datei: im_zip,
            lautstaerke: s.lautstaerke,
            schleife: s.schleife,
            musik: s.musik,
            nochmal_neu: s.nochmal == NochmalWahl::Neu,
            spiel: s.spiel,
            chat: s.chat,
        });
    }

    let umgebung = figur.umgebung.and_then(|u| serde_json::to_value(u).ok()).and_then(|v| v.as_str().map(str::to_string));
    let json = PaketJson { format: FORMAT, name: figur.name.clone(), halbtoene: figur.halbtoene, formant: figur.formant, bild, bild_ausschnitt: figur.bild_ausschnitt, umgebung, umgebung_staerke: figur.umgebung_staerke,
        verfahren: serde_json::to_value(figur.verfahren).ok().and_then(|v| v.as_str().map(str::to_string)),
        ki: figur.ki.clone(),
        ki_an: figur.ki_an,
        hell: figur.hell,
        sounds: liste,
    };
    zip.start_file(JSON_NAME, gepackt).map_err(fehler)?;
    zip.write_all(serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?.as_bytes()).map_err(|e| e.to_string())?;
    zip.finish().map_err(fehler)?;
    Ok(())
}

/// Was ein Import ergibt. Die Dateien liegen schon im App-Ordner.
#[derive(Debug)]
pub struct Import {
    pub figur: FigurDef,
    pub sounds: Vec<Sound>,
    /// Übersprungene Einträge (nicht erlaubte Namen).
    pub uebersprungen: usize,
}

/// Erlaubte Namen im ZIP. Alles andere wird nicht einmal gelesen.
fn erlaubt(name: &str) -> bool {
    if ist_json(name) {
        return true;
    }
    if let Some(ext) = name.strip_prefix("bild.") {
        return ["png", "jpg", "webp", "gif"].contains(&ext);
    }
    if let Some(rest) = name.strip_prefix("sounds/")
        && let Some((nr, ext)) = rest.split_once('.')
    {
        return !nr.is_empty() && nr.len() <= 4 && nr.bytes().all(|b| b.is_ascii_digit()) && ["mp3", "wav", "ogg", "flac"].contains(&ext);
    }
    false
}

/// Ton-Art am Inhalt: MP3 (ID3 oder Frame-Sync), WAV, OGG, FLAC.
pub fn ton_art(kopf: &[u8]) -> Option<&'static str> {
    match kopf {
        [b'I', b'D', b'3', ..] => Some("mp3"),
        [0xFF, b, ..] if b & 0xE0 == 0xE0 => Some("mp3"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'A', b'V', b'E', ..] => Some("wav"),
        [b'O', b'g', b'g', b'S', ..] => Some("ogg"),
        [b'f', b'L', b'a', b'C', ..] => Some("flac"),
        _ => None,
    }
}

/// Beim Export: Ist die Datei wirklich ein Ton? Sonst käme jede Datei mit
/// passender Endung ins ZIP, das der Nutzer weitergibt. Danach steht die
/// Datei wieder am Anfang.
pub(crate) fn ton_kopf_pruefen(f: &mut std::fs::File, name: &str) -> Result<(), String> {
    use std::io::{Seek, SeekFrom};
    let mut kopf = Vec::with_capacity(12);
    Read::by_ref(f).take(12).read_to_end(&mut kopf).map_err(|e| format!("Sound „{name}“: {e}"))?;
    ton_art(&kopf).ok_or_else(|| format!("Sound „{name}“ ist keine Ton-Datei"))?;
    f.seek(SeekFrom::Start(0)).map_err(|e| format!("Sound „{name}“: {e}"))?;
    Ok(())
}

/// Liest höchstens `grenze` Bytes. Mehr = Fehler (auch wenn das ZIP etwas
/// anderes behauptet).
pub(crate) fn lesen_begrenzt(r: &mut impl Read, grenze: u64, name: &str) -> Result<Vec<u8>, String> {
    let mut daten = Vec::new();
    r.take(grenze + 1).read_to_end(&mut daten).map_err(|e| format!("{name}: {e}"))?;
    if daten.len() as u64 > grenze {
        return Err(format!("{name} ist zu groß (Grenze {} MB).", grenze / 1024 / 1024));
    }
    Ok(daten)
}

/// Text aus dem Paket: ohne Steuerzeichen, gekürzt.
pub(crate) fn sauber(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

pub(crate) fn fnv(daten: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in daten {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Importiert ein Paket. Bilder kommen nach `bilder_ordner`, Sounds nach
/// `sounds_ordner/<kennung>/`. Bei einem Fehler bleibt nichts halb liegen.
pub fn importieren(zip_pfad: &Path, bilder_ordner: &Path, sounds_ordner: &Path, g: Grenzen) -> Result<Import, String> {
    let datei = std::fs::File::open(zip_pfad).map_err(|e| format!("{}: {e}", zip_pfad.display()))?;
    let mut zip = ZipArchive::new(datei).map_err(|e| format!("Kein gültiges ZIP: {e}"))?;
    if zip.len() > g.eintraege {
        return Err(format!("Zu viele Dateien im Paket ({}).", zip.len()));
    }

    let mut json: Option<PaketJson> = None;
    let mut bild: Option<(String, Vec<u8>)> = None;
    let mut toene: HashMap<String, Vec<u8>> = HashMap::new();
    let mut gesamt = 0u64;
    let mut uebersprungen = 0;
    for i in 0..zip.len() {
        let mut eintrag = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = eintrag.name().map_err(|e| e.to_string())?.into_owned();
        if eintrag.is_dir() || !erlaubt(&name) {
            uebersprungen += usize::from(!eintrag.is_dir());
            continue;
        }
        let grenze = if ist_json(&name) {
            g.json
        } else if name.starts_with("bild.") {
            g.bild
        } else {
            g.sound
        };
        let daten = lesen_begrenzt(&mut eintrag, grenze.min(g.gesamt - gesamt), &name)?;
        gesamt += daten.len() as u64;
        if ist_json(&name) {
            json = Some(serde_json::from_slice(&daten).map_err(|e| format!("{JSON_NAME}: {e}"))?);
        } else if name.starts_with("bild.") {
            bild = Some((name, daten));
        } else {
            toene.insert(name, daten);
        }
    }
    let json = json.ok_or("Im Paket fehlt maskcue-figur.json. Ist das eine MaskCue-Figur?")?;
    if json.format > FORMAT {
        return Err("Das Paket ist von einer neueren MaskCue-Version.".into());
    }

    // Erst alles prüfen, dann schreiben: Bei einem Fehler bleibt nichts liegen.
    let name = sauber(&json.name, 60);
    let name = if name.is_empty() { "Figur".to_string() } else { name };
    let mut bild_neu: Option<(String, Vec<u8>)> = None;
    let bild_wert = match json.bild.as_deref() {
        Some(b) if b.starts_with("avatar:") => {
            let id = sauber(&b[7..], 30);
            id.bytes().all(|c| c.is_ascii_lowercase()).then(|| format!("avatar:{id}"))
        }
        Some(b) if b.starts_with("datei:") => {
            let (_, daten) = bild.take().ok_or("Bild fehlt im Paket")?;
            let art = bild_art(&daten).ok_or("Das Bild im Paket ist kein Bild")?;
            let datei = format!("{:016x}.{art}", fnv(&daten));
            bild_neu = Some((datei.clone(), daten));
            Some(format!("datei:{datei}"))
        }
        Some(b) if farbe_gueltig(b) => Some(b.to_string()),
        _ => None,
    };
    let mut sounds_neu: Vec<(PathBuf, Vec<u8>, Sound)> = Vec::new();
    let kennung = format!("{:016x}", fnv(json.name.as_bytes()) ^ fnv(&gesamt.to_le_bytes()));
    let ziel_ordner = sounds_ordner.join(&kennung);
    for (i, s) in json.sounds.iter().enumerate() {
        let daten = toene.remove(&s.datei).ok_or_else(|| format!("Sound „{}“ fehlt im Paket", s.name))?;
        let art = ton_art(&daten).ok_or_else(|| format!("Sound „{}“ ist keine Ton-Datei", s.name))?;
        let pfad = ziel_ordner.join(format!("{i}.{art}"));
        let sound = Sound {
            name: sauber(&s.name, 80),
            datei: pfad.clone(),
            lautstaerke: s.lautstaerke.clamp(0.0, 2.0),
            schleife: s.schleife,
            musik: s.musik,
            nochmal: if s.nochmal_neu { NochmalWahl::Neu } else { NochmalWahl::Stopp },
            spiel: s.spiel,
            chat: s.chat,
            kategorie: String::new(),
            bild: None,
            bild_ausschnitt: None,
        };
        sounds_neu.push((pfad, daten, sound));
    }

    if let Some((datei, daten)) = bild_neu {
        std::fs::create_dir_all(bilder_ordner).map_err(|e| e.to_string())?;
        std::fs::write(bilder_ordner.join(datei), daten).map_err(|e| e.to_string())?;
    }
    if !sounds_neu.is_empty() {
        std::fs::create_dir_all(&ziel_ordner).map_err(|e| e.to_string())?;
    }
    let mut sounds = Vec::new();
    for (pfad, daten, sound) in sounds_neu {
        std::fs::write(&pfad, daten).map_err(|e| e.to_string())?;
        sounds.push(sound);
    }
    // Alte Pakete: Verfahren „ki“ = KI-Stimme ohne weiteres Verfahren.
    let alt_ki = json.verfahren.as_deref() == Some("ki");
    Ok(Import {
        figur: FigurDef {
            name,
            halbtoene: if json.halbtoene.is_finite() { json.halbtoene.clamp(wsola::HALBTOENE_MIN, wsola::HALBTOENE_MAX) } else { 0.0 },
            formant: json.formant.clamp(-4.0, 12.0),
            bild: bild_wert,
            bild_ausschnitt: json.bild_ausschnitt.and_then(Ausschnitt::sauber),
            umgebung: json.umgebung.and_then(|u| serde_json::from_value(serde_json::Value::String(u)).ok()),
            umgebung_staerke: json.umgebung_staerke.filter(|s| s.is_finite()).map(|s| s.clamp(0.0, 1.0)),
            verfahren: if alt_ki { Verfahren::Ohne } else { json.verfahren.and_then(|v| serde_json::from_value::<Verfahren>(serde_json::Value::String(v)).ok()).unwrap_or_default() },
            ki: json.ki.and_then(KiWahl::sauber),
            ki_an: json.ki_an || alt_ki,
            hell: if json.hell.is_finite() { json.hell.clamp(-6.0, 6.0) } else { 0.0 },
        },
        sounds,
        uebersprungen,
    })
}

/// Fügt einen Import in die Einstellungen ein. Gleicher Figur-Name: „Name (2)“.
pub fn einfuegen(e: &mut Einstellungen, imp: Import) -> String {
    let basis = imp.figur.name.clone();
    let mut name = basis.clone();
    let mut n = 2;
    while e.figuren.iter().any(|f| f.name.eq_ignore_ascii_case(&name)) {
        name = format!("{basis} ({n})");
        n += 1;
    }
    e.figuren.push(FigurDef { name: name.clone(), ..imp.figur });
    e.sounds.extend(imp.sounds);
    e.aufraeumen();
    name
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::umgebung::Umgebung;

    fn ordner(name: &str) -> PathBuf {
        let o = std::env::temp_dir().join(format!("maskcue_test_paket_{name}"));
        let _ = std::fs::remove_dir_all(&o);
        std::fs::create_dir_all(&o).unwrap();
        o
    }

    fn wav(n: usize) -> Vec<u8> {
        let mut d = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        d.resize(n, 7);
        d
    }

    /// Eigenes ZIP bauen (auch mit bösen Einträgen).
    fn zip_bauen(pfad: &Path, eintraege: &[(&str, &[u8])]) {
        let mut z = ZipWriter::new(std::fs::File::create(pfad).unwrap());
        for (n, d) in eintraege {
            z.start_file(*n, SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)).unwrap();
            z.write_all(d).unwrap();
        }
        z.finish().unwrap();
    }

    const JSON: &str = r#"{"format":1,"name":"Grimm","halbtoene":-6,"formant":-3,"bild":"datei:bild.png",
        "sounds":[{"name":"Lachen","datei":"sounds/0.wav","lautstaerke":0.8,"musik":false}]}"#;

    #[test]
    fn export_und_import_hin_und_zurueck() {
        let o = ordner("rund");
        let bilder = o.join("bilder");
        std::fs::create_dir_all(&bilder).unwrap();
        std::fs::write(bilder.join("abc.png"), [0x89, b'P', b'N', b'G', 1, 2]).unwrap();
        let ton = o.join("lachen.wav");
        std::fs::write(&ton, wav(500)).unwrap();
        let figur = FigurDef { name: "Grimm".into(), halbtoene: -6.0, formant: -3.0, bild: Some("datei:abc.png".into()), bild_ausschnitt: Some(Ausschnitt { x: 20.0, y: 70.0, zoom: 2.0 }), umgebung: Some(Umgebung::Hoehle), umgebung_staerke: Some(0.4), verfahren: Verfahren::Signalsmith, ki: None, ki_an: false, hell: 0.0 };
        let sound = Sound { name: "Lachen".into(), datei: ton, lautstaerke: 0.8, nochmal: NochmalWahl::Neu, chat: true, ..Default::default() };
        let zip = o.join("grimm.zip");
        exportieren(&figur, &[sound], &bilder, &zip).unwrap();

        let ziel_bilder = o.join("neu_bilder");
        let imp = importieren(&zip, &ziel_bilder, &o.join("neu_sounds"), Grenzen::default()).unwrap();
        assert_eq!(imp.figur.name, "Grimm");
        assert_eq!(imp.figur.halbtoene, -6.0);
        assert_eq!(imp.figur.umgebung, Some(Umgebung::Hoehle));
        assert_eq!(imp.figur.umgebung_staerke, Some(0.4));
        assert_eq!(imp.figur.verfahren, Verfahren::Signalsmith);
        assert_eq!(imp.figur.bild_ausschnitt, Some(Ausschnitt { x: 20.0, y: 70.0, zoom: 2.0 }));
        let b = imp.figur.bild.clone().unwrap();
        assert!(ziel_bilder.join(b.trim_start_matches("datei:")).exists(), "{b}");
        assert_eq!(imp.sounds.len(), 1);
        assert_eq!(imp.sounds[0].nochmal, NochmalWahl::Neu);
        assert!(imp.sounds[0].chat);
        assert_eq!(std::fs::read(&imp.sounds[0].datei).unwrap(), wav(500));

        let mut e = Einstellungen::default();
        e.figuren.push(FigurDef { name: "Grimm".into(), halbtoene: 0.0, formant: 0.0, bild: None, bild_ausschnitt: None, umgebung: None, umgebung_staerke: None, verfahren: Default::default(), ki: None, ki_an: false, hell: 0.0 });
        assert_eq!(einfuegen(&mut e, imp), "Grimm (2)");
        assert_eq!(e.sounds.len(), 1);
    }

    #[test]
    fn altes_paket_mit_glottix_namen_wird_gelesen() {
        let o = ordner("altname");
        let zip = o.join("alt.zip");
        zip_bauen(&zip, &[(JSON_NAME_ALT, JSON.as_bytes()), ("bild.png", &[0x89, b'P', b'N', b'G']), ("sounds/0.wav", &wav(64))]);
        let imp = importieren(&zip, &o.join("app").join("bilder"), &o.join("app").join("sounds"), Grenzen::default()).unwrap();
        assert_eq!(imp.uebersprungen, 0);
        assert!(erlaubt(JSON_NAME) && erlaubt(JSON_NAME_ALT));
    }

    #[test]
    fn zip_slip_wird_nicht_geschrieben() {
        let o = ordner("slip");
        let zip = o.join("boese.zip");
        let ziel = o.join("app");
        zip_bauen(&zip, &[(JSON_NAME, JSON.as_bytes()), ("bild.png", &[0x89, b'P', b'N', b'G']), ("sounds/0.wav", &wav(64)), ("../../boese.txt", b"x"), ("..\\boese2.txt", b"x"), ("C:/Windows/boese3.txt", b"x"), ("sounds/../../boese4.wav", &wav(64))]);
        let imp = importieren(&zip, &ziel.join("bilder"), &ziel.join("sounds"), Grenzen::default()).unwrap();
        assert_eq!(imp.uebersprungen, 4);
        assert!(!o.join("boese.txt").exists() && !std::env::temp_dir().join("boese.txt").exists());
        // Nur die erwarteten Dateien liegen im Ziel.
        let mut alle = Vec::new();
        for e in walk(&ziel) {
            alle.push(e.strip_prefix(&ziel).unwrap().to_string_lossy().replace('\\', "/"));
        }
        assert_eq!(alle.len(), 2, "{alle:?}");
        assert!(alle.iter().any(|p| p.starts_with("bilder/") && p.ends_with(".png")));
        assert!(alle.iter().any(|p| p.starts_with("sounds/") && p.ends_with("/0.wav")));
    }

    fn walk(p: &Path) -> Vec<PathBuf> {
        let mut v = Vec::new();
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                if e.path().is_dir() {
                    v.extend(walk(&e.path()));
                } else {
                    v.push(e.path());
                }
            }
        }
        v
    }

    #[test]
    fn zip_bombe_bricht_ab() {
        // 5 MB Nullen packen sich auf wenige KB. Grenze im Test: 1 MB je Sound.
        let o = ordner("bombe");
        let zip = o.join("bombe.zip");
        let mut riesig = wav(64);
        riesig.resize(5 * 1024 * 1024, 0);
        zip_bauen(&zip, &[(JSON_NAME, JSON.as_bytes()), ("bild.png", &[0x89, b'P', b'N', b'G']), ("sounds/0.wav", &riesig)]);
        assert!(std::fs::metadata(&zip).unwrap().len() < 100 * 1024);
        let g = Grenzen { sound: 1024 * 1024, ..Default::default() };
        let f = importieren(&zip, &o.join("b"), &o.join("s"), g).unwrap_err();
        assert!(f.contains("zu groß"), "{f}");
        // Nichts halb geschrieben.
        assert!(walk(&o.join("b")).is_empty() && walk(&o.join("s")).is_empty());
        // Gesamt-Grenze greift auch über mehrere Dateien.
        let g = Grenzen { gesamt: 100, ..Default::default() };
        assert!(importieren(&zip, &o.join("b"), &o.join("s"), g).is_err());
    }

    #[test]
    fn falsche_inhalte_werden_abgelehnt() {
        let o = ordner("falsch");
        // Ton-Datei ist in Wahrheit ein Programm.
        let zip = o.join("exe.zip");
        zip_bauen(&zip, &[(JSON_NAME, JSON.as_bytes()), ("bild.png", &[0x89, b'P', b'N', b'G']), ("sounds/0.wav", b"MZ\x90\0 das ist ein programm")]);
        let f = importieren(&zip, &o.join("b"), &o.join("s"), Grenzen::default()).unwrap_err();
        assert!(f.contains("keine Ton-Datei"), "{f}");
        // Kein ZIP.
        let kaputt = o.join("kaputt.zip");
        std::fs::write(&kaputt, b"kein zip").unwrap();
        assert!(importieren(&kaputt, &o.join("b"), &o.join("s"), Grenzen::default()).is_err());
        // ZIP ohne Figur.
        let leer = o.join("leer.zip");
        zip_bauen(&leer, &[("foto.png", &[0x89, b'P', b'N', b'G'])]);
        assert!(importieren(&leer, &o.join("b"), &o.join("s"), Grenzen::default()).unwrap_err().contains("fehlt"));
        // Werte werden begrenzt, Steuerzeichen entfernt.
        let wild = o.join("wild.zip");
        let j = r#"{"format":1,"name":"Bö\u0007se\nFigur","halbtoene":99,"formant":-50,"bild":"avatar:../x"}"#;
        zip_bauen(&wild, &[(JSON_NAME, j.as_bytes())]);
        let imp = importieren(&wild, &o.join("b"), &o.join("s"), Grenzen::default()).unwrap();
        assert_eq!(imp.figur.name, "BöseFigur");
        assert_eq!((imp.figur.halbtoene, imp.figur.formant), (30.0, -4.0));
        assert_eq!(imp.figur.bild, None);
    }

    #[test]
    fn erlaubte_namen() {
        for n in [JSON_NAME, "bild.png", "bild.webp", "sounds/0.mp3", "sounds/123.flac"] {
            assert!(erlaubt(n), "{n}");
        }
        for n in ["../glottix-figur.json", "bild.exe", "sounds/a.mp3", "sounds/0.mp3.exe", "sounds/../0.mp3", "sounds/12345.wav", "/bild.png", "Bild.PNG"] {
            assert!(!erlaubt(n), "{n}");
        }
    }
}
