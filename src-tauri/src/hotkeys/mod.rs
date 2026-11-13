//! Globale Hotkeys mit Profilen pro Spiel.
//!
//! Zwei Fäden:
//! - Hook-Faden ([`windows::Hook`]): sieht jede gedrückte Taste, auch im Spiel,
//!   und reicht sie nur weiter.
//! - Logik-Faden: prüft das Programm im Vordergrund, wählt das Profil und
//!   meldet Aktionen. Er prüft das Programm bei **jeder** Taste neu. So
//!   gilt nach Alt+Tab sofort das richtige Profil.

pub mod profil;
pub mod taste;
pub mod windows;

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use profil::{Aktion, HotkeyEinstellungen, Zustand};
use taste::{Kombination, Taste};

/// Was die Hotkeys melden.
#[derive(Clone, Debug, PartialEq)]
pub enum Ereignis {
    /// Ein Hotkey hat ausgelöst.
    Aktion { aktion: Aktion, taste: Kombination, profil: String },
    /// Das aktive Profil hat gewechselt (oder die Art: von Hand / automatisch).
    Profil {
        name: String,
        von_hand: bool,
        programm: String,
        /// Programm läuft als Administrator: Dann sieht MaskCue dort keine
        /// Tasten, außer MaskCue läuft auch als Administrator.
        admin_problem: bool,
    },
    /// Nur im Lern-Modus: jede gedrückte Kombination (für das Hotkey-Feld).
    Gedrueckt(Kombination),
    Fehler(String),
}

enum Nachricht {
    Taste(Kombination),
    /// Taste aus der Oberfläche: Ist MaskCue vorne, sieht der Hook die
    /// Tastatur nicht sicher (das WebView fängt sie ab).
    TasteOberflaeche(Kombination),
    Einstellungen(HotkeyEinstellungen),
    VonHand(Option<String>),
    Lernen(bool),
    Ende,
}

/// Meldet der Hook und die Oberfläche dieselbe Taste so kurz nacheinander,
/// zählt sie nur einmal.
const DOPPELT: Duration = Duration::from_millis(250);

/// So oft prüft der Logik-Faden das Programm im Vordergrund auch ohne Taste
/// (für die Anzeige des Profils).
const PRUEFEN: Duration = Duration::from_millis(500);

/// Läuft, solange der Wert lebt.
pub struct Hotkeys {
    tx: Sender<Nachricht>,
    faden: Option<JoinHandle<()>>,
}

fn braucht_maus(e: &HotkeyEinstellungen) -> bool {
    e.global.iter().chain(e.profile.iter().flat_map(|p| &p.belegungen)).any(|b| matches!(b.taste.taste, Taste::Maus(_)))
}

fn hook_starten(maus: bool, tx: &Sender<Nachricht>) -> Result<windows::Hook, String> {
    let tx = tx.clone();
    windows::Hook::start(maus, move |k| {
        let _ = tx.send(Nachricht::Taste(k));
    })
}

struct Logik {
    zustand: Zustand,
    eigene_pid: u32,
    selbst_admin: bool,
    letzte_pid: u32,
    admin_problem: bool,
    lernen: bool,
    /// Letzte Taste: Kombination, Zeit, kam aus der Oberfläche.
    zuletzt: Option<(Kombination, Instant, bool)>,
}

impl Logik {
    /// Kam dieselbe Taste gerade schon über den anderen Weg (Hook bzw. Oberfläche)?
    fn doppelt(&mut self, k: Kombination, oberflaeche: bool, jetzt: Instant) -> bool {
        if let Some((k2, t, o2)) = self.zuletzt
            && k2 == k
            && o2 != oberflaeche
            && jetzt.duration_since(t) < DOPPELT
        {
            self.zuletzt = None;
            return true;
        }
        self.zuletzt = Some((k, jetzt, oberflaeche));
        false
    }

    /// Programm im Vordergrund prüfen. Das eigene Fenster (MaskCue) ändert
    /// das Profil nicht: sonst wäre beim Einstellen immer „Aus“.
    fn vordergrund(&mut self, erzwingen: bool, ereignis: &mut dyn FnMut(Ereignis)) {
        let Some(pid) = windows::vordergrund_pid() else { return };
        if pid == self.eigene_pid || (pid == self.letzte_pid && !erzwingen) {
            return;
        }
        self.letzte_pid = pid;
        let name = windows::programm_name(pid).unwrap_or_default();
        let admin = !self.selbst_admin && windows::ist_admin(pid) != Some(false);
        let gewechselt = self.zustand.programm(&name);
        if gewechselt || erzwingen || admin != self.admin_problem {
            self.admin_problem = admin;
            self.melde_profil(ereignis);
        }
    }

