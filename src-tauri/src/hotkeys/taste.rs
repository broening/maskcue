//! Tasten und Tasten-Kombinationen, als Text lesbar ("Strg+Alt+F1", "Maus4").
//!
//! Tastatur-Tasten sind Windows-Tastencodes (Virtual-Key). Die Namen der
//! Sonder-Tasten (Ü, Ö, Ä, ß, …) gelten für die deutsche Tastatur.
//! Unbekannte Tasten heißen "VK123".

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MausTaste {
    /// Mausrad drücken.
    Mitte,
    /// Seitentaste hinten (meist „Zurück“).
    Seite1,
    /// Seitentaste vorn (meist „Vor“).
    Seite2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Taste {
    /// Windows-Tastencode (VK).
    Tastatur(u8),
    Maus(MausTaste),
}

/// Zusatz-Tasten, die gedrückt sein müssen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods(pub u8);

impl Mods {
    pub const STRG: Mods = Mods(1);
    pub const ALT: Mods = Mods(2);
    pub const UMSCHALT: Mods = Mods(4);
    pub const WIN: Mods = Mods(8);

    pub fn hat(self, m: Mods) -> bool {
        self.0 & m.0 == m.0
    }
    /// Alle Zusatz-Tasten von `self` sind auch in `andere` gedrückt.
    pub fn teil_von(self, andere: Mods) -> bool {
        andere.hat(self)
    }
    pub fn anzahl(self) -> u32 {
        self.0.count_ones()
    }
}

impl std::ops::BitOr for Mods {
    type Output = Mods;
    fn bitor(self, o: Mods) -> Mods {
        Mods(self.0 | o.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Kombination {
    pub mods: Mods,
    pub taste: Taste,
}

impl Kombination {
    pub fn neu(mods: Mods, taste: Taste) -> Self {
        Self { mods, taste }
    }
}

/// Strg, Alt, Umschalt, Win (links, rechts und allgemein). Sie sind nie
/// selbst die Haupt-Taste einer Kombination.
pub fn ist_zusatz(vk: u8) -> bool {
    matches!(vk, 0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA0..=0xA5)
}

/// Namen fester Tasten. Buchstaben, Ziffern, F-Tasten und Ziffernblock
/// stehen in [`vk_name`].
const NAMEN: &[(u8, &str)] = &[
    (0x08, "Rück"),
    (0x09, "Tab"),
    (0x0D, "Enter"),
    (0x13, "Pause"),
    (0x14, "Feststell"),
    (0x1B, "Esc"),
    (0x20, "Leertaste"),
    (0x21, "BildAuf"),
    (0x22, "BildAb"),
    (0x23, "Ende"),
    (0x24, "Pos1"),
    (0x25, "Links"),
    (0x26, "Hoch"),
    (0x27, "Rechts"),
    (0x28, "Runter"),
    (0x2C, "Druck"),
    (0x2D, "Einfg"),
    (0x2E, "Entf"),
    (0x5D, "Menü"),
    (0x6A, "Num*"),
    (0x6B, "Num+"),
    (0x6D, "Num-"),
    (0x6E, "NumKomma"),
    (0x6F, "Num/"),
    (0x90, "NumLock"),
    (0x91, "Rollen"),
    (0xAD, "Stumm"),
    (0xAE, "Leiser"),
    (0xAF, "Lauter"),
    (0xB0, "MedienWeiter"),
    (0xB1, "MedienZurück"),
    (0xB2, "MedienStopp"),
    (0xB3, "MedienPlay"),
    (0xBA, "Ü"),
    (0xBB, "Plus"),
    (0xBC, "Komma"),
    (0xBD, "Minus"),
    (0xBE, "Punkt"),
    (0xBF, "Raute"),
    (0xC0, "Ö"),
    (0xDB, "ß"),
    (0xDC, "Zirkumflex"),
    (0xDD, "Akzent"),
    (0xDE, "Ä"),
    (0xE2, "Kleiner"),
];

/// Name einer Kombination aus Windows-Tastencode und Zusatz-Bits
/// (Strg = 1, Alt = 2, Umschalt = 4, Win = 8), z. B. "Strg+K".
pub fn name_von_vk(vk: u8, mods: u8) -> Result<String, String> {
    if vk == 0 || vk == 0xFF || ist_zusatz(vk) {
        return Err(format!("Taste {vk} geht nicht als Hotkey"));
    }
    Ok(Kombination::neu(Mods(mods & 0x0F), Taste::Tastatur(vk)).to_string())
}

fn vk_name(vk: u8) -> String {
    match vk {
        b'0'..=b'9' | b'A'..=b'Z' => (vk as char).to_string(),
        0x60..=0x69 => format!("Num{}", vk - 0x60),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        _ => NAMEN.iter().find(|(v, _)| *v == vk).map_or_else(|| format!("VK{vk}"), |(_, n)| n.to_string()),
    }
}

fn vk_aus_name(name: &str) -> Option<u8> {
    let gross = name.to_uppercase();
    if gross.chars().count() == 1 {
        let c = gross.chars().next()?;
        if c.is_ascii_digit() || c.is_ascii_uppercase() {
            return Some(c as u8);
        }
    }
    if let Some(n) = gross.strip_prefix("NUM").and_then(|r| r.parse::<u8>().ok()) {
        return (n <= 9).then_some(0x60 + n);
    }
    if let Some(n) = gross.strip_prefix('F').and_then(|r| r.parse::<u8>().ok()) {
        return (1..=24).contains(&n).then_some(0x6F + n);
    }
    if let Some(n) = gross.strip_prefix("VK").and_then(|r| r.parse::<u8>().ok()) {
        return (n > 0 && !ist_zusatz(n)).then_some(n);
    }
    NAMEN.iter().find(|(_, n)| n.to_uppercase() == gross).map(|(v, _)| *v)
}

impl fmt::Display for Taste {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Taste::Tastatur(vk) => f.write_str(&vk_name(*vk)),
            Taste::Maus(MausTaste::Mitte) => f.write_str("Maus3"),
            Taste::Maus(MausTaste::Seite1) => f.write_str("Maus4"),
            Taste::Maus(MausTaste::Seite2) => f.write_str("Maus5"),
        }
    }
}

impl FromStr for Taste {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let s = s.trim();
        match s.to_uppercase().as_str() {
            "MAUS3" | "MAUSMITTE" => return Ok(Taste::Maus(MausTaste::Mitte)),
            "MAUS4" => return Ok(Taste::Maus(MausTaste::Seite1)),
            "MAUS5" => return Ok(Taste::Maus(MausTaste::Seite2)),
            "MAUS1" | "MAUS2" => return Err(format!("\"{s}\": linke und rechte Maustaste gehen nicht als Hotkey")),
            _ => {}
        }
        vk_aus_name(s).map(Taste::Tastatur).ok_or_else(|| format!("unbekannte Taste \"{s}\""))
    }
}

const ZUSATZ_NAMEN: &[(Mods, &[&str])] = &[
    (Mods::STRG, &["STRG", "CTRL"]),
    (Mods::ALT, &["ALT"]),
    (Mods::UMSCHALT, &["UMSCHALT", "SHIFT"]),
    (Mods::WIN, &["WIN"]),
];

impl fmt::Display for Kombination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (m, name) in [(Mods::STRG, "Strg"), (Mods::ALT, "Alt"), (Mods::UMSCHALT, "Umschalt"), (Mods::WIN, "Win")] {
            if self.mods.hat(m) {
                write!(f, "{name}+")?;
            }
        }
        write!(f, "{}", self.taste)
    }
}

