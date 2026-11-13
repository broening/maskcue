//! Motor der App: Tonweg, Regler, Sounds, Figur und Hotkeys an einem Ort.
//!
//! Läuft in einem eigenen Faden und nimmt Aufträge aus einer Warteschlange.
//! So greift nie ein zweiter Faden gleichzeitig auf den Tonweg zu, und die
//! Hotkeys warten nie auf die Oberfläche.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::audio::denoise::{Denoise, DenoiseWerte, Modell};
use crate::audio::platz::vor_und_nach_ki;
use crate::audio::dynamik::{Gate, GateEinstellung, Kompressor, KompressorEinstellung, PegelAnzeige, Pegelmesser};
use crate::audio::eq::{Band, Equalizer};
use crate::audio::figur::{FigurSteuerung, figur};
use crate::audio::ki::beatrice::{StimmDaten, StimmPaket};
use crate::audio::ki::{self, KiStimme, KiWahl};
use crate::audio::pipe::PipeConfig;
use crate::audio::regler::{Live, Regler, kanal};
use crate::audio::sounddatei::SoundDaten;
use crate::audio::pipe::INTERN_RATE;
use super::aufnahmen::{self, Abhoeren};
use crate::audio::stumm::Stumm;
use crate::audio::umgebung::{Umgebung, UmgebungSteuerung, UmgebungWahl, umgebung};
use std::sync::atomic::AtomicBool;
use crate::audio::tonweg::{self, Tonweg};
use crate::audio::verarbeitung::{Effekt, Kette, KettenBau};
use crate::hotkeys::profil::{Aktion, PROFIL_AUS};
use crate::hotkeys::{Ereignis, Hotkeys};

use super::einstellungen::{DenoiseModell, DenoiseWahl, Einstellungen, Geraete};
use super::sounds::{self, SoundVerwaltung};
use super::ui_toene::{Ton, UiToene};

/// So oft geht ein Status an die Oberfläche.
const STATUS_ALLE: Duration = Duration::from_millis(100);
/// So lange nach der letzten Änderung wird gespeichert.
const SPEICHERN_NACH: Duration = Duration::from_millis(800);