    fn melde_profil(&self, ereignis: &mut dyn FnMut(Ereignis)) {
        ereignis(Ereignis::Profil {
            name: self.zustand.aktives_profil().to_string(),
            von_hand: self.zustand.ist_von_hand(),
            programm: self.zustand.programm_name().to_string(),
            admin_problem: self.admin_problem,
        });
    }

    fn taste(&mut self, k: Kombination, ereignis: &mut dyn FnMut(Ereignis)) {
        if self.lernen {
            ereignis(Ereignis::Gedrueckt(k));
            return;
        }
        self.vordergrund(false, ereignis);
        let vorher = (self.zustand.aktives_profil().to_string(), self.zustand.ist_von_hand());
        let Some(aktion) = self.zustand.taste(k) else { return };
        let profil_aktion = matches!(aktion, Aktion::Profil(_) | Aktion::ProfilAutomatisch);
        ereignis(Ereignis::Aktion { aktion, taste: k, profil: self.zustand.aktives_profil().to_string() });
        if profil_aktion && vorher != (self.zustand.aktives_profil().to_string(), self.zustand.ist_von_hand()) {
            self.melde_profil(ereignis);
        }
    }
}

impl Hotkeys {
    /// Startet die Hotkeys. `ereignis` läuft im Logik-Faden und soll schnell
    /// zurückkehren (z. B. in eine Warteschlange schicken).
    pub fn start(e: HotkeyEinstellungen, mut ereignis: impl FnMut(Ereignis) + Send + 'static) -> Result<Hotkeys, String> {
        let (tx, rx) = mpsc::channel();
        let mut hook = hook_starten(braucht_maus(&e), &tx)?;
        let tx_faden = tx.clone();
        let faden = std::thread::Builder::new()
            .name("maskcue-hotkey-logik".into())
            .spawn(move || {
                let mut l = Logik {
                    zustand: Zustand::new(e),
                    eigene_pid: std::process::id(),
                    selbst_admin: windows::selbst_admin(),
                    letzte_pid: 0,
                    admin_problem: false,
                    lernen: false,
                    zuletzt: None,
                };
                l.vordergrund(true, &mut ereignis);
                loop {
                    match rx.recv_timeout(PRUEFEN) {
                        Ok(Nachricht::Taste(k)) => {
                            if !l.doppelt(k, false, Instant::now()) {
                                l.taste(k, &mut ereignis);
                            }
                        }
                        // Beim Lernen meldet die Oberfläche selbst (hotkeyFeld).
                        Ok(Nachricht::TasteOberflaeche(k)) => {
                            if !l.lernen && !l.doppelt(k, true, Instant::now()) {
                                l.taste(k, &mut ereignis);
                            }
                        }
                        Ok(Nachricht::Einstellungen(e)) => {
                            if l.zustand.einstellungen_setzen(e) {
                                l.melde_profil(&mut ereignis);
                            }
                        }
                        Ok(Nachricht::VonHand(name)) => {
                            if l.zustand.von_hand(name.as_deref()) {
                                l.melde_profil(&mut ereignis);
                            }
                        }
                        Ok(Nachricht::Lernen(an)) => l.lernen = an,
                        Ok(Nachricht::Ende) | Err(RecvTimeoutError::Disconnected) => return,
                        Err(RecvTimeoutError::Timeout) => l.vordergrund(false, &mut ereignis),
                    }
                    // Maus-Hook nur, wenn ein Hotkey ihn braucht (oder beim Lernen).
                    let maus = braucht_maus(l.zustand.einstellungen()) || l.lernen;
                    if maus != hook.maus {
                        drop(hook);
                        hook = match hook_starten(maus, &tx_faden) {
                            Ok(h) => h,
                            Err(f) => {
                                ereignis(Ereignis::Fehler(f));
                                return;
                            }
                        };
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Hotkeys { tx, faden: Some(faden) })
    }

    pub fn einstellungen(&self, e: HotkeyEinstellungen) {
        let _ = self.tx.send(Nachricht::Einstellungen(e));
    }

    /// Profil von Hand wählen (`None` = wieder automatisch).
    pub fn profil_von_hand(&self, name: Option<String>) {
        let _ = self.tx.send(Nachricht::VonHand(name));
    }

    /// Taste, die die Oberfläche gesehen hat (MaskCue vorne). Wirkt wie eine
    /// Taste aus dem Hook; kommt sie auch dort an, zählt sie nur einmal.
    pub fn taste_von_oberflaeche(&self, k: Kombination) {
        let _ = self.tx.send(Nachricht::TasteOberflaeche(k));
    }

    /// Lern-Modus für das Hotkey-Feld: meldet jede Kombination, löst nichts aus.
    pub fn lernen(&self, an: bool) {
        let _ = self.tx.send(Nachricht::Lernen(an));
    }
}

impl Drop for Hotkeys {
    fn drop(&mut self) {
        let _ = self.tx.send(Nachricht::Ende);
        if let Some(f) = self.faden.take() {
            let _ = f.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::profil::{Belegung, PROFIL_AUS};
    use super::*;
    use std::time::Duration;

    #[test]
    fn taste_ueber_beide_wege_zaehlt_einmal() {
        let mut l = Logik {
            zustand: Zustand::new(HotkeyEinstellungen::default()),
            eigene_pid: 0,
            selbst_admin: false,
            letzte_pid: 0,
            admin_problem: false,
            lernen: false,
            zuletzt: None,
        };
        let k: Kombination = "K".parse().unwrap();
        let t = Instant::now();
        // Hook, dann Oberfläche kurz danach: einmal.
        assert!(!l.doppelt(k, false, t));
        assert!(l.doppelt(k, true, t + Duration::from_millis(30)));
        // Oberfläche zuerst, dann Hook: einmal.
        assert!(!l.doppelt(k, true, t + Duration::from_millis(1000)));
        assert!(l.doppelt(k, false, t + Duration::from_millis(1040)));
        // Zweimal schnell über denselben Weg: zweimal (echter Doppeldruck).
        assert!(!l.doppelt(k, true, t + Duration::from_millis(2000)));
        assert!(!l.doppelt(k, true, t + Duration::from_millis(2100)));
        // Andere Taste: zählt.
        assert!(!l.doppelt("L".parse().unwrap(), false, t + Duration::from_millis(2120)));
        // Zu spät: zählt wieder.
        assert!(!l.doppelt(k, false, t + Duration::from_millis(3000)));
        assert!(!l.doppelt(k, true, t + Duration::from_millis(3400)));
    }

    /// Lern-Modus: Eine Tastatur-Taste kommt als `Gedrueckt` an (Fehler von Dan:
    /// nur Maustasten kamen an).
    #[test]
    #[ignore = "drückt echte Tasten (F24); von Hand starten: cargo test -- --ignored lernen"]
    fn lernen_meldet_tastatur() {
        let _sperre = windows::TASTEN_TEST.lock().unwrap_or_else(|e| e.into_inner());
        let e = HotkeyEinstellungen { ohne_spiel: None, global: vec![], profile: vec![] };
        let (tx, rx) = mpsc::channel();
        let h = Hotkeys::start(e, move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        h.lernen(true);
        std::thread::sleep(Duration::from_millis(300));
        windows::taste_senden(0x87, false);
        windows::taste_senden(0x87, true);
        let ende = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let rest = ende.saturating_duration_since(std::time::Instant::now());
            match rx.recv_timeout(rest) {
                Ok(Ereignis::Gedrueckt(k)) => {
                    assert_eq!(k.to_string(), "F24");
                    break;
                }
                Ok(_) => {}
                Err(_) => panic!("Tastatur-Taste kam im Lern-Modus nicht an"),
            }
        }
    }

    /// Ganzer Weg: Hook → Logik → Ereignis. Im Profil „Aus“ geht nur die Notbremse.
    #[test]
    #[ignore = "drückt echte Tasten (F24); von Hand starten: cargo test -- --ignored hook"]
    fn hook_notbremse_im_profil_aus() {
        let _sperre = windows::TASTEN_TEST.lock().unwrap_or_else(|e| e.into_inner());
        let e = HotkeyEinstellungen {
            ohne_spiel: None,
            global: vec![Belegung { taste: "F24".parse().unwrap(), aktion: Aktion::AllesStopp }],
            profile: vec![],
        };
        let (tx, rx) = mpsc::channel();
        let _h = Hotkeys::start(e, move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        match rx.recv_timeout(Duration::from_secs(2)).expect("Profil-Meldung") {
            Ereignis::Profil { name, .. } => assert_eq!(name, PROFIL_AUS),
            e => panic!("{e:?}"),
        }
        windows::taste_senden(0x87, false);
        windows::taste_senden(0x87, true);
        match rx.recv_timeout(Duration::from_secs(2)).expect("Aktion") {
            Ereignis::Aktion { aktion, profil, .. } => {
                assert_eq!(aktion, Aktion::AllesStopp);
                assert_eq!(profil, PROFIL_AUS);
            }
            e => panic!("{e:?}"),
        }
    }
}
