//! Soundboard: Sounds und Musik in die Ausgänge mischen.
//!
//! Zwei Teile:
//! - [`Mischer`] läuft im Ausgangs-Faden (Echtzeit). Er nimmt Befehle aus einer
//!   Warteschlange, addiert die Sounds zur Stimme, senkt Musik beim Sprechen
//!   (Ducking) und begrenzt am Ende den Pegel (Limiter). Kein Speicher, keine Sperren.
//! - [`Soundboard`] läuft in der App. Es hält die geladenen Sounds und schickt
//!   Befehle an die Mischer der gewählten Ausgänge.
//!
//! Sound-Daten wandern als `Arc` in den Mischer. Gibt der Mischer einen Sound
//! frei, schickt er den `Arc` zurück. So wird Speicher nie im Ton-Faden freigegeben.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};

use super::pipe::INTERN_RATE;
use super::sounddatei::SoundDaten;

/// Höchstens so viele Sounds gleichzeitig je Ausgang.
pub const MAX_SPUREN: usize = 32;
const BEFEHLE: usize = 256;

/// Was passiert, wenn ein Sound ausgelöst wird, der schon läuft.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nochmal {
    /// Stoppt den Sound (Standard).
    Stopp,
    /// Startet den Sound von vorn.
    NeuStarten,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundEinstellung {
    /// Lautstärke (1.0 = unverändert).
    pub lautstaerke: f32,
    pub schleife: bool,
    /// Musik wird beim Sprechen leiser (Ducking). Effekte nicht.
    pub musik: bool,
    pub nochmal: Nochmal,
    /// Je Ausgang: spielt der Sound dort? Standard: nur der erste (Spiel).
    pub ziele: [bool; 4],
}