#[derive(Clone, Debug, Default, Serialize)]
pub struct AusgangStatus {
    pub name: String,
    pub latenz_ms: Option<f64>,
    pub aussetzer: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    pub laeuft: bool,
    pub fehler: Option<String>,
    pub profil: String,
    pub profil_von_hand: bool,
    pub programm: String,
    pub admin_problem: bool,
    /// Aktive Figur (`None` = normale Stimme).
    pub figur: Option<String>,
    pub ausgaenge: Vec<AusgangStatus>,
    /// Lautester 10-ms-Block seit dem letzten Status, vor dem Gate (dBFS).
    pub pegel_db: Option<f32>,
    pub sounds_laufen: Vec<String>,
    /// Stelle, Länge und Pause der laufenden Sounds (für die Laufzeit-Leiste).
    pub sounds_stand: Vec<SoundStand>,
    pub sounds_fehler: Vec<(String, String)>,
    pub sounds_laden: usize,
    /// MaskCue-Stummtaste aktiv: Die Stimme geht nirgends hin.
    pub stumm: bool,
    /// Gerade wird gesprochen (Pegel über der Empfindlichkeit, nicht stumm).
    pub spricht: bool,
    /// Aktive Umgebung (von der Figur oder per Hotkey).
    pub umgebung: Option<Umgebung>,
    /// Die Umgebung kommt vom Hotkey (nicht von der Figur).
    pub umgebung_hotkey: bool,
    /// Läuft eine Aufnahme: ihre Länge in Sekunden.
    pub aufnahme_s: Option<f64>,
    /// Eine Aufnahme wird gerade angehört.
    pub abhoeren: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SoundStand {
    pub name: String,
    pub pos_s: f32,
    pub laenge_s: f32,
    pub pausiert: bool,
}

/// Was der Motor an die Oberfläche meldet.
#[derive(Clone, Debug)]
pub enum Meldung {
    Status(Status),
    /// Lern-Modus: gedrückte Taste für das Hotkey-Feld.
    Taste(String),
    /// Hinweis oder Fehler für eine kurze Anzeige.
    Hinweis(String),
    /// Aufnahme beendet: Name der gespeicherten Aufnahme oder Fehler.
    AufnahmeFertig(Result<String, String>),
}

enum Auftrag {
    Einstellungen(Box<Einstellungen>),
    Ausloesen(String),
    /// Laufenden Sound anhalten oder weiterspielen.
    SoundPause(String),
    /// Einen Sound stoppen (auch bei „Nochmal drücken: startet neu“).
    SoundStopp(String),
    /// Laufenden Sound an eine Stelle setzen (Anteil 0…1).
    SoundSpringen(String, f32),
    AllesStopp,
    Figur(Option<String>),
    Profil(Option<String>),
    Lernen(bool),
    Neustart,
    Geladen(PathBuf, Result<Arc<SoundDaten>, String>),
    Hotkey(Ereignis),
    UiTon(Ton),
    /// `None` = umschalten.
    Stumm(Option<bool>),
    /// Taste aus der Oberfläche (MaskCue vorne).
    Taste(crate::hotkeys::taste::Kombination),
    /// Umgebung umschalten (`None` = jede Umgebung aus).
    Umgebung(Option<Umgebung>),
    /// Aufnahme an (`Some(true)`), aus (`Some(false)`) oder umschalten (`None`).
    Aufnahme(Option<bool>),
    /// Aufnahme anhören (`None` = Anhören stoppen).
    Abhoeren(Option<PathBuf>),
    AbhoerenGeladen(Result<Arc<SoundDaten>, String>),
    /// KI-Stimme fertig geladen (Schlüssel, Ergebnis).
    KiGeladen(String, Result<Box<KiStimme>, String>),
    /// Neue Mischung für das schon geladene Modell (Schlüssel, Paket).
    KiPaket(String, Box<StimmPaket>),
    /// Hörprobe: Wahl, Tonhöhe (fein), Formant.
    KiProbe(KiWahl, f32, f32),
    Ende,
}

/// Was einen Neustart des Tonwegs braucht (alles andere geht im Lauf).
#[derive(Clone, Debug, PartialEq)]
struct Aufbau {
    geraete: Geraete,
    stumm: bool,
    denoise_spiel: DenoiseModell,
    denoise_chat: DenoiseModell,
}

impl Aufbau {
    fn von(e: &Einstellungen) -> Self {
        Self { geraete: e.geraete.clone(), stumm: e.stumm, denoise_spiel: e.mikro.denoise_spiel.modell, denoise_chat: e.mikro.denoise_chat.modell }
    }
}

/// Regler eines Ausgangs.
struct WegRegler {
    denoise: Option<Regler<DenoiseWerte>>,
    gate: Regler<GateEinstellung>,
    kompressor: Regler<KompressorEinstellung>,
    klang: Regler<Vec<Band>>,
}

struct Laufend {
    tw: Tonweg,
    aufbau: Aufbau,
    hochpass: Regler<Vec<Band>>,
    wege: Vec<WegRegler>,
    figur: FigurSteuerung,
    umgebung: UmgebungSteuerung,
    /// Zuletzt an den Effekt geschickt.
    umgebung_gesendet: UmgebungWahl,
    pegel: Arc<PegelAnzeige>,
    /// Gerade KI-Figur: Rauschunterdrückung und Gate laufen dann nach der
    /// Stimme statt davor (siehe [`crate::audio::platz`]).
    ki_aktiv: Arc<AtomicBool>,
    start: Instant,
    /// Zuletzt geschickte Werte (nur Änderungen schicken).
    gesendet: Einstellungen,
}

fn denoise_werte(d: &DenoiseWahl) -> DenoiseWerte {
    DenoiseWerte { daempfung_db: d.daempfung_db, nachfilter: 0.0 }
}

fn modell(m: DenoiseModell) -> Option<Modell> {
    match m {
        DenoiseModell::Aus => None,
        DenoiseModell::Leicht => Some(Modell::Leicht),
        DenoiseModell::Normal => Some(Modell::Normal),
    }
}

fn starten(e: &Einstellungen, stumm: Arc<AtomicBool>) -> Result<Laufend, String> {
    let g = &e.geraete;
    if g.eingang.trim().is_empty() {
        return Err("Bitte ein Mikrofon wählen.".into());
    }
    let mut ausgaenge = vec![g.spiel.as_str()];
    if !g.chat.trim().is_empty() {
        ausgaenge.push(g.chat.as_str());
    }
    let m = e.mikro;
    let pegel = PegelAnzeige::new();

    // Regler hier anlegen, Empfänger wandern in den Verarbeitungs-Faden.
    let (hochpass, hochpass_rx) = kanal();
    let (figur_effekt, figur_st) = figur(None);
    let (umgebung_effekt, umgebung_st) = umgebung(None);
    let mut wege = Vec::new();
    let mut empfang = Vec::new();
    for (i, _) in ausgaenge.iter().enumerate() {
        let (d, gate, an_gate) = if i == 0 { (m.denoise_spiel, m.gate_spiel.einstellung(), m.gate_spiel.an) } else { (m.denoise_chat, m.gate_chat.einstellung(), m.gate_chat.an) };
        let modell = modell(d.modell);
        let (dr, dx) = kanal();
        let (gr, gx) = kanal();
        let (kr, kx) = kanal();
        let (lr, lx) = kanal();
        wege.push(WegRegler { denoise: modell.map(|_| dr), gate: gr, kompressor: kr, klang: lr });
        empfang.push((modell, denoise_werte(&d), dx, gate, an_gate, gx, kx, lx));
    }
    let pegel_kette = pegel.clone();
    let ki_aktiv = Arc::new(AtomicBool::new(false));
    let ki_kette = ki_aktiv.clone();
    let mut figur_effekt = Some(figur_effekt);
    let mut umgebung_effekt = Some(umgebung_effekt);
    let bau: KettenBau = Box::new(move || {
        let mut kette = Kette::default();
        kette.gemeinsam.push(Box::new(Live::new(Equalizer::new(&m.hochpass()), true, hochpass_rx)));
        for (i, (modell, dw, dx, gate, an_gate, gx, kx, lx)) in empfang.into_iter().enumerate() {
            let mut weg: Vec<Box<dyn Effekt>> = Vec::new();
            let denoise = match modell {
                Some(modell) => Some(Box::new(Live::new(Denoise::laden(modell, dw.daempfung_db, dw.nachfilter)?, true, dx)) as Box<dyn Effekt>),
                None => None,
            };
            let gate: Box<dyn Effekt> = Box::new(Live::new(Gate::new(gate), an_gate, gx));
            // Spiel: Rauschunterdrückung und Gate vor der Figur, bei KI-Figur danach.
            let mut nach_figur: Vec<Box<dyn Effekt>> = Vec::new();
            let mut setzen = |weg: &mut Vec<Box<dyn Effekt>>, e: Box<dyn Effekt>| {
                if i == 0 {
                    let (vor, nach) = vor_und_nach_ki(e, ki_kette.clone());
                    weg.push(Box::new(vor));
                    nach_figur.push(Box::new(nach));
                } else {
                    weg.push(e);
                }
            };
            if let Some(d) = denoise {
                setzen(&mut weg, d);
            }
            if i == 0 {
                weg.push(Box::new(Pegelmesser(pegel_kette.clone())));
            }
            setzen(&mut weg, gate);
            weg.push(Box::new(Live::new(Kompressor::new(m.kompressor.einstellung()), m.kompressor.an, kx)));
            weg.push(Box::new(Live::new(Equalizer::new(&m.klang()), true, lx)));
            if i == 0 {
                weg.push(Box::new(figur_effekt.take().expect("einmal")));
                weg.append(&mut nach_figur);
                weg.push(Box::new(umgebung_effekt.take().expect("einmal")));
            }
            weg.push(Box::new(Stumm::new(stumm.clone())));
            kette.pro_ausgang.push(weg);
        }
        Ok(kette)
    });
    let config = PipeConfig { stumm: e.stumm, ..Default::default() };
    let tw = tonweg::start(&g.eingang, &ausgaenge, config, bau, true)?;
    let mut l = Laufend {
        tw,
        aufbau: Aufbau::von(e),
        hochpass,
        wege,
        figur: figur_st,
        umgebung: umgebung_st,
        umgebung_gesendet: None,
        pegel,
        ki_aktiv,
        start: Instant::now(),
        gesendet: e.clone(),
    };
    l.tw.soundboard.mischer_einstellen(e.mischer.einstellung());
    Ok(l)
}

impl Laufend {
    /// Schickt geänderte Werte an die Effekte. Nichts startet neu.
    fn regler_setzen(&mut self, e: &Einstellungen) {
        let (a, n) = (&self.gesendet.mikro, &e.mikro);
        if a.hochpass_hz != n.hochpass_hz {
            self.hochpass.setzen(n.hochpass());
        }
        for (i, w) in self.wege.iter_mut().enumerate() {
            let (ad, nd, ag, ng) = if i == 0 { (a.denoise_spiel, n.denoise_spiel, a.gate_spiel, n.gate_spiel) } else { (a.denoise_chat, n.denoise_chat, a.gate_chat, n.gate_chat) };
            if let Some(d) = w.denoise.as_mut()
                && ad.daempfung_db != nd.daempfung_db
            {
                d.setzen(denoise_werte(&nd));
            }
            if ag.schwelle_db != ng.schwelle_db {
                w.gate.setzen(ng.einstellung());
            }
            if ag.an != ng.an {
                w.gate.an(ng.an);
            }
            if a.kompressor != n.kompressor {
                w.kompressor.setzen(n.kompressor.einstellung());
                w.kompressor.an(n.kompressor.an);
            }
            if a.bass_db != n.bass_db || a.hoehen_db != n.hoehen_db {
                w.klang.setzen(n.klang());
            }
        }
        if self.gesendet.mischer != e.mischer {
            self.tw.soundboard.mischer_einstellen(e.mischer.einstellung());
        }
        self.gesendet = e.clone();
    }
}

struct Motor {
    e: Einstellungen,
    datei: PathBuf,
    lauf: Option<Laufend>,
    fehler: Option<String>,
    sounds: SoundVerwaltung,
    hotkeys: Option<Hotkeys>,
    profil: (String, bool, String, bool),
    figur: Option<String>,
    /// Umgebung per Hotkey: `None` = die der Figur gilt, `Some(None)` = aus
    /// (auch die der Figur), `Some(Some(u))` = `u`.
    umgebung_hotkey: Option<Option<Umgebung>>,
    /// Laufende Aufnahme: Name und Samples (48 kHz mono).
    aufnahme: Option<(String, Vec<f32>)>,
    abhoeren: Option<Abhoeren>,
    /// KI-Stimme, die der laufende Tonweg hat bzw. die gerade lädt (Schlüssel).
    ki_gesendet: Option<String>,
    ki_laedt: Option<String>,
    /// Modell, das im laufenden Tonweg steckt (Ordner, Stimm-Daten).
    ki_modell: Option<(String, Arc<StimmDaten>)>,
    /// Zu späte KI-Blöcke beim letzten Prüfen und wann.
    ki_spaet: (u64, Instant),
    ki_gewarnt: Option<Instant>,
    tx: Sender<Auftrag>,
    speichern_ab: Option<Instant>,
    ui: Option<UiToene>,
    stumm: Arc<AtomicBool>,
    /// Bis wann „spricht“ gilt (hält kurz nach, damit der Rahmen nicht flackert).
    spricht_bis: Instant,
    melden: Box<dyn Fn(Meldung) + Send>,
}

impl Motor {
    fn neu_starten(&mut self) {
        // Eine laufende Aufnahme endet hier (der Abgriff gehört zum Tonweg).
        if self.aufnahme.is_some() {
            self.aufnahme_stopp();
        }
        // Erst den alten Tonweg ganz beenden: Geräte sind dann frei.
        self.lauf = None;
        self.ki_gesendet = None;
        self.ki_modell = None;
        self.sounds.board_neu();
        match starten(&self.e, self.stumm.clone()) {
            Ok(l) => {
                self.lauf = Some(l);
                self.fehler = None;
                // Figur und Umgebung bleiben über den Neustart.
                let f = self.figur.take();
                self.figur_setzen(f);
                self.umgebung_abgleichen();
            }
            Err(f) => self.fehler = Some(f),
        }
        self.sounds_abgleichen();
    }

