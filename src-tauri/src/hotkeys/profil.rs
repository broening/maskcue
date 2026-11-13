//! Profile pro Spiel und die Entscheidung, welcher Hotkey was tut.
//!
//! Regeln (PLAN.md, Schritt 6):
//! - Das Profil wechselt nach dem Programm im Vordergrund.
//! - Ein Programm ohne Profil bekommt das Profil „Aus“: keine Sound-Hotkeys.
//! - Von Hand gewählt bleibt ein Profil, bis man „Automatisch“ wählt.
//! - Globale Hotkeys gelten in jedem Profil. Erlaubt sind dort nur
//!   „Alles stoppen“, „Stumm an/aus“ und der Profil-Wechsel. Sonst käme man
//!   aus „Aus“ nie heraus.
//! - Hält man Zusatz-Tasten, die der Hotkey nicht braucht (z. B. Umschalt
//!   zum Rennen), löst der Hotkey trotzdem aus. Passt eine Kombination
//!   genauer, gewinnt sie.

use serde::{Deserialize, Serialize};

use super::taste::Kombination;
use crate::audio::umgebung::Umgebung;

/// Name des Profils ohne Sound-Hotkeys.
pub const PROFIL_AUS: &str = "Aus";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Aktion {
    /// Sound abspielen (Name des Sounds). Nochmal drücken: je nach Sound Stopp oder Neustart.
    Sound(String),
    /// Notbremse: alle Sounds und Musik aus.
    AllesStopp,
    /// Figur-Stimme einschalten (Name der Figur).
    Figur(String),
    /// Normale Stimme.
    FigurAus,
    /// Profil von Hand wählen.
    Profil(String),
    /// Profil wieder nach dem Programm im Vordergrund wählen.
    ProfilAutomatisch,
    /// Eigene Stimme stumm schalten bzw. wieder an (Sounds laufen weiter).
    StummUmschalten,
    /// Umgebung an/aus (z. B. Funkgerät). Gilt auch bei normaler Stimme und
    /// geht vor der Umgebung der Figur.
    Umgebung(Umgebung),
    /// Jede Umgebung aus, auch die der Figur.
    UmgebungAus,
    /// Aufnahme starten bzw. stoppen (Aufnahme-Bereich).
    AufnahmeUmschalten,
}

impl Aktion {
    /// Darf als globaler Hotkey (in jedem Profil) gelten.
    pub fn global_erlaubt(&self) -> bool {
        matches!(self, Aktion::AllesStopp | Aktion::Profil(_) | Aktion::ProfilAutomatisch | Aktion::StummUmschalten | Aktion::AufnahmeUmschalten)
    }
}