impl Default for SoundEinstellung {
    fn default() -> Self {
        Self { lautstaerke: 1.0, schleife: false, musik: false, nochmal: Nochmal::Stopp, ziele: [true, false, false, false] }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MischerEinstellung {
    /// Musik so viel leiser, während gesprochen wird (dB, z. B. -12).
    pub ducking_db: f32,
    /// Ab diesem Stimm-Pegel gilt „spricht“ (dBFS).
    pub ducking_schwelle_db: f32,
    /// Obergrenze des Limiters (dBFS).
    pub limiter_db: f32,
}

impl Default for MischerEinstellung {
    fn default() -> Self {
        Self { ducking_db: -12.0, ducking_schwelle_db: -40.0, limiter_db: -1.0 }
    }
}

pub enum Befehl {
    Ausloesen { id: u32, daten: Arc<SoundDaten>, e: SoundEinstellung },
    Stopp { id: u32 },
    AllesStopp,
    Lautstaerke { id: u32, lautstaerke: f32 },
    /// Anhalten (`true`) oder weiterspielen (`false`). Blendet kurz aus bzw. ein.
    Pause { id: u32, an: bool },
    /// An diese Stelle springen (Frames ab Anfang).
    Springen { id: u32, pos: usize },
    Einstellen(MischerEinstellung),
}

/// Anzeige-Werte eines Mischers.
#[derive(Default)]
pub struct MischerAnzeige {
    pub spuren: AtomicU32,
    /// Absenkung durch den Limiter (dB × 100).
    pub limiter_cdb: AtomicU32,
    /// Aktuelle Ducking-Absenkung (dB × 100).
    pub ducking_cdb: AtomicU32,
    /// Nummern der laufenden Sounds (0 = Platz frei). Ausblendende zählen nicht.
    pub laufend: [AtomicU32; MAX_SPUREN],
    /// Stelle je Platz (Frames ab Anfang), passend zu `laufend`.
    pub pos: [AtomicU32; MAX_SPUREN],
    /// Angehalten je Platz.
    pub pausiert: [AtomicBool; MAX_SPUREN],
}

struct Spur {
    id: u32,
    daten: Arc<SoundDaten>,
    pos: usize,
    e: SoundEinstellung,
    /// Ein- und Ausblenden: aktuelle Stärke und Schritt pro Frame.
    blende: f32,
    blende_schritt: f32,
    endet: bool,
    /// Angehalten: blendet aus und bleibt dann an der Stelle stehen.
    pausiert: bool,
}

const EINBLENDEN_S: f32 = 0.002;
const AUSBLENDEN_S: f32 = 0.010;

fn db_zu_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn koeffizient(ms: f32) -> f32 {
    (-1.0 / (ms * 0.001 * INTERN_RATE as f32)).exp()
}

pub struct Mischer {
    befehle: rtrb::Consumer<Befehl>,
    rueckgabe: rtrb::Producer<Arc<SoundDaten>>,
    spuren: [Option<Spur>; MAX_SPUREN],
    e: MischerEinstellung,
    // Ducking
    stimm_huelle: f32,
    duck: f32,
    // Limiter
    limiter: f32,
    pub anzeige: Arc<MischerAnzeige>,
}

/// Steuerseite eines Mischers (gehört zum Soundboard).
pub struct MischerSteuerung {
    befehle: rtrb::Producer<Befehl>,
    rueckgabe: rtrb::Consumer<Arc<SoundDaten>>,
    pub anzeige: Arc<MischerAnzeige>,
}

pub fn mischer() -> (Mischer, MischerSteuerung) {
    let (btx, brx) = rtrb::RingBuffer::new(BEFEHLE);
    let (rtx, rrx) = rtrb::RingBuffer::new(BEFEHLE + MAX_SPUREN);
    let anzeige = Arc::new(MischerAnzeige::default());
    let m = Mischer {
        befehle: brx,
        rueckgabe: rtx,
        spuren: Default::default(),
        e: MischerEinstellung::default(),
        stimm_huelle: 0.0,
        duck: 1.0,
        limiter: 1.0,
        anzeige: anzeige.clone(),
    };
    (m, MischerSteuerung { befehle: btx, rueckgabe: rrx, anzeige })
}

impl Mischer {
    fn freigeben(&mut self, daten: Arc<SoundDaten>) {
        // Zurück an die App. Ist die Rückgabe voll, lieber Speicher verlieren
        // als ihn im Ton-Faden freizugeben.
        if let Err(rtrb::PushError::Full(d)) = self.rueckgabe.push(daten) {
            std::mem::forget(d);
        }
    }

    fn ausblenden(spur: &mut Spur) {
        spur.endet = true;
        spur.blende_schritt = -1.0 / (AUSBLENDEN_S * INTERN_RATE as f32);
    }

    fn befehle_abarbeiten(&mut self) {
        while let Ok(b) = self.befehle.pop() {
            match b {
                Befehl::Ausloesen { id, daten, e } => {
                    let laeuft = self.spuren.iter_mut().flatten().find(|s| s.id == id && !s.endet);
                    match (laeuft, e.nochmal) {
                        (Some(s), Nochmal::Stopp) => {
                            Self::ausblenden(s);
                            self.freigeben(daten);
                        }
                        (Some(s), Nochmal::NeuStarten) => {
                            s.pos = 0;
                            s.blende = 0.0;
                            s.blende_schritt = 1.0 / (EINBLENDEN_S * INTERN_RATE as f32);
                            s.e = e;
                            s.pausiert = false;
                            self.freigeben(daten);
                        }
                        (None, _) => {
                            let neu = Spur {
                                id,
                                daten,
                                pos: 0,
                                e,
                                blende: 0.0,
                                blende_schritt: 1.0 / (EINBLENDEN_S * INTERN_RATE as f32),
                                endet: false,
                                pausiert: false,
                            };
                            match self.spuren.iter_mut().find(|s| s.is_none()) {
                                Some(platz) => *platz = Some(neu),
                                // Alle Plätze belegt: Sound wird nicht gespielt.
                                None => self.freigeben(neu.daten),
                            }
                        }
                    }
                }
                Befehl::Stopp { id } => {
                    for s in self.spuren.iter_mut().flatten().filter(|s| s.id == id) {
                        Self::ausblenden(s);
                    }
                }
                Befehl::AllesStopp => {
                    for s in self.spuren.iter_mut().flatten() {
                        Self::ausblenden(s);
                    }
                }
                Befehl::Lautstaerke { id, lautstaerke } => {
                    for s in self.spuren.iter_mut().flatten().filter(|s| s.id == id) {
                        s.e.lautstaerke = lautstaerke;
                    }
                }
                Befehl::Pause { id, an } => {
                    for s in self.spuren.iter_mut().flatten().filter(|s| s.id == id && !s.endet) {
                        s.pausiert = an;
                        let schritt = if an { -1.0 / (AUSBLENDEN_S * INTERN_RATE as f32) } else { 1.0 / (EINBLENDEN_S * INTERN_RATE as f32) };
                        s.blende_schritt = schritt;
                    }
                }
                Befehl::Springen { id, pos } => {
                    for s in self.spuren.iter_mut().flatten().filter(|s| s.id == id && !s.endet) {
                        s.pos = pos.min(s.daten.frames());
                        // Kurz neu einblenden, sonst knackt der Sprung.
                        if !s.pausiert {
                            s.blende = 0.0;
                            s.blende_schritt = 1.0 / (EINBLENDEN_S * INTERN_RATE as f32);
                        }
                    }
                }
                Befehl::Einstellen(e) => self.e = e,
            }
        }
    }

    /// Mischt alle laufenden Sounds in `data` (verschachtelt, `kanaele` Kanäle).
    /// In `data` liegt schon die Stimme. Danach läuft der Limiter.
    pub fn mische(&mut self, data: &mut [f32], kanaele: usize) {
        self.befehle_abarbeiten();
        let kanaele = kanaele.max(1);
        let frames = data.len() / kanaele;

        // Ducking: Pegel der Stimme (vor den Sounds) verfolgen.
        let k_huelle_auf = koeffizient(10.0);
        let k_huelle_ab = koeffizient(300.0);
        let k_duck_runter = koeffizient(80.0);
        let k_duck_hoch = koeffizient(600.0);
        let schwelle = db_zu_lin(self.e.ducking_schwelle_db);
        let tiefe = db_zu_lin(self.e.ducking_db);
        let musik_laeuft = self.spuren.iter().flatten().any(|s| s.e.musik);

        for f in 0..frames {
            let frame = &mut data[f * kanaele..(f + 1) * kanaele];
            if musik_laeuft {
                let stimme = frame.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                let k = if stimme > self.stimm_huelle { k_huelle_auf } else { k_huelle_ab };
                self.stimm_huelle = stimme + (self.stimm_huelle - stimme) * k;
                let ziel = if self.stimm_huelle > schwelle { tiefe } else { 1.0 };
                let k = if ziel < self.duck { k_duck_runter } else { k_duck_hoch };
                self.duck = ziel + (self.duck - ziel) * k;
            }
            for spur in self.spuren.iter_mut().flatten() {
                let d = &spur.daten;
                let sk = d.kanaele;
                let gesamt = d.frames();
                if spur.pos >= gesamt {
                    if spur.e.schleife && !spur.endet {
                        spur.pos = 0;
                    } else {
                        continue;
                    }
                }
                spur.blende = (spur.blende + spur.blende_schritt).clamp(0.0, 1.0);
                // Angehalten und ausgeblendet: Stelle bleibt stehen.
                if spur.pausiert && spur.blende <= 0.0 {
                    continue;
                }
                let g = spur.e.lautstaerke * spur.blende * if spur.e.musik { self.duck } else { 1.0 };
                let i = spur.pos * sk;
                let (l, r) = if sk >= 2 { (d.samples[i], d.samples[i + 1]) } else { (d.samples[i], d.samples[i]) };
                match kanaele {
                    1 => frame[0] += g * 0.5 * (l + r),
                    _ => {
                        frame[0] += g * l;
                        frame[1] += g * r;
                        for c in frame.iter_mut().skip(2) {
                            *c += g * 0.5 * (l + r);
                        }
                    }
                }
                spur.pos += 1;
            }
        }

        // Fertige Spuren freigeben.
        for platz in self.spuren.iter_mut() {
            let fertig = platz.as_ref().is_some_and(|s| (s.endet && s.blende <= 0.0) || (!s.e.schleife && s.pos >= s.daten.frames()));
            if fertig {
                let s = platz.take().expect("geprüft");
                if let Err(rtrb::PushError::Full(d)) = self.rueckgabe.push(s.daten) {
                    std::mem::forget(d);
                }
            }
        }

        // Limiter: sofort runter, wenn eine Spitze die Grenze überschreitet,
        // in ca. 80 ms wieder hoch. Danach hart begrenzen (Sicherheit).
        let grenze = db_zu_lin(self.e.limiter_db);
        let k_frei = koeffizient(80.0);
        let mut tiefste = 1.0f32;
        for frame in data.chunks_exact_mut(kanaele) {
            let spitze = frame.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            let noetig = if spitze * self.limiter > grenze { grenze / spitze } else { 1.0 };
            self.limiter = if noetig < self.limiter { noetig } else { 1.0 + (self.limiter - 1.0) * k_frei };
            for v in frame.iter_mut() {
                *v = (*v * self.limiter).clamp(-grenze, grenze);
            }
            tiefste = tiefste.min(self.limiter);
        }

        let a = &self.anzeige;
        a.spuren.store(self.spuren.iter().flatten().count() as u32, Relaxed);
        for (i, spur) in self.spuren.iter().enumerate() {
            let spur = spur.as_ref().filter(|s| !s.endet);
            a.laufend[i].store(spur.map_or(0, |s| s.id), Relaxed);
            a.pos[i].store(spur.map_or(0, |s| s.pos.min(u32::MAX as usize) as u32), Relaxed);
            a.pausiert[i].store(spur.is_some_and(|s| s.pausiert), Relaxed);
        }
        a.limiter_cdb.store((-20.0 * tiefste.max(1e-6).log10() * 100.0) as u32, Relaxed);
        a.ducking_cdb.store((-20.0 * self.duck.max(1e-6).log10() * 100.0) as u32, Relaxed);
    }
}

/// Stand eines laufenden Sounds für die Anzeige.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fortschritt {
    pub id: u32,
    pub pos_s: f32,
    pub laenge_s: f32,
    pub pausiert: bool,
}

struct Eintrag {
    daten: Arc<SoundDaten>,
    e: SoundEinstellung,
}

/// Steuert die Mischer aller Ausgänge.
pub struct Soundboard {
    mischer: Vec<MischerSteuerung>,
    sounds: HashMap<u32, Eintrag>,
    naechste_id: u32,
}

impl Soundboard {
    pub fn new(mischer: Vec<MischerSteuerung>) -> Self {
        Self { mischer, sounds: HashMap::new(), naechste_id: 1 }
    }

