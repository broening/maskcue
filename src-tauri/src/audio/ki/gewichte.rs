//! Modell-Ordner von Beatrice 2 lesen („paraphernalia“).
//!
//! Der Beatrice-Trainer (MIT-Lizenz, Project Beatrice) schreibt die Gewichte
//! als rohe fp16-Zahlen hintereinander, ohne Kopf. Die Reihenfolge legt der
//! Trainer fest (`dump()`), darum liest jeder Baustein genau so viele Zahlen,
//! wie er braucht. Am Ende muss jede Datei ganz gelesen sein, sonst passt der
//! Aufbau nicht.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// fp16 → f32 (IEEE 754 half).
pub fn f16_zu_f32(h: u16) -> f32 {
    let vorzeichen = ((h >> 15) as u32) << 31;
    let exp = ((h >> 10) & 0x1f) as u32;
    let mant = (h & 0x3ff) as u32;
    let bits = match exp {
        0 if mant == 0 => vorzeichen,
        0 => {
            // Subnormal: mant · 2^-24.
            let v = mant as f32 * (1.0 / 16_777_216.0);
            return if vorzeichen != 0 { -v } else { v };
        }
        31 => vorzeichen | 0x7f80_0000 | (mant << 13),
        _ => vorzeichen | ((exp + 127 - 15) << 23) | (mant << 13),
    };
    f32::from_bits(bits)
}

/// Liest eine Gewichts-Datei Stück für Stück.
pub struct Leser {
    name: String,
    roh: Vec<u16>,
    pos: usize,
}

impl Leser {
    pub fn datei(pfad: &Path) -> Result<Self, String> {
        let name = pfad.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let bytes = std::fs::read(pfad).map_err(|e| format!("{name}: {e}"))?;
        if bytes.len() % 2 != 0 {
            return Err(format!("{name}: Länge ist ungerade"));
        }
        let roh = bytes.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
        Ok(Self { name, roh, pos: 0 })
    }

    /// Die nächsten `n` Zahlen als f32.
    pub fn nimm(&mut self, n: usize) -> Result<Vec<f32>, String> {
        Ok(self.nimm_roh(n)?.iter().map(|&h| f16_zu_f32(h)).collect())
    }

    /// Die nächsten `n` Zahlen als fp16 (spart Speicher bei großen Tabellen).
    pub fn nimm_roh(&mut self, n: usize) -> Result<&[u16], String> {
        let ende = self.pos + n;
        if ende > self.roh.len() {
            return Err(format!("{}: zu kurz (Aufbau passt nicht zu dieser Version)", self.name));
        }
        let s = &self.roh[self.pos..ende];
        self.pos = ende;
        Ok(s)
    }

    pub fn fertig(&self) -> Result<(), String> {
        if self.pos != self.roh.len() {
            return Err(format!("{}: {} Zahlen übrig (Aufbau passt nicht zu dieser Version)", self.name, self.roh.len() - self.pos));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct TomlDatei {
    model: TomlModell,
    #[serde(default)]
    voice: std::collections::BTreeMap<String, TomlStimme>,
}

#[derive(Debug, Deserialize)]
struct TomlModell {
    version: String,
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct TomlStimme {
    #[serde(default)]
    name: String,
    /// Mittlere Tonhöhe als MIDI-Note (69 = 440 Hz).
    #[serde(default)]
    average_pitch: f32,
}

/// Eine Stimme im Modell.
#[derive(Clone, Debug, PartialEq)]
pub struct StimmInfo {
    pub nummer: usize,
    pub name: String,
    pub mittlere_note: f32,
}

/// Beschreibung eines Modell-Ordners.
#[derive(Clone, Debug)]
pub struct ModellInfo {
    pub ordner: PathBuf,
    pub name: String,
    pub version: String,
    pub stimmen: Vec<StimmInfo>,
}

/// Versionen des Trainers, deren Aufbau MaskCue kennt.
const VERSIONEN: &[&str] = &["2.0.0-rc.0"];

/// Liest `beatrice_paraphernalia_*.toml` im Ordner.
pub fn info(ordner: &Path) -> Result<ModellInfo, String> {
    let toml_pfad = std::fs::read_dir(ordner)
        .map_err(|e| format!("{}: {e}", ordner.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| {
            let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            n.starts_with("beatrice_paraphernalia") && n.ends_with(".toml")
        })
        .ok_or("Keine beatrice_paraphernalia_*.toml im Ordner")?;
    let text = std::fs::read_to_string(&toml_pfad).map_err(|e| e.to_string())?;
    let t: TomlDatei = toml::from_str(&text).map_err(|e| format!("Modell-Beschreibung: {e}"))?;
    if !VERSIONEN.contains(&t.model.version.as_str()) {
        return Err(format!("Modell-Version {} kennt MaskCue nicht (bekannt: {})", t.model.version, VERSIONEN.join(", ")));
    }
    let mut stimmen: Vec<StimmInfo> = t
        .voice
        .into_iter()
        .filter_map(|(k, v)| Some(StimmInfo { nummer: k.parse().ok()?, name: v.name, mittlere_note: v.average_pitch }))
        .collect();
    stimmen.sort_by_key(|s| s.nummer);
    Ok(ModellInfo { ordner: ordner.to_path_buf(), name: t.model.name, version: t.model.version, stimmen })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f16_werte() {
        assert_eq!(f16_zu_f32(0x3c00), 1.0);
        assert_eq!(f16_zu_f32(0xc000), -2.0);
        assert_eq!(f16_zu_f32(0x0000), 0.0);
        assert_eq!(f16_zu_f32(0x7bff), 65504.0);
        assert_eq!(f16_zu_f32(0x0001), 2f32.powi(-24));
        assert_eq!(f16_zu_f32(0x3555), 0.333_251_95);
        assert!(f16_zu_f32(0x7c00).is_infinite());
    }
}