impl FromStr for Kombination {
    type Err = String;
    /// Zusatz-Tasten von links abschneiden, der Rest ist die Taste. So geht
    /// auch "Strg+Num+".
    fn from_str(s: &str) -> Result<Self, String> {
        // Tasten-Namen haben keine Leerzeichen: "Strg + F5" = "Strg+F5".
        let ohne: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        let mut rest = ohne.as_str();
        let mut mods = Mods::default();
        'suche: loop {
            for (m, namen) in ZUSATZ_NAMEN {
                for n in *namen {
                    let laenge = n.len() + 1;
                    if rest.len() > laenge && rest.is_char_boundary(laenge) && rest[..laenge].to_uppercase() == format!("{n}+") {
                        mods = mods | *m;
                        rest = &rest[laenge..];
                        continue 'suche;
                    }
                }
            }
            break;
        }
        let taste = rest.parse::<Taste>().map_err(|e| format!("Hotkey \"{s}\": {e}"))?;
        Ok(Kombination { mods, taste })
    }
}

impl Serialize for Kombination {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Kombination {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(s: &str) -> Kombination {
        s.parse().unwrap()
    }

    #[test]
    fn text_hin_und_zurueck() {
        for s in ["A", "7", "F1", "F24", "Num0", "Num9", "Num+", "Strg+Num+", "Strg+Alt+Umschalt+Win+Entf", "Maus4", "Alt+Maus5", "Ü", "Umschalt+ß", "VK255"] {
            assert_eq!(k(s).to_string(), s);
        }
    }

    #[test]
    fn gross_klein_und_englisch() {
        assert_eq!(k("ctrl+shift+a"), k("Strg+Umschalt+A"));
        assert_eq!(k("strg + f5"), Kombination::neu(Mods::STRG, Taste::Tastatur(0x74)));
        assert_eq!(k("num1").taste, Taste::Tastatur(0x61));
    }

    #[test]
    fn falsche_eingaben() {
        for s in ["", "Strg+", "Strg", "Maus1", "Hallo", "F25", "Num10", "VK17"] {
            assert!(s.parse::<Kombination>().is_err(), "{s} sollte nicht gehen");
        }
    }

    #[test]
    fn json() {
        let j = serde_json::to_string(&k("Strg+Maus4")).unwrap();
        assert_eq!(j, "\"Strg+Maus4\"");
        assert_eq!(serde_json::from_str::<Kombination>(&j).unwrap(), k("Strg+Maus4"));
        assert!(serde_json::from_str::<Kombination>("\"Maus1\"").is_err());
    }

    #[test]
    fn name_aus_tastencode_der_oberflaeche() {
        assert_eq!(name_von_vk(0x4B, 0).unwrap(), "K");
        assert_eq!(name_von_vk(0x77, 1 | 4).unwrap(), "Strg+Umschalt+F8");
        assert_eq!(name_von_vk(0x61, 0).unwrap(), "Num1");
        assert_eq!(name_von_vk(0xDE, 2).unwrap(), "Alt+Ä");
        assert!(name_von_vk(0x11, 0).is_err(), "Strg allein ist kein Hotkey");
        // Der Name lässt sich wieder lesen.
        let k: Kombination = name_von_vk(0x7C, 8).unwrap().parse().unwrap();
        assert_eq!(k.to_string(), "Win+F13");
    }
}