    pub fn anzeige(&self, ausgang: usize) -> Option<&MischerAnzeige> {
        self.mischer.get(ausgang).map(|m| &*m.anzeige)
    }

    /// Sound aufnehmen. Gibt seine Nummer zurück.
    pub fn hinzufuegen(&mut self, daten: SoundDaten, e: SoundEinstellung) -> u32 {
        self.hinzufuegen_geteilt(Arc::new(daten), e)
    }

    /// Wie [`Self::hinzufuegen`], für schon geladene Daten (z. B. nach einem
    /// Neustart des Tonwegs, ohne die Datei neu zu laden).
    pub fn hinzufuegen_geteilt(&mut self, daten: Arc<SoundDaten>, e: SoundEinstellung) -> u32 {
        let id = self.naechste_id;
        self.naechste_id += 1;
        self.sounds.insert(id, Eintrag { daten, e });
        id
    }

    pub fn einstellen(&mut self, id: u32, e: SoundEinstellung) {
        if let Some(s) = self.sounds.get_mut(&id) {
            s.e = e;
        }
        self.an_alle(|| Befehl::Lautstaerke { id, lautstaerke: e.lautstaerke });
    }

    pub fn entfernen(&mut self, id: u32) {
        self.stopp(id);
        self.sounds.remove(&id);
    }