impl std::fmt::Display for Aktion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Aktion::Sound(s) => write!(f, "Sound „{s}“"),
            Aktion::AllesStopp => write!(f, "Alles stoppen"),
            Aktion::Figur(s) => write!(f, "Figur „{s}“"),
            Aktion::FigurAus => write!(f, "Figur aus"),
            Aktion::Profil(s) => write!(f, "Profil „{s}“ (von Hand)"),
            Aktion::ProfilAutomatisch => write!(f, "Profil automatisch"),
            Aktion::StummUmschalten => write!(f, "Stumm an/aus"),
            Aktion::Umgebung(u) => write!(f, "Umgebung „{u}“"),
            Aktion::UmgebungAus => write!(f, "Umgebung aus"),
            Aktion::AufnahmeUmschalten => write!(f, "Aufnahme an/aus"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Belegung {
    pub taste: Kombination,
    pub aktion: Aktion,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profil {
    pub name: String,
    /// Programme, für die das Profil gilt, z. B. "FiveM*.exe". `*` steht für
    /// beliebig viele Zeichen, `?` für eins. Groß/klein egal.
    #[serde(default)]
    pub programme: Vec<String>,
    #[serde(default)]
    pub belegungen: Vec<Belegung>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HotkeyEinstellungen {
    /// Gilt in jedem Profil (nur „Alles stoppen“ und Profil-Wechsel).
    #[serde(default)]
    pub global: Vec<Belegung>,
    #[serde(default)]
    pub profile: Vec<Profil>,
    /// Profil für alle Programme ohne eigenes Profil (z. B. Windows-Desktop,
    /// zum Testen und Aufnehmen). Fehlt es, gilt „Aus“. Achtung: gilt auch
    /// für Spiele ohne eigenes Profil.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ohne_spiel: Option<String>,
}

/// Passt `name` auf `muster`? `*` = beliebig viele Zeichen, `?` = eins. Groß/klein egal.
pub fn passt(muster: &str, name: &str) -> bool {
    let m: Vec<char> = muster.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    // Klassisch mit Rücksprung zum letzten `*`.
    let (mut i, mut j) = (0, 0);
    let mut stern: Option<(usize, usize)> = None;
    while j < n.len() {
        if i < m.len() && (m[i] == '?' || m[i] == n[j]) {
            i += 1;
            j += 1;
        } else if i < m.len() && m[i] == '*' {
            stern = Some((i, j));
            i += 1;
        } else if let Some((si, sj)) = stern {
            i = si + 1;
            j = sj + 1;
            stern = Some((si, sj + 1));
        } else {
            return false;
        }
    }
    m[i..].iter().all(|&c| c == '*')
}

impl HotkeyEinstellungen {
    /// Erstes Profil, dessen Programm-Muster passt.
    pub fn profil_fuer(&self, programm: &str) -> Option<usize> {
        self.profile.iter().position(|p| p.programme.iter().any(|m| passt(m, programm)))
    }

    pub fn profil_nach_name(&self, name: &str) -> Option<usize> {
        self.profile.iter().position(|p| p.name.eq_ignore_ascii_case(name))
    }

    /// Warnungen für die Anzeige: doppelte Tasten, falsche globale Hotkeys,
    /// unbekannte Profile.
    pub fn pruefen(&self) -> Vec<String> {
        let mut w = Vec::new();
        for b in &self.global {
            if !b.aktion.global_erlaubt() {
                w.push(format!("Global {}: {} geht nur in einem Profil. Wird nicht ausgeführt.", b.taste, b.aktion));
            }
        }
        doppelte(&self.global, "Global", &mut w);
        for p in &self.profile {
            if p.name.eq_ignore_ascii_case(PROFIL_AUS) {
                w.push(format!("Profil „{}“: Der Name ist für das Profil ohne Sounds reserviert.", p.name));
            }
            doppelte(&p.belegungen, &format!("Profil „{}“", p.name), &mut w);
            for b in &p.belegungen {
                if self.global.iter().any(|g| g.taste == b.taste) {
                    w.push(format!("Profil „{}“: {} ist schon global belegt. Der globale Hotkey gewinnt.", p.name, b.taste));
                }
            }
        }
        for (i, p) in self.profile.iter().enumerate() {
            if self.profile[..i].iter().any(|q| q.name.eq_ignore_ascii_case(&p.name)) {
                w.push(format!("Profil „{}“ gibt es doppelt.", p.name));
            }
            for m in &p.programme {
                if let Some(q) = self.profile[..i].iter().find(|q| q.programme.iter().any(|n| n.eq_ignore_ascii_case(m))) {
                    w.push(format!("Programm {m} steht in „{}“ und „{}“. Es gilt „{}“.", q.name, p.name, q.name));
                }
            }
        }
        let alle = self.global.iter().chain(self.profile.iter().flat_map(|p| &p.belegungen));
        for b in alle {
            if let Aktion::Profil(n) = &b.aktion {
                if !n.eq_ignore_ascii_case(PROFIL_AUS) && self.profil_nach_name(n).is_none() {
                    w.push(format!("{}: Profil „{n}“ gibt es nicht.", b.taste));
                }
            }
        }
        w
    }
}

fn doppelte(liste: &[Belegung], wo: &str, w: &mut Vec<String>) {
    for (i, b) in liste.iter().enumerate() {
        if liste[..i].iter().any(|a| a.taste == b.taste) {
            w.push(format!("{wo}: {} ist doppelt belegt. Es gilt die erste Belegung.", b.taste));
        }
    }
}

/// Beste Belegung für die gedrückte Kombination: gleiche Taste, alle nötigen
/// Zusatz-Tasten gedrückt, die meisten passenden Zusatz-Tasten gewinnen.
fn beste<'a>(liste: impl Iterator<Item = &'a Belegung>, k: Kombination) -> Option<&'a Belegung> {
    let mut best: Option<&Belegung> = None;
    for b in liste {
        if b.taste.taste == k.taste && b.taste.mods.teil_von(k.mods) && best.is_none_or(|x| b.taste.mods.anzahl() > x.taste.mods.anzahl()) {
            best = Some(b);
        }
    }
    best
}

/// Aktives Profil und Auswahl der Aktionen. Ohne Windows, darum gut testbar.
pub struct Zustand {
    e: HotkeyEinstellungen,
    von_hand: Option<String>,
    programm: String,
    aktiv: Option<usize>,
}

impl Zustand {
    pub fn new(e: HotkeyEinstellungen) -> Self {
        Self { e, von_hand: None, programm: String::new(), aktiv: None }
    }

    pub fn einstellungen(&self) -> &HotkeyEinstellungen {
        &self.e
    }

    /// Neue Einstellungen. Gibt `true` zurück, wenn das aktive Profil wechselt.
    pub fn einstellungen_setzen(&mut self, e: HotkeyEinstellungen) -> bool {
        let alt = self.aktives_profil().to_string();
        self.e = e;
        self.neu_waehlen();
        alt != self.aktives_profil()
    }

    /// Programm im Vordergrund (Name der .exe). Gibt `true` zurück, wenn das
    /// aktive Profil wechselt.
    pub fn programm(&mut self, exe: &str) -> bool {
        if exe == self.programm {
            return false;
        }
        exe.clone_into(&mut self.programm);
        let alt = self.aktiv;
        self.neu_waehlen();
        alt != self.aktiv
    }

    pub fn programm_name(&self) -> &str {
        &self.programm
    }

    /// Profil von Hand wählen (`None` = automatisch). Gibt `true` zurück,
    /// wenn das aktive Profil oder die Art der Wahl wechselt.
    pub fn von_hand(&mut self, name: Option<&str>) -> bool {
        let vorher = (self.aktiv, self.von_hand.is_some());
        self.von_hand = name.map(str::to_string);
        self.neu_waehlen();
        vorher != (self.aktiv, self.von_hand.is_some())
    }

    pub fn ist_von_hand(&self) -> bool {
        self.von_hand.is_some()
    }

    /// Name des aktiven Profils ("Aus", wenn keins gilt).
    pub fn aktives_profil(&self) -> &str {
        self.aktiv.map_or(PROFIL_AUS, |i| self.e.profile[i].name.as_str())
    }

    fn neu_waehlen(&mut self) {
        self.aktiv = match &self.von_hand {
            Some(n) => self.e.profil_nach_name(n),
            None => self.e.profil_fuer(&self.programm).or_else(|| self.e.ohne_spiel.as_deref().and_then(|n| self.e.profil_nach_name(n))),
        };
    }

    /// Gedrückte Kombination. Gibt die Aktion zurück, wenn ein Hotkey passt.
    /// Profil-Aktionen wirken sofort auf den Zustand.
    pub fn taste(&mut self, k: Kombination) -> Option<Aktion> {
        let global = beste(self.e.global.iter().filter(|b| b.aktion.global_erlaubt()), k);
        let profil = self.aktiv.and_then(|i| beste(self.e.profile[i].belegungen.iter(), k));
        // Gleich genau: global gewinnt (Notbremse geht immer).
        let b = match (global, profil) {
            (Some(g), Some(p)) => Some(if p.taste.mods.anzahl() > g.taste.mods.anzahl() { p } else { g }),
            (g, p) => g.or(p),
        }?;
        let aktion = b.aktion.clone();
        match &aktion {
            Aktion::Profil(n) => {
                self.von_hand(Some(n));
            }
            Aktion::ProfilAutomatisch => {
                self.von_hand(None);
            }
            _ => {}
        }
        Some(aktion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(s: &str) -> Kombination {
        s.parse().unwrap()
    }
    fn b(t: &str, a: Aktion) -> Belegung {
        Belegung { taste: k(t), aktion: a }
    }
    fn sound(s: &str) -> Aktion {
        Aktion::Sound(s.into())
    }

    fn beispiel() -> HotkeyEinstellungen {
        HotkeyEinstellungen {
            ohne_spiel: None,
            global: vec![
                b("Strg+Alt+S", Aktion::AllesStopp),
                b("Strg+Alt+A", Aktion::ProfilAutomatisch),
                b("Strg+Alt+F", Aktion::Profil("FiveM".into())),
                b("Strg+Alt+0", Aktion::Profil(PROFIL_AUS.into())),
            ],
            profile: vec![
                Profil {
                    name: "FiveM".into(),
                    programme: vec!["FiveM*.exe".into()],
                    belegungen: vec![
                        b("Num1", sound("Wind.wav")),
                        b("Umschalt+Num1", sound("Engel Segen.wav")),
                        b("Maus4", Aktion::Figur("Opa".into())),
                        b("Maus5", Aktion::FigurAus),
                    ],
                },
                Profil { name: "RedM".into(), programme: vec!["RedM*.exe".into()], belegungen: vec![b("Num1", sound("Kirchenglocken.wav"))] },
            ],
        }
    }

    #[test]
    fn muster() {
        assert!(passt("FiveM*.exe", "FiveM_b3095_GTAProcess.exe"));
        assert!(passt("fivem*.exe", "FiveM.exe"));
        assert!(passt("cs2.exe", "CS2.EXE"));
        assert!(passt("*", "egal.exe"));
        assert!(passt("Red?.exe", "RedM.exe"));
        assert!(passt("*a*b*.exe", "xxaxxbxx.exe"));
        assert!(!passt("FiveM*.exe", "FiveMx.dll"));
        assert!(!passt("cs2.exe", "cs2.exe.bak"));
        assert!(!passt("Red?.exe", "Red.exe"));
        assert!(!passt("", "a"));
    }

    #[test]
    fn profil_wechselt_nach_programm() {
        let mut z = Zustand::new(beispiel());
        assert_eq!(z.aktives_profil(), PROFIL_AUS);
        assert!(z.programm("FiveM_b3095_GTAProcess.exe"));
        assert_eq!(z.aktives_profil(), "FiveM");
        assert_eq!(z.taste(k("Num1")), Some(sound("Wind.wav")));
        assert!(z.programm("RedM.exe"));
        assert_eq!(z.taste(k("Num1")), Some(sound("Kirchenglocken.wav")));
        // Anderes Programm mit gleichem Profil: kein Wechsel.
        assert!(z.programm("RedM_GameProcess.exe") == false);
    }

    #[test]
    fn shooter_ohne_profil_spielt_keinen_sound() {
        let mut z = Zustand::new(beispiel());
        z.programm("FiveM.exe");
        assert!(z.programm("cs2.exe"));
        assert_eq!(z.aktives_profil(), PROFIL_AUS);
        for t in ["Num1", "Umschalt+Num1", "Maus4", "Maus5"] {
            assert_eq!(z.taste(k(t)), None, "{t}");
        }
        // Notbremse geht trotzdem.
        assert_eq!(z.taste(k("Strg+Alt+S")), Some(Aktion::AllesStopp));
    }

    #[test]
    fn zusatz_tasten() {
        let mut z = Zustand::new(beispiel());
        z.programm("FiveM.exe");
        // Genau belegt: die genaue Belegung gewinnt.
        assert_eq!(z.taste(k("Umschalt+Num1")), Some(sound("Engel Segen.wav")));
        // Strg gehalten (z. B. Ducken): Num1 löst trotzdem aus.
        assert_eq!(z.taste(k("Strg+Num1")), Some(sound("Wind.wav")));
        assert_eq!(z.taste(k("Strg+Umschalt+Num1")), Some(sound("Engel Segen.wav")));
        // Fehlt eine nötige Zusatz-Taste, passiert nichts.
        assert_eq!(z.taste(k("Alt+S")), None);
        assert_eq!(z.taste(k("Num2")), None);
    }

    #[test]
    fn von_hand_bleibt_bis_automatisch() {
        let mut z = Zustand::new(beispiel());
        z.programm("cs2.exe");
        assert_eq!(z.taste(k("Strg+Alt+F")), Some(Aktion::Profil("FiveM".into())));
        assert_eq!(z.aktives_profil(), "FiveM");
        assert!(z.ist_von_hand());
        // Programm wechselt: Profil bleibt.
        assert!(!z.programm("RedM.exe"));
        assert_eq!(z.aktives_profil(), "FiveM");
        // Zurück auf automatisch: RedM, weil RedM vorn ist.
        z.taste(k("Strg+Alt+A"));
        assert!(!z.ist_von_hand());
        assert_eq!(z.aktives_profil(), "RedM");
        // Von Hand auf „Aus“.
        z.taste(k("Strg+Alt+0"));
        assert_eq!(z.aktives_profil(), PROFIL_AUS);
        assert_eq!(z.taste(k("Num1")), None);
    }

    #[test]
    fn global_gewinnt_bei_gleicher_taste() {
        let mut e = beispiel();
        e.profile[0].belegungen.push(b("Strg+Alt+S", sound("Wind.wav")));
        let mut z = Zustand::new(e);
        z.programm("FiveM.exe");
        assert_eq!(z.taste(k("Strg+Alt+S")), Some(Aktion::AllesStopp));
    }

    #[test]
    fn sound_als_globaler_hotkey_wird_ignoriert() {
        let mut e = beispiel();
        e.global.push(b("F9", sound("Wind.wav")));
        let w = e.pruefen();
        assert!(w.iter().any(|t| t.contains("F9")), "{w:?}");
        let mut z = Zustand::new(e);
        z.programm("cs2.exe");
        assert_eq!(z.taste(k("F9")), None);
    }

    #[test]
    fn pruefen_findet_probleme() {
        assert!(beispiel().pruefen().is_empty(), "{:?}", beispiel().pruefen());
        let mut e = beispiel();
        e.profile[0].belegungen.push(b("Num1", sound("Wind.wav")));
        e.profile[0].belegungen.push(b("Strg+Alt+S", sound("Wind.wav")));
        e.profile[1].programme.push("fivem*.exe".into());
        e.profile.push(Profil { name: "aus".into(), programme: vec![], belegungen: vec![] });
        e.global.push(b("F8", Aktion::Profil("Gibtsnicht".into())));
        let w = e.pruefen();
        assert_eq!(w.len(), 5, "{w:#?}");
    }

    #[test]
    fn json_hin_und_zurueck() {
        let e = beispiel();
        let j = serde_json::to_string_pretty(&e).unwrap();
        assert!(j.contains("\"Strg+Alt+S\""));
        assert_eq!(serde_json::from_str::<HotkeyEinstellungen>(&j).unwrap(), e);
    }

    #[test]
    fn profil_ohne_spiel_fuer_den_desktop() {
        let mut e = beispiel();
        let mut z = Zustand::new(e.clone());
        assert!(!z.programm("explorer.exe"));
        assert_eq!(z.aktives_profil(), PROFIL_AUS);
        // Mit Einstellung: Desktop bekommt FiveM, ein Spiel mit eigenem Profil bleibt bei seinem.
        e.ohne_spiel = Some("FiveM".into());
        assert!(z.einstellungen_setzen(e));
        assert_eq!(z.aktives_profil(), "FiveM");
        assert_eq!(z.taste(k("Num1")), Some(sound("Wind.wav")));
        // Unbekannter Name: wieder „Aus“.
        let mut e2 = beispiel();
        e2.ohne_spiel = Some("Gibtsnicht".into());
        z.einstellungen_setzen(e2);
        assert_eq!(z.aktives_profil(), PROFIL_AUS);
    }
}