    fn sounds_abgleichen(&mut self) {
        let sb = self.lauf.as_mut().map(|l| &mut l.tw.soundboard);
        for datei in self.sounds.abgleichen(&self.e.sounds, sb) {
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let ergebnis = sounds::laden(&datei);
                let _ = tx.send(Auftrag::Geladen(datei, ergebnis));
            });
        }
    }

    fn figur_setzen(&mut self, name: Option<String>) {
        let def = name.as_ref().and_then(|n| self.e.figuren.iter().find(|f| f.name.eq_ignore_ascii_case(n)));
        if name.is_some() && def.is_none() {
            (self.melden)(Meldung::Hinweis(format!("Figur „{}“ gibt es nicht.", name.unwrap_or_default())));
            return;
        }
        let einstellung = def.map(|f| {
            let mut e = f.einstellung();
            if let Some(t) = e.ki.as_mut() {
                t.halbtoene = self.ki_halbtoene(f.ki.as_ref(), t.halbtoene);
            }
            e
        });
        let mit_ki = einstellung.is_some_and(|e| e.ki.is_some());
        if let Some(l) = self.lauf.as_mut() {
            l.figur.setzen(einstellung);
            l.ki_aktiv.store(mit_ki, Relaxed);
        }
        // KI-Figur: Modell im Hintergrund laden, wenn es noch nicht da ist.
        let ki = def.filter(|_| mit_ki).map(|f| {
            let t = einstellung.and_then(|e| e.ki).unwrap_or(crate::audio::figur::KiTon { halbtoene: 0.0, formant: 0.0 });
            (f.name.clone(), f.ki.clone(), t.halbtoene, t.formant)
        });
        if ki.is_none() {
            // Der Effekt wirft die KI-Stimme weg, sobald keine KI-Figur mehr gilt.
            self.ki_gesendet = None;
            self.ki_modell = None;
        }
        let neu = def.map(|f| f.name.clone());
        if neu != self.figur {
            self.umgebung_hotkey = umgebung_nach_figurwechsel(self.umgebung_hotkey, def.and_then(|f| f.umgebung));
        }
        self.figur = neu;
        self.umgebung_abgleichen();
        match ki {
            Some((_, Some(wahl), halbtoene, formant)) => self.ki_laden(wahl, halbtoene, formant),
            Some((name, None, ..)) => (self.melden)(Meldung::Hinweis(format!("Figur „{name}“: Keine KI-Stimme gewählt."))),
            None => {}
        }
    }

    /// Lädt eine KI-Stimme im Hintergrund (nur Modell und Mischung zählen;
    /// Tonhöhe, Formant und Laut-Anteil gehen ohne Neuladen).
    fn ki_laden(&mut self, wahl: KiWahl, halbtoene: f32, formant: f32) {
        let schluessel = ki_schluessel(&wahl);
        if self.lauf.is_none() || self.ki_gesendet.as_ref() == Some(&schluessel) || self.ki_laedt.as_ref() == Some(&schluessel) {
            return;
        }
        if !ki::modell_name_ok(&wahl.modell) {
            (self.melden)(Meldung::Hinweis(format!("KI-Modell „{}“: ungültiger Name.", wahl.modell)));
            return;
        }
        self.ki_laedt = Some(schluessel.clone());
        let tx = self.tx.clone();
        // Gleiches Modell läuft schon: nur die Mischung vorrechnen.
        if let Some((m, daten)) = &self.ki_modell
            && *m == wahl.modell
        {
            let daten = daten.clone();
            let stimmen: Vec<(usize, f32)> = wahl.stimmen.iter().map(|a| (a.nr, a.gewicht)).collect();
            std::thread::spawn(move || {
                let _ = tx.send(Auftrag::KiPaket(schluessel, Box::new(daten.paket(&stimmen))));
            });
            return;
        }
        let ordner = super::ki_modelle::ordner().join(&wahl.modell);
        std::thread::spawn(move || {
            let ergebnis = ki::vorbereiten(&ordner, &wahl, halbtoene, formant).map(Box::new);
            let _ = tx.send(Auftrag::KiGeladen(schluessel, ergebnis));
        });
    }

    /// Tonhöhe für eine KI-Stimme: automatisch (eigene → Ziel-Tonhöhe) plus
    /// Feinabstimmung, oder nur der Wert der Figur.
    fn ki_halbtoene(&self, wahl: Option<&KiWahl>, fein: f32) -> f32 {
        let Some(w) = wahl.filter(|w| w.tonhoehe_auto) else { return fein };
        let eigene = self.e.ki.eigene_hz.unwrap_or(EIGENE_HZ_STANDARD);
        match super::ki_modelle::ziel_hz(&super::ki_modelle::ordner(), &w.modell, &w.stimmen) {
            Some(ziel) => (ki::halbtoene_zu(eigene, ziel) + fein).clamp(-24.0, 24.0),
            None => fein,
        }
    }

    /// Hörprobe mit dem Probesatz auf dem Abhör-Gerät.
    fn ki_probe(&mut self, wahl: KiWahl, fein: f32, formant: f32) {
        let satz = self.e.ki.probesatz.clone();
        if satz.is_empty() {
            return (self.melden)(Meldung::Hinweis("Bitte zuerst einen Probesatz wählen (eine Aufnahme mit deiner normalen Stimme).".into()));
        }
        let halbtoene = self.ki_halbtoene(Some(&wahl), fein);
        let datei = super::aufnahmen::pfad(&super::aufnahmen::standard_ordner(), &satz);
        let ordner = super::ki_modelle::ordner().join(&wahl.modell);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let ergebnis = (|| {
                let d = sounds::laden(&datei?)?;
                let mono: Vec<f32> = d.samples.chunks(d.kanaele.max(1)).map(|c| c.iter().sum::<f32>() / c.len() as f32).collect();
                let y = ki::probe(&ordner, &wahl, halbtoene, formant, &mono)?;
                Ok(Arc::new(SoundDaten { name: format!("Probe {}", wahl.modell), kanaele: 1, samples: y }))
            })();
            let _ = tx.send(Auftrag::AbhoerenGeladen(ergebnis));
        });
    }

    /// Kommt die KI nicht mit (Blöcke zu spät = Lücken), einmal warnen.
    /// Danach erst wieder nach einer Minute.
    fn ki_pruefen(&mut self) {
        let jetzt = ki::ZU_SPAET_GESAMT.load(Relaxed);
        let (stand, zeit) = self.ki_spaet;
        if zeit.elapsed() < KI_PRUEFEN {
            return;
        }
        self.ki_spaet = (jetzt, Instant::now());
        // Mehr als 2 % der Blöcke in den letzten Sekunden.
        let grenze = (KI_PRUEFEN.as_millis() / 10 / 50) as u64;
        if jetzt.saturating_sub(stand) > grenze && self.ki_gewarnt.is_none_or(|t| t.elapsed() > Duration::from_secs(60)) {
            self.ki_gewarnt = Some(Instant::now());
            (self.melden)(Meldung::Hinweis(
                "Die KI-Stimme kommt nicht mit: Der Prozessor ist zu ausgelastet. Es entstehen kurze Lücken. Andere Programme schließen oder ein anderes Verfahren wählen.".into(),
            ));
        }
    }

    fn ki_paket(&mut self, schluessel: String, p: Box<StimmPaket>) {
        if self.ki_laedt.as_ref() == Some(&schluessel) {
            self.ki_laedt = None;
        }
        if self.ki_gewuenscht().as_ref() != Some(&schluessel) {
            return;
        }
        if let Some(l) = self.lauf.as_mut()
            && l.figur.ki_paket(p)
        {
            self.ki_gesendet = Some(schluessel);
        }
    }

    /// Schlüssel der KI-Stimme, die die aktive Figur will.
    fn ki_gewuenscht(&self) -> Option<String> {
        self.figur_def().filter(|f| f.ki_an).and_then(|f| f.ki.as_ref()).map(ki_schluessel)
    }

    fn ki_geladen(&mut self, schluessel: String, ergebnis: Result<Box<KiStimme>, String>) {
        if self.ki_laedt.as_ref() == Some(&schluessel) {
            self.ki_laedt = None;
        }
        // Inzwischen eine andere Figur? Dann wegwerfen.
        if self.ki_gewuenscht().as_ref() != Some(&schluessel) {
            return;
        }
        match ergebnis {
            Ok(k) => {
                let daten = k.stimm_daten();
                let modell = schluessel.split('|').next().unwrap_or_default().to_string();
                if let Some(l) = self.lauf.as_mut()
                    && l.figur.ki(k)
                {
                    self.ki_gesendet = Some(schluessel);
                    self.ki_modell = daten.map(|d| (modell, d));
                }
            }
            Err(f) => (self.melden)(Meldung::Hinweis(format!("KI-Stimme: {f}"))),
        }
    }

    fn figur_def(&self) -> Option<&crate::audio::figur::FigurDef> {
        self.figur.as_ref().and_then(|n| self.e.figuren.iter().find(|f| &f.name == n))
    }

    /// Umgebung der aktiven Figur.
    fn figur_umgebung(&self) -> Option<Umgebung> {
        self.figur_def().and_then(|f| f.umgebung)
    }

    /// Umgebung, die gerade gilt: Hotkey vor Figur.
    fn umgebung_jetzt(&self) -> Option<Umgebung> {
        self.umgebung_hotkey.unwrap_or_else(|| self.figur_umgebung())
    }

    /// Hotkey für Umgebung `u` (`None` = jede aus).
    fn umgebung_umschalten(&mut self, u: Option<Umgebung>) {
        self.umgebung_hotkey = umgebung_regel(self.umgebung_hotkey, self.figur_umgebung(), u);
        self.umgebung_abgleichen();
    }

    /// Schickt die geltende Umgebung an den Effekt, wenn sie sich geändert hat.
    fn umgebung_abgleichen(&mut self) {
        // Eigene Stärke der Figur gilt nur für ihre eigene Umgebung (nicht per Hotkey).
        let eigene = if self.umgebung_hotkey.is_none() { self.figur_def().and_then(|f| f.umgebung_staerke) } else { None };
        let wahl = self.umgebung_jetzt().map(|u| (u, eigene.unwrap_or_else(|| self.e.umgebung_staerke(u)).clamp(0.0, 1.0)));
        if let Some(l) = self.lauf.as_mut()
            && l.umgebung_gesendet != wahl
            && l.umgebung.setzen(wahl)
        {
            l.umgebung_gesendet = wahl;
        }
    }

    fn einstellungen(&mut self, neu: Einstellungen) {
        let alt = std::mem::replace(&mut self.e, neu);
        if alt.hotkeys != self.e.hotkeys
            && let Some(h) = &self.hotkeys
        {
            h.einstellungen(self.e.hotkeys.clone());
        }
        let neustart = self.lauf.as_ref().map_or(Aufbau::von(&alt) != Aufbau::von(&self.e) || self.fehler.is_some(), |l| l.aufbau != Aufbau::von(&self.e));
        if neustart {
            self.neu_starten();
        } else {
            if let Some(l) = self.lauf.as_mut() {
                l.regler_setzen(&self.e);
            }
            self.sounds_abgleichen();
        }
        // Werte der aktiven Figur geändert (Regler in der UI): neu schicken.
        if let Some(n) = self.figur.clone()
            && (alt.figuren != self.e.figuren || alt.ki != self.e.ki)
        {
            let f = if self.e.figuren.iter().any(|f| f.name == n) { Some(n) } else { None };
            self.figur_setzen(f);
        }
        if alt.oberflaeche != self.e.oberflaeche {
            self.ui_abgleichen();
        }
        // Stärke oder Umgebung einer Figur geändert.
        self.umgebung_abgleichen();
        self.speichern_ab = Some(Instant::now() + SPEICHERN_NACH);
    }

    /// Klick-Töne an, aus oder auf ein anderes Gerät.
    fn ui_abgleichen(&mut self) {
        let o = &self.e.oberflaeche;
        let geraet = o.ui_toene_geraet.trim();
        if !o.ui_toene || geraet.is_empty() {
            self.ui = None;
            return;
        }
        if self.ui.as_ref().is_some_and(|u| u.geraet == geraet) {
            return;
        }
        self.ui = None;
        match UiToene::start(geraet) {
            Ok(u) => self.ui = Some(u),
            Err(f) => (self.melden)(Meldung::Hinweis(format!("Klick-Töne: {f}"))),
        }
    }

    fn speichern(&mut self) {
        self.speichern_ab = None;
        if let Err(f) = self.e.speichern(&self.datei) {
            (self.melden)(Meldung::Hinweis(format!("Speichern ging nicht: {f}")));
        }
    }

    fn aktion(&mut self, a: Aktion) {
        match a {
            Aktion::Sound(n) => self.ausloesen(&n),
            Aktion::AllesStopp => self.alles_stopp(),
            Aktion::Figur(n) => self.figur_setzen(Some(n)),
            Aktion::FigurAus => self.figur_setzen(None),
            Aktion::StummUmschalten => self.stumm_setzen(None),
            Aktion::Umgebung(u) => self.umgebung_umschalten(Some(u)),
            Aktion::UmgebungAus => self.umgebung_umschalten(None),
            Aktion::AufnahmeUmschalten => self.aufnahme_schalten(None),
            Aktion::Profil(_) | Aktion::ProfilAutomatisch => {}
        }
    }

    fn aufnahme_schalten(&mut self, an: Option<bool>) {
        match an.unwrap_or(self.aufnahme.is_none()) {
            true if self.aufnahme.is_none() => {
                let Some(l) = self.lauf.as_mut() else {
                    (self.melden)(Meldung::Hinweis("Aufnahme geht nicht: Der Ton läuft nicht.".into()));
                    return;
                };
                l.tw.aufnahme.schalten(true);
                let name = aufnahmen::neuer_name(&aufnahmen::standard_ordner());
                self.aufnahme = Some((name, Vec::with_capacity(INTERN_RATE as usize * 60)));
            }
            false if self.aufnahme.is_some() => self.aufnahme_stopp(),
            _ => {}
        }
    }

    /// Holt die Samples aus dem Abgriff. Stoppt nach [`aufnahmen::MAX_SEKUNDEN`].
    fn aufnahme_abholen(&mut self) {
        if let (Some((_, daten)), Some(l)) = (self.aufnahme.as_mut(), self.lauf.as_mut()) {
            l.tw.aufnahme.abholen(&mut |s| daten.extend_from_slice(s));
            if daten.len() as f64 / INTERN_RATE as f64 >= aufnahmen::MAX_SEKUNDEN {
                self.aufnahme_stopp();
            }
        }
    }

    fn aufnahme_stopp(&mut self) {
        let mut verloren = 0;
        if let Some(l) = self.lauf.as_mut() {
            if let Some((_, daten)) = self.aufnahme.as_mut() {
                l.tw.aufnahme.abholen(&mut |s| daten.extend_from_slice(s));
            }
            verloren = l.tw.aufnahme.verloren();
            l.tw.aufnahme.schalten(false);
        }
        let Some((name, daten)) = self.aufnahme.take() else { return };
        let ergebnis = aufnahmen::speichern(&aufnahmen::standard_ordner(), &name, &daten).map(|_| name);
        if verloren > 0 {
            (self.melden)(Meldung::Hinweis(format!("Die Aufnahme hat Lücken ({:.1} s fehlen).", verloren as f64 / INTERN_RATE as f64)));
        }
        (self.melden)(Meldung::AufnahmeFertig(ergebnis));
    }

    fn abhoeren(&mut self, datei: Option<PathBuf>) {
        match datei {
            None => {
                if let Some(a) = self.abhoeren.as_mut() {
                    a.stopp();
                }
            }
            Some(p) => {
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(Auftrag::AbhoerenGeladen(sounds::laden(&p)));
                });
            }
        }
    }

    fn abhoeren_geladen(&mut self, daten: Result<Arc<SoundDaten>, String>) {
        let daten = match daten {
            Ok(d) => d,
            Err(f) => return (self.melden)(Meldung::Hinweis(format!("Anhören: {f}"))),
        };
        let geraet = self.e.oberflaeche.ui_toene_geraet.trim().to_string();
        if geraet.is_empty() {
            return (self.melden)(Meldung::Hinweis("Bitte in den Einstellungen einen Sound-Ausgang wählen (z. B. Kopfhörer).".into()));
        }
        if self.abhoeren.as_ref().is_none_or(|a| a.geraet != geraet) {
            self.abhoeren = None;
            match Abhoeren::start(&geraet) {
                Ok(a) => self.abhoeren = Some(a),
                // Kein Ersatz-Gerät: Der Windows-Standard kann ein Stream- oder
                // Chat-Kanal sein, dann hörten andere die Aufnahme mit.
                // Kaputte Bluetooth-Namen repariert `geraet_name.rs`.
                Err(f) if f.ends_with("nicht gefunden") => {
                    return (self.melden)(Meldung::Hinweis(format!(
                        "Anhören: „{geraet}“ nicht gefunden. Ist es verbunden? Sonst in den Einstellungen den Sound-Ausgang neu wählen."
                    )));
                }
                Err(f) => return (self.melden)(Meldung::Hinweis(format!("Anhören: {f}"))),
            }
        }
        if let Some(a) = self.abhoeren.as_mut() {
            a.spielen(daten);
        }
    }

    fn stumm_setzen(&mut self, an: Option<bool>) {
        let neu = an.unwrap_or(!self.stumm.load(Relaxed));
        self.stumm.store(neu, Relaxed);
    }

    fn ausloesen(&mut self, name: &str) {
        match (self.sounds.id(name), self.lauf.as_mut()) {
            (Some(id), Some(l)) => l.tw.soundboard.ausloesen(id),
            (None, _) => (self.melden)(Meldung::Hinweis(format!("Sound „{name}“ ist nicht geladen."))),
            _ => {}
        }
    }

    fn sound_pause(&mut self, name: &str) {
        if let (Some(id), Some(l)) = (self.sounds.id(name), self.lauf.as_mut()) {
            l.tw.soundboard.pause(id);
        }
    }

    fn sound_stopp(&mut self, name: &str) {
        if let (Some(id), Some(l)) = (self.sounds.id(name), self.lauf.as_mut()) {
            l.tw.soundboard.stopp(id);
        }
    }

    fn sound_springen(&mut self, name: &str, anteil: f32) {
        if let (Some(id), Some(l)) = (self.sounds.id(name), self.lauf.as_mut()) {
            l.tw.soundboard.springen(id, anteil);
        }
    }

    fn alles_stopp(&mut self) {
        if let Some(l) = self.lauf.as_mut() {
            l.tw.soundboard.alles_stopp();
        }
    }

    fn status(&mut self) -> Status {
        let (profil, profil_von_hand, programm, admin_problem) = self.profil.clone();
        let mut s = Status {
            laeuft: self.lauf.is_some(),
            fehler: self.fehler.clone(),
            profil,
            profil_von_hand,
            programm,
            admin_problem,
            figur: self.figur.clone(),
            sounds_fehler: self.sounds.fehler(&self.e.sounds),
            sounds_laden: self.sounds.laedt(),
            stumm: self.stumm.load(Relaxed),
            umgebung: self.umgebung_jetzt(),
            umgebung_hotkey: self.umgebung_hotkey.is_some_and(|u| u.is_some()),
            aufnahme_s: self.aufnahme.as_ref().map(|(_, d)| d.len() as f64 / INTERN_RATE as f64),
            abhoeren: self.abhoeren.as_mut().is_some_and(|a| a.laeuft()),
            ..Default::default()
        };
        if let Some(l) = self.lauf.as_mut() {
            l.tw.soundboard.aufraeumen();
            s.ausgaenge = l
                .tw
                .ausgaenge
                .iter()
                .map(|a| AusgangStatus {
                    name: a.info.name.clone(),
                    latenz_ms: a.stats.latenz_fenster().map(|f| f.mittel_ms),
                    aussetzer: a.stats.aussetzer.load(Relaxed),
                })
                .collect();
            s.pegel_db = l.pegel.fenster().map(|(_, laut)| laut);
            // Spricht = Pegel über der Empfindlichkeit (ohne Gate: -45 dBFS).
            let g = self.e.mikro.gate_spiel;
            let schwelle = if g.an { g.schwelle_db } else { -45.0 };
            if s.pegel_db.is_some_and(|p| p >= schwelle) {
                self.spricht_bis = Instant::now() + Duration::from_millis(350);
            }
            let laufend = l.tw.soundboard.laufende();
            s.sounds_laufen = laufend.iter().filter_map(|&id| self.sounds.name(id).map(str::to_string)).collect();
            s.sounds_stand = l
                .tw
                .soundboard
                .fortschritt()
                .into_iter()
                .filter_map(|f| Some(SoundStand { name: self.sounds.name(f.id)?.to_string(), pos_s: f.pos_s, laenge_s: f.laenge_s, pausiert: f.pausiert }))
                .collect();
            // Kurz nach dem Start meldet das Mikrofon fast immer einen Überlauf.
            // Der zählt nicht (wie im Test-Programm).
            if let Some(f) = l.tw.eingang_stats.letzter_fehler.lock().ok().and_then(|mut f| f.take())
                && l.start.elapsed() > Duration::from_secs(3)
            {
                (self.melden)(Meldung::Hinweis(f));
            }
        }
        s.spricht = !s.stumm && s.laeuft && Instant::now() < self.spricht_bis;
        s
    }

    fn hotkey(&mut self, e: Ereignis) {
        match e {
            Ereignis::Aktion { aktion, .. } => self.aktion(aktion),
            Ereignis::Profil { name, von_hand, programm, admin_problem } => self.profil = (name, von_hand, programm, admin_problem),
            Ereignis::Gedrueckt(k) => (self.melden)(Meldung::Taste(k.to_string())),
            Ereignis::Fehler(f) => (self.melden)(Meldung::Hinweis(format!("Hotkeys: {f}"))),
        }
    }
}