    /// Sound starten (oder, wenn er läuft, stoppen bzw. neu starten).
    pub fn ausloesen(&mut self, id: u32) {
        self.aufraeumen();
        let Some(s) = self.sounds.get(&id) else { return };
        for (i, m) in self.mischer.iter_mut().enumerate() {
            if s.e.ziele.get(i).copied().unwrap_or(false) {
                let _ = m.befehle.push(Befehl::Ausloesen { id, daten: s.daten.clone(), e: s.e });
            }
        }
    }

    pub fn stopp(&mut self, id: u32) {
        self.an_alle(|| Befehl::Stopp { id });
    }

    /// Laufenden Sound anhalten oder weiterspielen (umschalten).
    pub fn pause(&mut self, id: u32) {
        let an = !self.fortschritt().iter().any(|f| f.id == id && f.pausiert);
        self.an_alle(|| Befehl::Pause { id, an });
    }

    /// Laufenden Sound an eine Stelle setzen (`anteil` 0…1 der Länge).
    pub fn springen(&mut self, id: u32, anteil: f32) {
        let Some(s) = self.sounds.get(&id) else { return };
        let pos = (anteil.clamp(0.0, 1.0) as f64 * s.daten.frames() as f64) as usize;
        self.an_alle(|| Befehl::Springen { id, pos });
    }

