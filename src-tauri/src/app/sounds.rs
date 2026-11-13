//! Hält die geladenen Sound-Dateien und gleicht sie mit dem Soundboard ab.
//!
//! Laden dauert (eine 6-Minuten-WAV ca. 1 s). Darum lädt ein eigener Faden,
//! und das Ergebnis kommt später über [`SoundVerwaltung::geladen`] zurück.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use crate::audio::sounddatei::{self, SoundDaten};
use crate::audio::soundboard::{SoundEinstellung, Soundboard};

use super::einstellungen::Sound;

struct ImBoard {
    id: u32,
    datei: PathBuf,
    e: SoundEinstellung,
}

#[derive(Default)]
pub struct SoundVerwaltung {
    daten: HashMap<PathBuf, Result<Arc<SoundDaten>, String>>,
    laedt: HashSet<PathBuf>,
    board: HashMap<String, ImBoard>,
}

impl SoundVerwaltung {
    /// Neues Soundboard (nach Neustart des Tonwegs): alle neu eintragen.
    pub fn board_neu(&mut self) {
        self.board.clear();
    }

    /// Gleicht das Soundboard mit der Liste ab. Gibt Dateien zurück, die
    /// geladen werden müssen (der Aufrufer startet das Laden).
    pub fn abgleichen(&mut self, sounds: &[Sound], sb: Option<&mut Soundboard>) -> Vec<PathBuf> {
        let mut zu_laden = Vec::new();
        for s in sounds {
            if !self.daten.contains_key(&s.datei) && self.laedt.insert(s.datei.clone()) {
                zu_laden.push(s.datei.clone());
            }
        }
        // Daten ohne Sound freigeben.
        let benutzt: HashSet<&PathBuf> = sounds.iter().map(|s| &s.datei).collect();
        self.daten.retain(|p, _| benutzt.contains(p));

        let Some(sb) = sb else { return zu_laden };
        // Entfernte oder geänderte Sounds raus.
        let weg: Vec<String> = self
            .board
            .iter()
            .filter(|(name, b)| !sounds.iter().any(|s| &s.name == *name && s.datei == b.datei))
            .map(|(n, _)| n.clone())
            .collect();
        for n in weg {
            if let Some(b) = self.board.remove(&n) {
                sb.entfernen(b.id);
            }
        }
        for s in sounds {
            let e = s.einstellung();
            match (self.board.get_mut(&s.name), self.daten.get(&s.datei)) {
                (Some(b), _) => {
                    // Nur bei Änderung: Sonst läuft die Befehls-Schlange beim Schieben voll.
                    if b.e != e {
                        sb.einstellen(b.id, e);
                        b.e = e;
                    }
                }
                (None, Some(Ok(d))) => {
                    let id = sb.hinzufuegen_geteilt(d.clone(), e);
                    self.board.insert(s.name.clone(), ImBoard { id, datei: s.datei.clone(), e });
                }
                _ => {}
            }
        }
        zu_laden
    }

    /// Ergebnis des Ladens. Danach [`Self::abgleichen`] aufrufen.
    pub fn geladen(&mut self, datei: PathBuf, ergebnis: Result<Arc<SoundDaten>, String>) {
        self.laedt.remove(&datei);
        self.daten.insert(datei, ergebnis);
    }

    pub fn id(&self, name: &str) -> Option<u32> {
        self.board.get(name).map(|b| b.id)
    }

    pub fn name(&self, id: u32) -> Option<&str> {
        self.board.iter().find(|(_, b)| b.id == id).map(|(n, _)| n.as_str())
    }

    pub fn laedt(&self) -> usize {
        self.laedt.len()
    }

    /// Sounds, deren Datei nicht ging: (Name, Fehler).
    pub fn fehler(&self, sounds: &[Sound]) -> Vec<(String, String)> {
        sounds
            .iter()
            .filter_map(|s| match self.daten.get(&s.datei) {
                Some(Err(f)) => Some((s.name.clone(), f.clone())),
                _ => None,
            })
            .collect()
    }
}

/// Lädt eine Datei (im eigenen Faden aufrufen).
pub fn laden(datei: &std::path::Path) -> Result<Arc<SoundDaten>, String> {
    sounddatei::laden(datei).map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::soundboard::mischer;

    fn daten() -> Arc<SoundDaten> {
        Arc::new(SoundDaten { name: "t".into(), kanaele: 1, samples: vec![0.1; 4800] })
    }

    fn sound(name: &str, datei: &str) -> Sound {
        Sound { name: name.into(), datei: datei.into(), ..Default::default() }
    }

    #[test]
    fn laden_eintragen_aendern_entfernen() {
        let (_m, st) = mischer();
        let mut sb = Soundboard::new(vec![st]);
        let mut v = SoundVerwaltung::default();
        let mut liste = vec![sound("Wind", "a.wav"), sound("Glocke", "b.wav")];

        // Erst laden, noch nichts im Board.
        assert_eq!(v.abgleichen(&liste, Some(&mut sb)).len(), 2);
        assert_eq!(v.id("Wind"), None);
        // Nochmal abgleichen startet kein zweites Laden.
        assert!(v.abgleichen(&liste, Some(&mut sb)).is_empty());
        v.geladen("a.wav".into(), Ok(daten()));
        v.geladen("b.wav".into(), Err("kaputt".into()));
        v.abgleichen(&liste, Some(&mut sb));
        let id = v.id("Wind").expect("im Board");
        assert_eq!(v.id("Glocke"), None);
        assert_eq!(v.fehler(&liste), vec![("Glocke".to_string(), "kaputt".to_string())]);

        // Umbenennen: neue Nummer, gleiche Daten, kein neues Laden.
        liste[0].name = "Sturm".into();
        assert!(v.abgleichen(&liste, Some(&mut sb)).is_empty());
        assert_eq!(v.id("Wind"), None);
        assert!(v.id("Sturm").is_some_and(|n| n != id));

        // Entfernen gibt die Daten frei.
        liste.remove(0);
        v.abgleichen(&liste, Some(&mut sb));
        assert_eq!(v.id("Sturm"), None);
        assert!(!v.daten.contains_key(&PathBuf::from("a.wav")));
    }

    #[test]
    fn neues_board_traegt_alles_neu_ein() {
        let liste = vec![sound("Wind", "a.wav")];
        let mut v = SoundVerwaltung::default();
        v.abgleichen(&liste, None);
        v.geladen("a.wav".into(), Ok(daten()));
        let (_m1, st1) = mischer();
        let mut sb1 = Soundboard::new(vec![st1]);
        v.abgleichen(&liste, Some(&mut sb1));
        assert!(v.id("Wind").is_some());
        v.board_neu();
        let (_m2, st2) = mischer();
        let mut sb2 = Soundboard::new(vec![st2]);
        v.abgleichen(&liste, Some(&mut sb2));
        assert!(v.id("Wind").is_some());
    }
}