/// Eigene Tonhöhe, solange sie nicht gemessen ist (typische Männerstimme).
const EIGENE_HZ_STANDARD: f32 = 120.0;

/// So oft prüft der Motor, ob die KI mitkommt.
const KI_PRUEFEN: Duration = Duration::from_secs(5);

/// Was eine neue KI-Stimme braucht: Modell und Mischung.
fn ki_schluessel(w: &KiWahl) -> String {
    let stimmen: Vec<String> = w.stimmen.iter().map(|a| format!("{}*{}", a.nr, a.gewicht)).collect();
    format!("{}|{}", w.modell, stimmen.join("+"))
}

/// Hotkey-Zustand nach einem Figurwechsel. Hat die neue Figur eine eigene
/// Umgebung, gilt sie (Wunsch Dan: die Figur bringt ihre Umgebung mit). Sonst
/// bleibt eine Umgebung per Hotkey an (z. B. Funk über den Wechsel hinweg).
/// „Aus per Hotkey“ gilt nur für die Figur, bei der es gedrückt wurde.
fn umgebung_nach_figurwechsel(hotkey: Option<Option<Umgebung>>, neue_figur: Option<Umgebung>) -> Option<Option<Umgebung>> {
    match hotkey {
        _ if neue_figur.is_some() => None,
        Some(None) => None,
        h => h,
    }
}