    /// Stelle, Länge (Sekunden) und Pause der laufenden Sounds. Bei mehreren
    /// Ausgängen zählt der erste, in dem der Sound läuft.
    pub fn fortschritt(&self) -> Vec<Fortschritt> {
        let mut v: Vec<Fortschritt> = Vec::new();
        for m in &self.mischer {
            let a = &m.anzeige;
            for i in 0..MAX_SPUREN {
                let id = a.laufend[i].load(Relaxed);
                if id == 0 || v.iter().any(|f| f.id == id) {
                    continue;
                }
                let Some(s) = self.sounds.get(&id) else { continue };
                v.push(Fortschritt {
                    id,
                    pos_s: a.pos[i].load(Relaxed) as f32 / INTERN_RATE as f32,
                    laenge_s: s.daten.frames() as f32 / INTERN_RATE as f32,
                    pausiert: a.pausiert[i].load(Relaxed),
                });
            }
        }
        v.sort_unstable_by_key(|f| f.id);
        v
    }

    /// Notbremse: alles sofort ausblenden.
    pub fn alles_stopp(&mut self) {
        self.an_alle(|| Befehl::AllesStopp);
    }

    pub fn mischer_einstellen(&mut self, e: MischerEinstellung) {
        self.an_alle(|| Befehl::Einstellen(e));
    }

    fn an_alle(&mut self, befehl: impl Fn() -> Befehl) {
        for m in self.mischer.iter_mut() {
            let _ = m.befehle.push(befehl());
        }
    }