/// Neuer Hotkey-Zustand nach dem Drücken von `u` (`None` = „Umgebung aus“).
/// `hotkey`: bisheriger Zustand (siehe `Motor::umgebung_hotkey`), `figur`:
/// Umgebung der Figur. Ist `u` per Hotkey an, geht sie aus und die der Figur
/// gilt wieder. Ist `u` die Umgebung der Figur, geht sie aus. Ist das Ergebnis
/// das, was die Figur sowieso hat, folgt die Umgebung wieder der Figur.
fn umgebung_regel(hotkey: Option<Option<Umgebung>>, figur: Option<Umgebung>, u: Option<Umgebung>) -> Option<Option<Umgebung>> {
    let jetzt = hotkey.unwrap_or(figur);
    let neu = match u {
        Some(u) if hotkey == Some(Some(u)) => figur,
        Some(u) if jetzt == Some(u) => None,
        u => u,
    };
    if neu == figur { None } else { Some(neu) }
}

/// Griff auf den Motor. Löschen beendet ihn und speichert.
pub struct MotorGriff {
    tx: Sender<Auftrag>,
    faden: std::sync::Mutex<Option<JoinHandle<()>>>,
}

impl MotorGriff {
    /// Startet den Motor mit den Einstellungen aus `datei`.
    pub fn start(datei: PathBuf, melden: impl Fn(Meldung) + Send + 'static) -> (MotorGriff, Einstellungen) {
        let (e, meldung) = Einstellungen::laden(&datei);
        if let Some(m) = meldung {
            melden(Meldung::Hinweis(m));
        }
        let (tx, rx) = mpsc::channel();
        let tx_motor = tx.clone();
        let start_e = e.clone();
        let faden = std::thread::Builder::new()
            .name("maskcue-motor".into())
            .spawn(move || {
                let mut m = Motor {
                    e: start_e,
                    datei,
                    lauf: None,
                    fehler: None,
                    sounds: SoundVerwaltung::default(),
                    hotkeys: None,
                    profil: (PROFIL_AUS.into(), false, String::new(), false),
                    figur: None,
                    umgebung_hotkey: None,
                    aufnahme: None,
                    abhoeren: None,
                    ki_gesendet: None,
                    ki_laedt: None,
                    ki_modell: None,
                    ki_spaet: (0, Instant::now()),
                    ki_gewarnt: None,
                    tx: tx_motor.clone(),
                    speichern_ab: None,
                    ui: None,
                    stumm: Arc::new(AtomicBool::new(false)),
                    spricht_bis: Instant::now(),
                    melden: Box::new(melden),
                };
                let tx_h = tx_motor;
                match Hotkeys::start(m.e.hotkeys.clone(), move |e| {
                    let _ = tx_h.send(Auftrag::Hotkey(e));
                }) {
                    Ok(h) => m.hotkeys = Some(h),
                    Err(f) => (m.melden)(Meldung::Hinweis(format!("Hotkeys gehen nicht: {f}"))),
                }
                m.neu_starten();
                m.ui_abgleichen();
                let mut naechster_status = Instant::now();
                loop {
                    let warten = naechster_status.saturating_duration_since(Instant::now());
                    match rx.recv_timeout(warten) {
                        Ok(Auftrag::Einstellungen(e)) => m.einstellungen(*e),
                        Ok(Auftrag::Ausloesen(n)) => m.ausloesen(&n),
                        Ok(Auftrag::SoundPause(n)) => m.sound_pause(&n),
                        Ok(Auftrag::SoundStopp(n)) => m.sound_stopp(&n),
                        Ok(Auftrag::SoundSpringen(n, a)) => m.sound_springen(&n, a),
                        Ok(Auftrag::AllesStopp) => m.alles_stopp(),
                        Ok(Auftrag::Figur(f)) => m.figur_setzen(f),
                        Ok(Auftrag::Profil(p)) => {
                            if let Some(h) = &m.hotkeys {
                                h.profil_von_hand(p);
                            }
                        }
                        Ok(Auftrag::Lernen(an)) => {
                            if let Some(h) = &m.hotkeys {
                                h.lernen(an);
                            }
                        }
                        Ok(Auftrag::Neustart) => m.neu_starten(),
                        Ok(Auftrag::Geladen(datei, ergebnis)) => {
                            m.sounds.geladen(datei, ergebnis);
                            m.sounds_abgleichen();
                        }
                        Ok(Auftrag::Hotkey(e)) => m.hotkey(e),
                        Ok(Auftrag::Stumm(an)) => m.stumm_setzen(an),
                        Ok(Auftrag::Taste(k)) => {
                            if let Some(h) = &m.hotkeys {
                                h.taste_von_oberflaeche(k);
                            }
                        }
                        Ok(Auftrag::Umgebung(u)) => m.umgebung_umschalten(u),
                        Ok(Auftrag::Aufnahme(an)) => m.aufnahme_schalten(an),
                        Ok(Auftrag::Abhoeren(d)) => m.abhoeren(d),
                        Ok(Auftrag::AbhoerenGeladen(d)) => m.abhoeren_geladen(d),
                        Ok(Auftrag::KiGeladen(k, e)) => m.ki_geladen(k, e),
                        Ok(Auftrag::KiPaket(k, p)) => m.ki_paket(k, p),
                        Ok(Auftrag::KiProbe(w, h, f)) => m.ki_probe(w, h, f),
                        Ok(Auftrag::UiTon(t)) => {
                            if let Some(u) = m.ui.as_mut() {
                                u.spielen(t);
                            }
                        }
                        Ok(Auftrag::Ende) | Err(RecvTimeoutError::Disconnected) => break,
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                    m.aufnahme_abholen();
                    if Instant::now() >= naechster_status {
                        naechster_status = Instant::now() + STATUS_ALLE;
                        let s = m.status();
                        (m.melden)(Meldung::Status(s));
                        m.ki_pruefen();
                    }
                    if m.speichern_ab.is_some_and(|t| Instant::now() >= t) {
                        m.speichern();
                    }
                }
                if m.aufnahme.is_some() {
                    m.aufnahme_stopp();
                }
                if m.speichern_ab.is_some() {
                    m.speichern();
                }
                // Reihenfolge: erst Hotkeys, dann Ton.
                m.abhoeren = None;
                m.hotkeys = None;
                m.ui = None;
                m.lauf = None;
            })
            .expect("Motor-Faden");
        (MotorGriff { tx, faden: std::sync::Mutex::new(Some(faden)) }, e)
    }

    pub fn einstellungen(&self, e: Einstellungen) {
        let _ = self.tx.send(Auftrag::Einstellungen(Box::new(e)));
    }
    pub fn ausloesen(&self, name: String) {
        let _ = self.tx.send(Auftrag::Ausloesen(name));
    }
    pub fn sound_pause(&self, name: String) {
        let _ = self.tx.send(Auftrag::SoundPause(name));
    }
    pub fn sound_stopp(&self, name: String) {
        let _ = self.tx.send(Auftrag::SoundStopp(name));
    }
    pub fn sound_springen(&self, name: String, anteil: f32) {
        let _ = self.tx.send(Auftrag::SoundSpringen(name, anteil));
    }
    pub fn alles_stopp(&self) {
        let _ = self.tx.send(Auftrag::AllesStopp);
    }
    pub fn figur(&self, name: Option<String>) {
        let _ = self.tx.send(Auftrag::Figur(name));
    }
    pub fn ki_probe(&self, wahl: KiWahl, halbtoene: f32, formant: f32) {
        let _ = self.tx.send(Auftrag::KiProbe(wahl, halbtoene, formant));
    }
    pub fn profil(&self, name: Option<String>) {
        let _ = self.tx.send(Auftrag::Profil(name));
    }
    pub fn lernen(&self, an: bool) {
        let _ = self.tx.send(Auftrag::Lernen(an));
    }
    /// Stumm an (`Some(true)`), aus (`Some(false)`) oder umschalten (`None`).
    pub fn stumm(&self, an: Option<bool>) {
        let _ = self.tx.send(Auftrag::Stumm(an));
    }
    /// Taste aus der Oberfläche an die Hotkeys (MaskCue vorne).
    pub fn taste(&self, k: crate::hotkeys::taste::Kombination) {
        let _ = self.tx.send(Auftrag::Taste(k));
    }
    /// Aufnahme an, aus oder umschalten (`None`).
    pub fn aufnahme(&self, an: Option<bool>) {
        let _ = self.tx.send(Auftrag::Aufnahme(an));
    }
    /// Aufnahme anhören (`None` = stoppen).
    pub fn abhoeren(&self, datei: Option<PathBuf>) {
        let _ = self.tx.send(Auftrag::Abhoeren(datei));
    }
    /// Umgebung umschalten wie mit dem Hotkey (`None` = jede aus).
    pub fn umgebung(&self, u: Option<Umgebung>) {
        let _ = self.tx.send(Auftrag::Umgebung(u));
    }
    pub fn ui_ton(&self, t: Ton) {
        let _ = self.tx.send(Auftrag::UiTon(t));
    }
    pub fn neustart(&self) {
        let _ = self.tx.send(Auftrag::Neustart);
    }
}

impl MotorGriff {
    /// Beendet den Motor und speichert. Wartet, bis alles zu ist.
    pub fn beenden(&self) {
        let _ = self.tx.send(Auftrag::Ende);
        let faden = self.faden.lock().ok().and_then(|mut f| f.take());
        if let Some(f) = faden {
            let _ = f.join();
        }
    }
}

impl Drop for MotorGriff {
    fn drop(&mut self) {
        self.beenden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Umgebung::{Funk, Halle};

    #[test]
    fn umgebung_hotkey_bei_normaler_stimme() {
        // Funk an, nochmal: aus.
        let h = umgebung_regel(None, None, Some(Funk));
        assert_eq!(h, Some(Some(Funk)));
        assert_eq!(umgebung_regel(h, None, Some(Funk)), None);
        // „Umgebung aus“ ohne Umgebung ändert nichts.
        assert_eq!(umgebung_regel(None, None, None), None);
    }

    #[test]
    fn figur_bringt_ihre_umgebung_mit() {
        // Fehler von Dan: Funk global an, dann Figur mit eigener Umgebung gewählt: Sie kam nicht.
        assert_eq!(umgebung_nach_figurwechsel(Some(Some(Funk)), Some(Halle)), None);
        assert_eq!(umgebung_nach_figurwechsel(Some(None), Some(Halle)), None);
        // Neue Figur ohne eigene Umgebung: Funk per Hotkey bleibt.
        assert_eq!(umgebung_nach_figurwechsel(Some(Some(Funk)), None), Some(Some(Funk)));
        assert_eq!(umgebung_nach_figurwechsel(Some(None), None), None);
    }

    #[test]
    fn umgebung_hotkey_geht_vor_figur() {
        // Figur hat Halle. Funk per Hotkey ersetzt Halle, Funk nochmal: wieder Halle.
        let h = umgebung_regel(None, Some(Halle), Some(Funk));
        assert_eq!(h, Some(Some(Funk)));
        assert_eq!(umgebung_regel(h, Some(Halle), Some(Funk)), None);
        // Halle-Hotkey bei Halle-Figur: aus. Nochmal: wieder wie die Figur.
        let h = umgebung_regel(None, Some(Halle), Some(Halle));
        assert_eq!(h, Some(None));
        assert_eq!(umgebung_regel(h, Some(Halle), Some(Halle)), None);
        // „Umgebung aus“ schaltet auch die der Figur aus.
        assert_eq!(umgebung_regel(Some(Some(Funk)), Some(Halle), None), Some(None));
    }
}