    /// Nummern aller Sounds, die gerade in irgendeinem Ausgang laufen.
    pub fn laufende(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.mischer.iter().flat_map(|m| m.anzeige.laufend.iter().map(|a| a.load(Relaxed))).filter(|&id| id != 0).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Gibt zurückgeschickte Sound-Daten frei (außerhalb des Ton-Fadens).
    pub fn aufraeumen(&mut self) {
        for m in self.mischer.iter_mut() {
            while m.rueckgabe.pop().is_ok() {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ton(sekunden: f32, wert: f32, kanaele: usize) -> SoundDaten {
        let n = (sekunden * INTERN_RATE as f32) as usize * kanaele;
        SoundDaten { name: "test".into(), kanaele, samples: vec![wert; n] }
    }

    fn rechne(m: &mut Mischer, sekunden: f32) -> Vec<f32> {
        let mut alles = Vec::new();
        for _ in 0..(sekunden * 100.0) as usize {
            let mut b = vec![0.0f32; 480 * 2];
            m.mische(&mut b, 2);
            alles.extend(b);
        }
        alles
    }

    fn board() -> (Soundboard, Mischer) {
        let (m, s) = mischer();
        (Soundboard::new(vec![s]), m)
    }

    #[test]
    fn sound_spielt_und_endet() {
        let (mut sb, mut m) = board();
        let id = sb.hinzufuegen(ton(0.1, 0.25, 1), SoundEinstellung::default());
        sb.ausloesen(id);
        let x = rechne(&mut m, 0.2);
        // Nach dem Einblenden voll da (beide Kanäle), nach 0,1 s still.
        assert!((x[2 * 1000] - 0.25).abs() < 1e-6 && (x[2 * 1000 + 1] - 0.25).abs() < 1e-6);
        assert!(x[2 * 6000..].iter().all(|&v| v == 0.0));
        assert_eq!(m.anzeige.spuren.load(Relaxed), 0);
    }

    #[test]
    fn laufende_sounds_fuer_die_anzeige() {
        let (mut sb, mut m) = board();
        let a = sb.hinzufuegen(ton(5.0, 0.1, 1), SoundEinstellung::default());
        let b = sb.hinzufuegen(ton(0.05, 0.1, 1), SoundEinstellung::default());
        sb.ausloesen(a);
        sb.ausloesen(b);
        rechne(&mut m, 0.01);
        assert_eq!(sb.laufende(), vec![a, b]);
        rechne(&mut m, 0.1);
        assert_eq!(sb.laufende(), vec![a]);
        // Beim Ausblenden gilt der Sound schon als gestoppt.
        sb.stopp(a);
        rechne(&mut m, 0.01);
        assert!(sb.laufende().is_empty());
    }

    #[test]
    fn nochmal_druecken_stoppt() {
        let (mut sb, mut m) = board();
        let id = sb.hinzufuegen(ton(5.0, 0.25, 1), SoundEinstellung::default());
        sb.ausloesen(id);
        rechne(&mut m, 0.1);
        sb.ausloesen(id);
        let x = rechne(&mut m, 0.1);
        // Ausblenden in 10 ms, danach still.
        assert!(x[2 * 960..].iter().all(|&v| v == 0.0));
        assert_eq!(m.anzeige.spuren.load(Relaxed), 0);
    }

    #[test]
    fn nochmal_druecken_startet_neu() {
        let (mut sb, mut m) = board();
        let e = SoundEinstellung { nochmal: Nochmal::NeuStarten, ..Default::default() };
        let id = sb.hinzufuegen(ton(0.3, 0.25, 1), e);
        sb.ausloesen(id);
        rechne(&mut m, 0.2);
        sb.ausloesen(id);
        // Neu gestartet: spielt noch volle 0,3 s, nicht nur die restlichen 0,1 s.
        let x = rechne(&mut m, 0.25);
        assert!((x[2 * 11_000] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn schleife_und_mehrere_gleichzeitig() {
        let (mut sb, mut m) = board();
        let a = sb.hinzufuegen(ton(0.05, 0.1, 1), SoundEinstellung { schleife: true, ..Default::default() });
        let b = sb.hinzufuegen(ton(1.0, 0.2, 2), SoundEinstellung::default());
        sb.ausloesen(a);
        sb.ausloesen(b);
        let x = rechne(&mut m, 0.5);
        // Nach 0,4 s: Schleife läuft noch, beide addiert.
        assert!((x[2 * 19_200] - 0.3).abs() < 1e-5, "{}", x[2 * 19_200]);
        assert_eq!(m.anzeige.spuren.load(Relaxed), 2);
        sb.alles_stopp();
        let x = rechne(&mut m, 0.1);
        assert!(x[2 * 960..].iter().all(|&v| v == 0.0));
    }

    #[test]
    fn nur_gewaehlte_ausgaenge() {
        let (m1, s1) = mischer();
        let (m2, s2) = mischer();
        let (mut m1, mut m2) = (m1, m2);
        let mut sb = Soundboard::new(vec![s1, s2]);
        let nur_spiel = sb.hinzufuegen(ton(1.0, 0.1, 1), SoundEinstellung::default());
        let beide = sb.hinzufuegen(ton(1.0, 0.2, 1), SoundEinstellung { ziele: [true, true, false, false], ..Default::default() });
        sb.ausloesen(nur_spiel);
        sb.ausloesen(beide);
        let x1 = rechne(&mut m1, 0.1);
        let x2 = rechne(&mut m2, 0.1);
        assert!((x1[2000] - 0.3).abs() < 1e-5);
        assert!((x2[2000] - 0.2).abs() < 1e-5);
    }

    #[test]
    fn musik_wird_beim_sprechen_leiser() {
        let (mut sb, mut m) = board();
        let id = sb.hinzufuegen(ton(3.0, 0.1, 1), SoundEinstellung { musik: true, schleife: true, ..Default::default() });
        sb.ausloesen(id);
        rechne(&mut m, 0.5);
        // Stimme (0,1 ≈ -20 dBFS) 1 s lang: Musik fällt um ca. 12 dB.
        let mut letzter = vec![0.0f32; 960];
        for _ in 0..100 {
            let mut b = vec![0.0f32; 960];
            for (i, v) in b.iter_mut().enumerate() {
                *v = if i % 2 == 0 { 0.1 } else { -0.1 };
            }
            m.mische(&mut b, 2);
            letzter = b;
        }
        let musik = (letzter[0] - 0.1).abs().max((letzter[1] + 0.1).abs());
        let db = 20.0 * (musik / 0.1).log10();
        assert!((db + 12.0).abs() < 1.0, "Musik {db} dB");
        // Effekte (musik: false) werden nicht gesenkt.
        let effekt = sb.hinzufuegen(ton(1.0, 0.1, 1), SoundEinstellung::default());
        sb.ausloesen(effekt);
        let mut b = vec![0.1f32; 960];
        m.mische(&mut b, 2);
        let mut b = vec![0.1f32; 960];
        m.mische(&mut b, 2);
        assert!(b[500] > 0.1 + 0.1 * 0.95, "{}", b[500]);
    }

    #[test]
    fn limiter_haelt_die_grenze() {
        let (mut sb, mut m) = board();
        for _ in 0..5 {
            let id = sb.hinzufuegen(ton(1.0, 0.5, 1), SoundEinstellung::default());
            sb.ausloesen(id);
        }
        let x = rechne(&mut m, 0.5);
        let grenze = db_zu_lin(-1.0);
        assert!(x.iter().all(|v| v.abs() <= grenze + 1e-6));
        // Gleichmäßig gedämpft, nicht nur abgeschnitten: Wert liegt an der Grenze.
        assert!((x[2 * 10_000] - grenze).abs() < 1e-3);
        assert!(m.anzeige.limiter_cdb.load(Relaxed) > 700);
    }

    #[test]
    fn pause_haelt_an_und_spielt_weiter() {
        let (mut sb, mut m) = board();
        let id = sb.hinzufuegen(ton(1.0, 0.25, 1), SoundEinstellung::default());
        sb.ausloesen(id);
        rechne(&mut m, 0.2);
        sb.pause(id);
        let x = rechne(&mut m, 0.3);
        // Nach dem Ausblenden (10 ms) still, Stelle bleibt bei ca. 0,2 s.
        assert!(x[2 * 960..].iter().all(|&v| v == 0.0));
        let f = sb.fortschritt();
        assert_eq!(f.len(), 1);
        assert!(f[0].pausiert && (f[0].pos_s - 0.21).abs() < 0.02, "{f:?}");
        assert!((f[0].laenge_s - 1.0).abs() < 1e-3);
        // Angehalten zählt weiter als laufend (Pad bleibt markiert).
        assert_eq!(sb.laufende(), vec![id]);
        sb.pause(id);
        let x = rechne(&mut m, 0.1);
        assert!((x[2 * 1000] - 0.25).abs() < 1e-6);
        assert!(!sb.fortschritt()[0].pausiert);
    }

    #[test]
    fn springen_setzt_die_stelle() {
        let (mut sb, mut m) = board();
        let id = sb.hinzufuegen(ton(2.0, 0.25, 1), SoundEinstellung::default());
        sb.ausloesen(id);
        rechne(&mut m, 0.1);
        sb.springen(id, 0.5);
        rechne(&mut m, 0.1);
        let f = sb.fortschritt();
        assert!((f[0].pos_s - 1.1).abs() < 0.02, "{f:?}");
        // Ans Ende springen: Sound ist fertig.
        sb.springen(id, 1.0);
        rechne(&mut m, 0.05);
        assert!(sb.fortschritt().is_empty());
    }

    #[test]
    fn speicher_kommt_zurueck() {
        let (mut sb, mut m) = board();
        let id = sb.hinzufuegen(ton(0.05, 0.1, 1), SoundEinstellung::default());
        for _ in 0..10 {
            sb.ausloesen(id);
            rechne(&mut m, 0.1);
        }
        sb.aufraeumen();
        // Nur das Soundboard hält den Sound noch.
        assert_eq!(Arc::strong_count(&sb.sounds[&id].daten), 1);
    }
}
