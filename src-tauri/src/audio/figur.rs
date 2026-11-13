//! Figur-Stimme, die sich im Lauf umschalten lässt (Hotkey).
//!
//! Drei Verfahren für Tonhöhe und Formant:
//! - PSOLA (eigenes, `psola.rs`): ca. 31 ms Verzögerung. Gut bis etwa
//!   7 Halbtöne; bei einer Oktave tiefer klingt es leicht roboterhaft.
//! - Signalsmith Stretch (`stimme.rs`): ca. 50 ms. Natürlicher bei großen
//!   Sprüngen (gemessen bei −12: Pulsen 0,45 wie die echte Stimme, PSOLA 0,52).
//! - WSOLA mit Neuabtastung (eigenes, `wsola.rs`): im Mittel 10–16 ms, sehr
//!   wenig Rechenzeit. Hält den Formant nicht (Helium-Klang).
//!
//! - Ohne: Stimme bleibt, 0 ms. Nur Hell/Dunkel und Umgebung wirken.
//!
//! Davor kann eine KI-Stimme laufen (`ki/`, Beatrice 2): andere Person statt
//! nur andere Tonhöhe, 74,5 ms mit Vorgriff. Braucht ein Modell und ca.
//! 20–30 % eines Prozessor-Kerns (eigener Faden). Das Verfahren arbeitet dann
//! auf der KI-Stimme (Dan, 21.10.2026: KI und Verfahren kombinieren); die
//! Verzögerungen zählen zusammen. Mit „Ohne“ stellt die KI selbst Tonhöhe und
//! Formant ein.
//!
//! Es rechnet nur das Verfahren, das man gerade hört. Die anderen ruhen, bei
//! „Figur aus“ alle. So spart „Leicht“ (WSOLA) auf schwachen PCs wirklich
//! Rechenzeit. Ein gewecktes Verfahren startet leer (ohne alte Samples) und läuft
//! erst warm: seine Verzögerung plus Anlauf, auf Blöcke gerundet (PSOLA und
//! Signalsmith 60 ms, WSOLA 30 ms). So lange klingt die
//! bisherige Stimme weiter, dann wird umgeschaltet. Die KI-Stimme läuft genauso
//! warm: Der Motor lädt sie im Hintergrund, hier läuft sie erst ein paar Blöcke
//! mit. „Figur aus“ gibt die trockene Stimme ohne Verzögerung aus. Beim
//! Umschalten blendet der Effekt über einen Block (10 ms) über, damit es nicht
//! knackt.

use serde::{Deserialize, Serialize};

use super::eq::{Band, Equalizer};
use super::ki::beatrice::StimmPaket;
use super::ki::{self, KiStimme, KiWahl};
use super::psola::{Psola, PsolaEinstellung};
use super::stimme::{StimmEinstellung, Stimme};
use super::umgebung::Umgebung;
use super::verarbeitung::Effekt;
use super::wsola::{self, Wsola, WsolaEinstellung};

/// Verfahren für Tonhöhe und Formant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verfahren {
    /// Eigenes PSOLA: wenig Verzögerung.
    #[default]
    Psola,
    /// Signalsmith Stretch: natürlicher bei großen Sprüngen, mehr Verzögerung.
    Signalsmith,
    /// WSOLA mit Neuabtastung: wenig Verzögerung und Rechenzeit, Helium-Klang
    /// (ohne Formant).
    Wsola,
    /// Ohne Tonhöhe und Formant: 0 ms. Es wirken nur Hell/Dunkel und die
    /// Umgebung (z. B. Funkgerät mit eigener Stimme).
    Ohne,
}

/// Was der Effekt für eine Figur braucht.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FigurStimme {
    pub halbtoene: f32,
    pub formant: f32,
    pub verfahren: Verfahren,
    /// Nur KI: Laut-Anteil des Ziel-Sprechers (0…1).
    pub laut_anteil: f32,
    /// Hell (+) oder dunkel (−) in dB, nach der Stimme.
    pub hell: f32,
    /// KI-Stimme vor dem Verfahren (`None` = keine).
    pub ki: Option<KiTon>,
}

/// Tonhöhe und Formant für die KI-Stimme selbst.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KiTon {
    pub halbtoene: f32,
    pub formant: f32,
}

/// Fenster von Signalsmith (ms). Kleiner erzeugt falsche Töne (PLAN.md).
const SIGNALSMITH_FENSTER_MS: f32 = 40.0;

/// Ausschnitt eines eigenen Bildes (nicht quadratisch, oder nur ein Teil).
/// Das Bild selbst bleibt unverändert; die Oberfläche zeigt nur den Ausschnitt.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ausschnitt {
    /// Mitte des Ausschnitts in Prozent der Breite (0…100).
    pub x: f32,
    /// Mitte des Ausschnitts in Prozent der Höhe (0…100).
    pub y: f32,
    /// Vergrößerung (1 = ganzes Bild, bis 4).
    pub zoom: f32,
}

impl Ausschnitt {
    /// Werte in den erlaubten Bereich bringen (auch aus fremden Dateien).
    pub fn sauber(self) -> Option<Ausschnitt> {
        let ok = |v: f32| v.is_finite();
        (ok(self.x) && ok(self.y) && ok(self.zoom)).then(|| Ausschnitt { x: self.x.clamp(0.0, 100.0), y: self.y.clamp(0.0, 100.0), zoom: self.zoom.clamp(1.0, 4.0) })
    }
}

/// Bild-Wert "farbe:#rrggbb" gültig?
pub fn farbe_gueltig(bild: &str) -> bool {
    bild.strip_prefix("farbe:#").is_some_and(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Eine Figur: Name und Stimme.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FigurDef {
    pub name: String,
    /// Tonhöhe in Halbtönen (−15 bis +30). Bei der KI-Stimme mit automatischer
    /// Tonhöhe: Feinabstimmung (eigene Grenzen, siehe `ki`).
    pub halbtoene: f32,
    /// Formant (Klangfarbe) in Halbtönen (−4 bis +12).
    #[serde(default)]
    pub formant: f32,
    /// Bild der Figur: "avatar:<name>" (mitgeliefert), "farbe:#rrggbb" oder "datei:<name>"
    /// (eigenes Bild in `%APPDATA%\MaskCueilder`). Fehlt es, zeigt die
    /// Oberfläche eine gemalte Szene.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bild: Option<String>,
    /// Ausschnitt des eigenen Bildes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bild_ausschnitt: Option<Ausschnitt>,
    /// Feste Umgebung der Figur (z. B. Höhle). Ein Umgebungs-Hotkey
    /// geht vor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub umgebung: Option<Umgebung>,
    /// Eigene Stärke der Umgebung für diese Figur (0…1). Fehlt sie, gilt die
    /// Stärke aus der Seite „Umgebungen“.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub umgebung_staerke: Option<f32>,
    /// Verfahren für Tonhöhe und Formant (Standard PSOLA).
    #[serde(default, skip_serializing_if = "ist_psola")]
    pub verfahren: Verfahren,
    /// KI-Stimme: Modell und Stimme (gilt, wenn `ki_an`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ki: Option<KiWahl>,
    /// KI-Stimme vor dem Verfahren an.
    #[serde(default, skip_serializing_if = "ist_aus")]
    pub ki_an: bool,
    /// Hell (+) oder dunkel (−), −6 … +6 dB. Wirkt nach der Stimme.
    #[serde(default, skip_serializing_if = "ist_null")]
    pub hell: f32,
}

fn ist_null(v: &f32) -> bool {
    *v == 0.0
}

fn ist_aus(v: &bool) -> bool {
    !*v
}

fn ist_psola(v: &Verfahren) -> bool {
    *v == Verfahren::Psola
}

impl FigurDef {
    /// Einstellung für den Effekt. Mit KI und „Ohne“ bekommt die KI Tonhöhe
    /// und Formant der Figur (Feinabstimmung; die automatische Tonhöhe rechnet
    /// der Motor dazu), sonst das Verfahren.
    pub fn einstellung(&self) -> FigurStimme {
        let laut_anteil = self.ki.as_ref().map_or(1.0, |k| k.laut_anteil);
        let ki_an = self.ki_an && self.ki.is_some();
        let fein = if self.halbtoene.is_finite() { self.halbtoene } else { 0.0 };
        let ki = ki_an.then(|| if self.verfahren == Verfahren::Ohne { KiTon { halbtoene: fein, formant: self.formant } } else { KiTon { halbtoene: 0.0, formant: 0.0 } });
        // Werte aus der Datei können außerhalb liegen.
        let halbtoene = fein.clamp(wsola::HALBTOENE_MIN, wsola::HALBTOENE_MAX);
        FigurStimme { halbtoene, formant: self.formant, verfahren: self.verfahren, laut_anteil, hell: self.hell.clamp(-6.0, 6.0), ki }
    }
}

enum Befehl {
    Stimme(Option<FigurStimme>),
    /// Neue KI-Stimme (fertig geladen und eingestellt).
    Ki(Box<KiStimme>),
    /// Neue Mischung für die laufende KI-Stimme (gleiches Modell).
    KiPaket(Box<StimmPaket>),
}

/// Schickt den Figur-Wechsel in den Verarbeitungs-Faden. `None` = Figur aus.
pub struct FigurSteuerung {
    tx: rtrb::Producer<Befehl>,
}

impl FigurSteuerung {
    /// Gibt `false` zurück, wenn die Warteschlange voll ist.
    pub fn setzen(&mut self, e: Option<FigurStimme>) -> bool {
        self.tx.push(Befehl::Stimme(e)).is_ok()
    }

    /// Fertige KI-Stimme übergeben. Sie läuft erst warm, dann wird umgeschaltet.
    pub fn ki(&mut self, k: Box<KiStimme>) -> bool {
        self.tx.push(Befehl::Ki(k)).is_ok()
    }

    /// Neue Mischung für die laufende KI-Stimme. Sie gilt sofort, das Netz
    /// läuft weiter.
    pub fn ki_paket(&mut self, p: Box<StimmPaket>) -> bool {
        self.tx.push(Befehl::KiPaket(p)).is_ok()
    }
}

// Eine neue KI-Stimme läuft erst `KiStimme::warm_bloecke()` Blöcke mit, bevor
// man sie hört. Beatrice 30: Die ersten Bilder kommen nach 4 Blöcken; danach
// füllen sich die Faltungen (bis 33 Bilder lang), vorher klingt sie leiser
// (gemessen mit examples/ki_figur.rs).

/// Warmlaufen der anderen Verfahren: ihre Verzögerung plus Anlauf (siehe
/// `Figur::anlauf`), aufgerundet auf Blöcke (Test `geweckt_ist_gleich_voll_da`).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Quelle {
    Trocken,
    /// Figur an, aber ohne Verfahren: der trockene Block.
    Ohne,
    Psola,
    Signalsmith,
    Wsola,
}

pub struct Figur {
    psola: Psola,
    signalsmith: Stimme,
    wsola: Wsola,
    ki: Option<Box<KiStimme>>,
    /// Neue KI-Stimme, die gerade warm läuft (Blöcke bisher).
    ki_neu: Option<(Box<KiStimme>, usize)>,
    rx: rtrb::Consumer<Befehl>,
    /// Gewünschtes Verfahren.
    wunsch: Quelle,
    /// KI-Stimme vor dem Verfahren gewünscht, und ob sie schon zu hören ist
    /// (erst, wenn sie bereit ist).
    ki_wunsch: bool,
    ki_hoerbar: bool,
    quelle: Quelle,
    /// Gewecktes Verfahren, das gerade warm läuft (Blöcke bisher).
    warm: Option<(Quelle, usize)>,
    letzte: Option<FigurStimme>,
    trocken: Vec<f32>,
    aus_psola: Vec<f32>,
    aus_signalsmith: Vec<f32>,
    aus_wsola: Vec<f32>,
    aus_ki: Vec<f32>,
    aus_ki_neu: Vec<f32>,
    /// Hell/Dunkel: aktueller Wert, Ziel und Filter (None = 0 dB).
    hell: f32,
    hell_ziel: f32,
    hell_eq: Option<Equalizer>,
}

/// Filter für Hell/Dunkel: Höhen ab 2,5 kHz um `db`, Tiefen unter 250 Hz
/// halb so stark in die andere Richtung.
fn hell_eq(db: f32) -> Option<Equalizer> {
    (db != 0.0).then(|| Equalizer::new(&[Band::Hoehen { hz: 2500.0, db }, Band::Tiefen { hz: 250.0, db: -db * 0.5 }]))
}

fn psola_einstellung(e: FigurStimme) -> PsolaEinstellung {
    PsolaEinstellung { halbtoene: e.halbtoene, formant: e.formant }
}

fn signalsmith_einstellung(e: FigurStimme) -> StimmEinstellung {
    StimmEinstellung { halbtoene: e.halbtoene, formant: e.formant, fenster_ms: SIGNALSMITH_FENSTER_MS }
}

fn wsola_einstellung(e: FigurStimme) -> WsolaEinstellung {
    WsolaEinstellung { halbtoene: e.halbtoene, formant: e.formant }
}

fn quelle_fuer(e: Option<FigurStimme>) -> Quelle {
    match e.map(|e| e.verfahren) {
        None => Quelle::Trocken,
        Some(Verfahren::Signalsmith) => Quelle::Signalsmith,
        Some(Verfahren::Psola) => Quelle::Psola,
        Some(Verfahren::Wsola) => Quelle::Wsola,
        Some(Verfahren::Ohne) => Quelle::Ohne,
    }
}

/// Tonhöhe, Formant und Laut-Anteil an eine KI-Stimme geben.
pub fn ki_einstellen(k: &mut KiStimme, e: FigurStimme) {
    if let Some(t) = e.ki {
        k.einstellen(t.halbtoene, t.formant, e.laut_anteil);
    }
}

/// Neue Figur-Stimme. `start`: Figur beim Start (`None` = aus).
pub fn figur(start: Option<FigurStimme>) -> (Figur, FigurSteuerung) {
    let (tx, rx) = rtrb::RingBuffer::new(16);
    let neutral = FigurStimme { halbtoene: 0.0, formant: 0.0, verfahren: Verfahren::Psola, laut_anteil: 1.0, hell: 0.0, ki: None };
    let e = start.unwrap_or(neutral);
    // Eine KI-Figur beim Start klingt erst ohne KI, bis ihr Modell da ist.
    let q = quelle_fuer(start);
    let f = Figur {
        psola: Psola::new(psola_einstellung(e)),
        signalsmith: Stimme::new(signalsmith_einstellung(e)),
        wsola: Wsola::new(wsola_einstellung(e)),
        ki: None,
        ki_neu: None,
        rx,
        wunsch: quelle_fuer(start),
        ki_wunsch: e.ki.is_some() && start.is_some(),
        ki_hoerbar: false,
        quelle: q,
        warm: None,
        letzte: start,
        trocken: Vec::new(),
        aus_psola: Vec::new(),
        aus_signalsmith: Vec::new(),
        aus_wsola: Vec::new(),
        aus_ki: Vec::new(),
        aus_ki_neu: Vec::new(),
        hell: e.hell,
        hell_ziel: e.hell,
        hell_eq: hell_eq(e.hell),
    };
    (f, FigurSteuerung { tx })
}

impl Figur {
    pub fn ist_an(&self) -> bool {
        self.quelle != Quelle::Trocken
    }

    fn quelle(&self, q: Quelle) -> &[f32] {
        match q {
            Quelle::Trocken | Quelle::Ohne => &self.trocken,
            Quelle::Psola => &self.aus_psola,
            Quelle::Signalsmith => &self.aus_signalsmith,
            Quelle::Wsola => &self.aus_wsola,
        }
    }

    /// Hell/Dunkel auf den fertigen Block. Ändert sich der Wert, blendet
    /// der Block vom alten zum neuen Filter über (sonst knackt es).
    fn hell_anwenden(&mut self, block: &mut [f32]) -> Result<(), String> {
        if self.hell_ziel == self.hell {
            if let Some(eq) = self.hell_eq.as_mut() {
                eq.verarbeite(block)?;
            }
            return Ok(());
        }
        let mut alt = block.to_vec();
        if let Some(eq) = self.hell_eq.as_mut() {
            eq.verarbeite(&mut alt)?;
        }
        let mut neu_eq = hell_eq(self.hell_ziel);
        if let Some(eq) = neu_eq.as_mut() {
            eq.verarbeite(block)?;
        }
        let n = block.len() as f32;
        for (i, (b, a)) in block.iter_mut().zip(&alt).enumerate() {
            let w = (i + 1) as f32 / n;
            *b = *a * (1.0 - w) + *b * w;
        }
        self.hell_eq = neu_eq;
        self.hell = self.hell_ziel;
        Ok(())
    }

    /// Verzögerung eines Verfahrens (ohne KI), auch wenn es gerade ruht.
    fn verzoegerung_von(&self, q: Quelle) -> usize {
        match q {
            Quelle::Psola => self.psola.verzoegerung(),
            Quelle::Signalsmith => self.signalsmith.verzoegerung(),
            Quelle::Wsola => self.wsola.verzoegerung(),
            Quelle::Trocken | Quelle::Ohne => 0,
        }
    }

    /// Wie lange ein gewecktes Verfahren nach seiner Verzögerung noch anders
    /// klingt als eingeschwungen (Samples). PSOLA: bis das YIN-Fenster voll ist
    /// (ohne das sprang PSOLA im Test nach dem Wecken). Signalsmith und WSOLA:
    /// ein Block Reserve (WSOLA liegt bis ca. 10 ms über seinem Mittel).
    /// Gemessen: erster gehörter Block 0,93–1,06 vom normalen Pegel.
    fn anlauf(&self, q: Quelle, block: usize) -> usize {
        match q {
            Quelle::Psola => self.psola.anlauf(),
            Quelle::Signalsmith | Quelle::Wsola => block,
            Quelle::Trocken | Quelle::Ohne => 0,
        }
    }

    /// Ruhendes Verfahren wecken: leer starten, ohne alte Samples.
    fn wecken(&mut self, q: Quelle) {
        match q {
            Quelle::Psola => self.psola.zuruecksetzen(),
            Quelle::Signalsmith => self.signalsmith.zuruecksetzen(),
            Quelle::Wsola => self.wsola.zuruecksetzen(),
            Quelle::Trocken | Quelle::Ohne => {}
        }
    }

    /// Läuft gerade eine KI-Stimme (auch warm im Hintergrund)?
    pub fn ki_laeuft(&self) -> bool {
        self.ki.is_some() || self.ki_neu.is_some()
    }
}

impl Effekt for Figur {
    fn verarbeite(&mut self, block: &mut [f32]) -> Result<(), String> {
        while let Ok(b) = self.rx.pop() {
            match b {
                Befehl::Stimme(e) => {
                    if let Some(e) = e {
                        self.psola.einstellen(psola_einstellung(e));
                        self.signalsmith.einstellen(signalsmith_einstellung(e));
                        self.wsola.einstellen(wsola_einstellung(e));
                        if let Some(k) = self.ki.as_mut() {
                            ki_einstellen(k, e);
                        }
                        if let Some((k, _)) = self.ki_neu.as_mut() {
                            ki_einstellen(k, e);
                        }
                    }
                    self.letzte = e;
                    self.wunsch = quelle_fuer(e);
                    self.ki_wunsch = e.is_some_and(|e| e.ki.is_some());
                    self.hell_ziel = e.map_or(0.0, |e| e.hell);
                }
                Befehl::Ki(mut k) => {
                    if let Some(e) = self.letzte {
                        ki_einstellen(&mut k, e);
                    }
                    self.ki_neu = Some((k, 0));
                }
                Befehl::KiPaket(p) => {
                    // Gilt für die Stimme, die gerade läuft oder warm läuft.
                    if let Some((k, _)) = self.ki_neu.as_mut() {
                        k.paket(p);
                    } else if let Some(k) = self.ki.as_mut() {
                        k.paket(p);
                    }
                }
            }
        }
        let n = block.len();

        // 1. KI-Stimme (falls gewünscht und bereit) vor dem Verfahren.
        for v in [&mut self.aus_ki, &mut self.aus_ki_neu] {
            if v.len() != n {
                v.resize(n, 0.0);
            }
            v.copy_from_slice(block);
        }
        if let Some(k) = self.ki.as_mut() {
            k.verarbeite(&mut self.aus_ki)?;
        }
        // Eine neue KI-Stimme läuft warm und ersetzt dann die alte.
        // `alte_ki`: Ausgabe der alten KI in diesem Block (zum Überblenden).
        let mut alte_ki = false;
        if let Some((k, bloecke)) = self.ki_neu.as_mut() {
            k.verarbeite(&mut self.aus_ki_neu)?;
            *bloecke += 1;
            if *bloecke >= k.warm_bloecke() {
                let (k, _) = self.ki_neu.take().expect("da");
                alte_ki = self.ki_hoerbar;
                // Ab jetzt: aus_ki = neue Stimme, aus_ki_neu = alte (falls es eine gab).
                std::mem::swap(&mut self.aus_ki, &mut self.aus_ki_neu);
                self.ki = Some(k);
            }
        }
        let ki_ziel = self.ki_wunsch && self.ki.is_some();
        if ki_ziel != self.ki_hoerbar || alte_ki {
            // Eingang des Verfahrens über einen Block überblenden (gleiche Leistung).
            for i in 0..n {
                let von = if alte_ki { self.aus_ki_neu[i] } else if self.ki_hoerbar { self.aus_ki[i] } else { block[i] };
                let nach = if ki_ziel { self.aus_ki[i] } else { block[i] };
                let w = (i + 1) as f32 / n as f32 * std::f32::consts::FRAC_PI_2;
                block[i] = von * w.cos() + nach * w.sin();
            }
        } else if ki_ziel {
            block.copy_from_slice(&self.aus_ki);
        }
        self.ki_hoerbar = ki_ziel;
        // Keine KI mehr gewünscht: Rechenzeit sparen.
        if !self.ki_wunsch && !self.ki_hoerbar {
            self.ki = None;
            self.ki_neu = None;
        }

        // 2. Verfahren für Tonhöhe und Formant.
        for v in [&mut self.trocken, &mut self.aus_psola, &mut self.aus_signalsmith, &mut self.aus_wsola] {
            if v.len() != n {
                v.resize(n, 0.0);
            }
            v.copy_from_slice(block);
        }
        let mut ziel = self.wunsch;
        // Ein anderes Verfahren wird geweckt und läuft erst warm.
        let rechnet = matches!(ziel, Quelle::Psola | Quelle::Signalsmith | Quelle::Wsola);
        if rechnet && ziel != self.quelle {
            if self.warm.map(|(q, _)| q) != Some(ziel) {
                self.wecken(ziel);
                self.warm = Some((ziel, 0));
            }
        } else {
            self.warm = None;
        }
        // Nur das gehörte und das warm laufende Verfahren rechnen.
        let laeuft = |q: Quelle| q == self.quelle || self.warm.map(|(w, _)| w) == Some(q);
        let (psola, signalsmith, wsola) = (laeuft(Quelle::Psola), laeuft(Quelle::Signalsmith), laeuft(Quelle::Wsola));
        if psola {
            self.psola.verarbeite(&mut self.aus_psola)?;
        }
        if signalsmith {
            self.signalsmith.verarbeite(&mut self.aus_signalsmith)?;
        }
        if wsola {
            self.wsola.verarbeite(&mut self.aus_wsola)?;
        }
        if let Some((q, bloecke)) = self.warm {
            let noetig = (self.verzoegerung_von(q) + self.anlauf(q, n)).div_ceil(n);
            if bloecke + 1 >= noetig {
                self.warm = None;
            } else {
                self.warm = Some((q, bloecke + 1));
                ziel = self.quelle;
            }
        }
        if ziel == self.quelle {
            block.copy_from_slice(self.quelle(ziel));
        } else {
            // Überblenden über einen Block. Die Quellen sind zeitlich versetzt
            // (nicht gleich), darum gleiche Leistung (sin/cos) statt linear.
            let von = self.quelle(self.quelle);
            let nach = self.quelle(ziel);
            for i in 0..n {
                let w = (i + 1) as f32 / n as f32 * std::f32::consts::FRAC_PI_2;
                block[i] = von[i] * w.cos() + nach[i] * w.sin();
            }
        }
        self.quelle = ziel;
        self.hell_anwenden(block)
    }

    fn verzoegerung(&self) -> usize {
        let verfahren = match self.quelle {
            Quelle::Trocken | Quelle::Ohne => 0,
            Quelle::Psola => self.psola.verzoegerung(),
            Quelle::Signalsmith => self.signalsmith.verzoegerung(),
            Quelle::Wsola => self.wsola.verzoegerung(),
        };
        let ki = if self.ki_hoerbar { self.ki.as_ref().map_or(ki::VERZOEGERUNG, |k| k.verzoegerung()) } else { 0 };
        verfahren + ki
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::denoise::BLOCK;

    fn sinus(n: usize, hz: f32) -> Vec<f32> {
        (0..n).map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / 48_000.0).sin()).collect()
    }

    const OPA: FigurStimme = FigurStimme { halbtoene: -4.0, formant: -3.0, verfahren: Verfahren::Psola, laut_anteil: 1.0, hell: 0.0, ki: None };
    const RIESE: FigurStimme = FigurStimme { halbtoene: -12.0, formant: -3.0, verfahren: Verfahren::Signalsmith, laut_anteil: 1.0, hell: 0.0, ki: None };

    #[test]
    fn aus_gibt_eingang_unveraendert_aus() {
        let (mut f, _st) = figur(None);
        let x = sinus(BLOCK * 20, 200.0);
        let mut y = x.clone();
        for b in y.chunks_mut(BLOCK) {
            f.verarbeite(b).unwrap();
        }
        assert_eq!(x, y);
        assert_eq!(f.verzoegerung(), 0);
    }

    #[test]
    fn an_gibt_dasselbe_wie_psola() {
        let (mut f, _st) = figur(Some(OPA));
        let mut p = Psola::new(psola_einstellung(OPA));
        let x = sinus(BLOCK * 20, 200.0);
        let (mut a, mut b) = (x.clone(), x);
        for (ba, bb) in a.chunks_mut(BLOCK).zip(b.chunks_mut(BLOCK)) {
            f.verarbeite(ba).unwrap();
            p.verarbeite(bb).unwrap();
        }
        assert_eq!(a, b);
        assert_eq!(f.verzoegerung(), p.verzoegerung());
    }

    /// Größter Sprung zwischen zwei Samples.
    fn groesster_sprung(y: &[f32]) -> f32 {
        y.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
    }

    /// Hell/Dunkel wirkt bei jedem Verfahren (auch Leicht und Ohne): Es
    /// läuft nach der Stimme auf dem fertigen Block.
    #[test]
    fn hell_wirkt_bei_jedem_verfahren() {
        for verfahren in [Verfahren::Psola, Verfahren::Signalsmith, Verfahren::Wsola, Verfahren::Ohne] {
            let pegel = |hell: f32| {
                let e = FigurStimme { halbtoene: 0.0, formant: 0.0, verfahren, laut_anteil: 1.0, hell, ki: None };
                let (mut f, _st) = figur(Some(e));
                let mut y = sinus(BLOCK * 40, 3000.0);
                for b in y.chunks_mut(BLOCK) {
                    f.verarbeite(b).unwrap();
                }
                (y[BLOCK * 20..].iter().map(|v| v * v).sum::<f32>() / (BLOCK * 20) as f32).sqrt()
            };
            let (dunkel, normal, hell) = (pegel(-6.0), pegel(0.0), pegel(6.0));
            assert!(hell > normal * 1.5 && dunkel < normal * 0.7, "{verfahren:?}: dunkel {dunkel}, normal {normal}, hell {hell}");
        }
    }

    /// „Ohne“: keine Verzögerung, die Stimme bleibt, wie sie ist (nur Hell/Dunkel).
    #[test]
    fn ohne_hat_keine_verzoegerung() {
        let e = FigurStimme { halbtoene: 5.0, formant: 3.0, verfahren: Verfahren::Ohne, laut_anteil: 1.0, hell: 0.0, ki: None };
        let (mut f, mut st) = figur(Some(OPA));
        let x = sinus(BLOCK * 30, 220.0);
        let mut y = x.clone();
        for (i, b) in y.chunks_mut(BLOCK).enumerate() {
            if i == 10 {
                assert!(st.setzen(Some(e)));
            }
            f.verarbeite(b).unwrap();
        }
        assert_eq!(f.verzoegerung(), 0);
        assert!(f.ist_an());
        // Nach dem Überblenden (ein Block): genau der Eingang.
        assert_eq!(&y[BLOCK * 12..], &x[BLOCK * 12..]);
    }

    #[test]
    fn umschalten_knackt_nicht() {
        let (mut f, mut st) = figur(None);
        let x = sinus(BLOCK * 60, 200.0);
        let mut y = x.clone();
        for (i, b) in y.chunks_mut(BLOCK).enumerate() {
            match i {
                20 => assert!(st.setzen(Some(OPA))),
                40 => assert!(st.setzen(None)),
                _ => {}
            }
            f.verarbeite(b).unwrap();
        }
        // Normaler Sprung beim Sinus: 0,5 · 2π · 200 / 48000 ≈ 0,013. PSOLA selbst
        // springt beim reinen Sinus etwas mehr (eigen). Harter Wechsel wäre bis 1,0.
        let mut p = Psola::new(psola_einstellung(OPA));
        let mut allein = sinus(BLOCK * 60, 200.0);
        for b in allein.chunks_mut(BLOCK) {
            p.verarbeite(b).unwrap();
        }
        let eigen = groesster_sprung(&allein[BLOCK * 10..]).max(0.013);
        let sprung = groesster_sprung(&y[BLOCK * 10..]);
        println!("Sprung {sprung:.3}, PSOLA allein {eigen:.3}");
        assert!(sprung < eigen * 1.3, "Sprung {sprung}, PSOLA allein {eigen}");
        // Nach dem Ausschalten wieder genau trocken.
        assert_eq!(&y[BLOCK * 41..], &x[BLOCK * 41..]);
    }

    #[test]
    fn verzoegerung_folgt_dem_schalter() {
        let (mut f, mut st) = figur(None);
        let mut b = vec![0.0; BLOCK];
        st.setzen(Some(OPA));
        // PSOLA läuft erst warm (Verzögerung + YIN-Fenster: 6 Blöcke), dann ist die Figur an.
        for _ in 0..6 {
            assert!(!f.ist_an());
            f.verarbeite(&mut b).unwrap();
        }
        assert!(f.ist_an());
        assert!(f.verzoegerung() > 1000, "{}", f.verzoegerung());
        st.setzen(None);
        f.verarbeite(&mut b).unwrap();
        assert_eq!(f.verzoegerung(), 0);
    }

    #[test]
    fn hell_aendern_knackt_nicht() {
        let (mut f, mut st) = figur(None);
        let x = sinus(BLOCK * 60, 3000.0);
        let mut y = x.clone();
        let hell = |db: f32| Some(FigurStimme { halbtoene: 0.0, formant: 0.0, verfahren: Verfahren::Psola, laut_anteil: 1.0, hell: db, ki: None });
        for (i, b) in y.chunks_mut(BLOCK).enumerate() {
            match i {
                10 => assert!(st.setzen(hell(0.0))),
                20 => assert!(st.setzen(hell(6.0))),
                40 => assert!(st.setzen(hell(-6.0))),
                _ => {}
            }
            f.verarbeite(b).unwrap();
        }
        let pegel = |a: usize, b: usize| (y[a * BLOCK..b * BLOCK].iter().map(|v| v * v).sum::<f32>() / ((b - a) * BLOCK) as f32).sqrt();
        // +6 dB Höhen: bei 3 kHz deutlich lauter, −6 dB leiser.
        assert!(pegel(30, 39) > pegel(12, 19) * 1.5, "hell {} gegen {}", pegel(30, 39), pegel(12, 19));
        assert!(pegel(50, 59) < pegel(12, 19) * 0.7, "dunkel {} gegen {}", pegel(50, 59), pegel(12, 19));
        // Kein harter Sprung an den Wechseln (Sinus 0,5 bei 3 kHz: normal bis ca. 0,2).
        for k in [20, 40] {
            let sprung = groesster_sprung(&y[BLOCK * (k - 1)..BLOCK * (k + 2)]);
            let normal = groesster_sprung(&y[BLOCK * (k + 5)..BLOCK * (k + 8)]).max(groesster_sprung(&y[BLOCK * (k - 8)..BLOCK * (k - 5)]));
            assert!(sprung < normal * 1.3, "Wechsel bei Block {k}: Sprung {sprung}, normal {normal}");
        }
    }

    #[test]
    fn signalsmith_gibt_dasselbe_wie_stimme() {
        let (mut f, _st) = figur(Some(RIESE));
        let mut p = Stimme::new(signalsmith_einstellung(RIESE));
        let x = sinus(BLOCK * 20, 200.0);
        let (mut a, mut b) = (x.clone(), x);
        for (ba, bb) in a.chunks_mut(BLOCK).zip(b.chunks_mut(BLOCK)) {
            f.verarbeite(ba).unwrap();
            p.verarbeite(bb).unwrap();
        }
        assert_eq!(a, b);
        assert_eq!(f.verzoegerung(), p.verzoegerung());
    }

    #[test]
    fn wechsel_zwischen_verfahren_knackt_nicht() {
        let (mut f, mut st) = figur(None);
        let x = sinus(BLOCK * 100, 200.0);
        let mut y = x.clone();
        for (i, b) in y.chunks_mut(BLOCK).enumerate() {
            match i {
                20 => assert!(st.setzen(Some(OPA))),
                40 => assert!(st.setzen(Some(RIESE))),
                60 => assert!(st.setzen(Some(OPA))),
                80 => assert!(st.setzen(None)),
                _ => {}
            }
            f.verarbeite(b).unwrap();
        }
        // Signalsmith ist schon eingeschwungen: keine Lücke nach dem Wechsel.
        let leiseste = y[BLOCK * 41..BLOCK * 60].chunks(BLOCK).map(|b| b.iter().fold(0.0f32, |m, v| m.max(v.abs()))).fold(1.0f32, f32::min);
        assert!(leiseste > 0.1, "Lücke nach dem Wechsel zu Signalsmith: {leiseste}");
        // Kein harter Sprung an den Wechseln (Sinus 0,5: normal bis ca. 0,013 je Sample).
        for k in [20, 40, 60, 80] {
            let sprung = groesster_sprung(&y[BLOCK * (k - 1)..BLOCK * (k + 2)]);
            assert!(sprung < 0.15, "Wechsel bei Block {k}: Sprung {sprung}");
        }
        assert_eq!(&y[BLOCK * 81..], &x[BLOCK * 81..]);
    }

    const HELIUM: FigurStimme = FigurStimme { halbtoene: 12.0, formant: 0.0, verfahren: Verfahren::Wsola, laut_anteil: 1.0, hell: 0.0, ki: None };

    #[test]
    fn wsola_gibt_dasselbe_wie_wsola() {
        let (mut f, _st) = figur(Some(HELIUM));
        let mut p = Wsola::new(wsola_einstellung(HELIUM));
        let x = sinus(BLOCK * 20, 200.0);
        let (mut a, mut b) = (x.clone(), x);
        for (ba, bb) in a.chunks_mut(BLOCK).zip(b.chunks_mut(BLOCK)) {
            f.verarbeite(ba).unwrap();
            p.verarbeite(bb).unwrap();
        }
        assert_eq!(a, b);
        assert_eq!(f.verzoegerung(), p.verzoegerung());
    }

    #[test]
    fn wechsel_zu_wsola_knackt_nicht() {
        let (mut f, mut st) = figur(None);
        let x = sinus(BLOCK * 100, 200.0);
        let mut y = x.clone();
        for (i, b) in y.chunks_mut(BLOCK).enumerate() {
            match i {
                20 => assert!(st.setzen(Some(HELIUM))),
                40 => assert!(st.setzen(Some(OPA))),
                60 => assert!(st.setzen(Some(HELIUM))),
                80 => assert!(st.setzen(None)),
                _ => {}
            }
            f.verarbeite(b).unwrap();
        }
        // WSOLA ist schon eingeschwungen: keine Lücke nach dem Wechsel.
        let leiseste = y[BLOCK * 21..BLOCK * 40].chunks(BLOCK).map(|b| b.iter().fold(0.0f32, |m, v| m.max(v.abs()))).fold(1.0f32, f32::min);
        assert!(leiseste > 0.1, "Lücke nach dem Wechsel zu WSOLA: {leiseste}");
        // Normaler Sprung bei 400 Hz (Sinus 0,5): ca. 0,026 je Sample.
        for k in [20, 40, 60, 80] {
            let sprung = groesster_sprung(&y[BLOCK * (k - 1)..BLOCK * (k + 2)]);
            assert!(sprung < 0.15, "Wechsel bei Block {k}: Sprung {sprung}");
        }
        assert_eq!(&y[BLOCK * 81..], &x[BLOCK * 81..]);
    }

    /// Stimmähnlicher Ton (Obertöne 1/k).
    fn klang(n: usize, f0: f32) -> Vec<f32> {
        (0..n).map(|i| (1..20).map(|k| (std::f32::consts::TAU * f0 * k as f32 * i as f32 / 48_000.0).sin() / k as f32).sum::<f32>() * 0.2).collect()
    }

    #[test]
    fn extreme_tonhoehen_bleiben_endlich() {
        for verfahren in [Verfahren::Psola, Verfahren::Signalsmith, Verfahren::Wsola] {
            for halbtoene in [-15.0, 30.0] {
                let e = FigurStimme { halbtoene, formant: 0.0, verfahren, laut_anteil: 1.0, hell: 0.0, ki: None };
                let (mut f, _st) = figur(Some(e));
                let mut y = klang(BLOCK * 100, 150.0);
                for b in y.chunks_mut(BLOCK) {
                    f.verarbeite(b).unwrap();
                }
                let spitze = y.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                let pegel = (y[BLOCK * 50..].iter().map(|v| v * v).sum::<f32>() / (BLOCK * 50) as f32).sqrt();
                assert!(y.iter().all(|v| v.is_finite()), "{verfahren:?} {halbtoene}: nicht endlich");
                // Eingang: Spitze ca. 0,33, Pegel ca. 0,18.
                assert!(spitze < 2.0 && pegel > 0.02, "{verfahren:?} {halbtoene}: Spitze {spitze}, Pegel {pegel}");
            }
        }
    }

    /// Wecken aus der Ruhe: Wie lange läuft das Verfahren warm, ist der erste
    /// gehörte Block schon voll da, knackt es, gibt es eine Lücke?
    #[test]
    fn geweckt_ist_gleich_voll_da() {
        let figuren = [
            ("PSOLA", OPA),
            ("Signalsmith", RIESE),
            ("WSOLA +12", HELIUM),
            ("WSOLA -15", FigurStimme { halbtoene: -15.0, ..HELIUM }),
            ("WSOLA +30", FigurStimme { halbtoene: 30.0, ..HELIUM }),
        ];
        // Erst trocken, dann jede Figur, dazwischen „aus“ (alle ruhen).
        for von in [None, Some(HELIUM), Some(OPA)] {
            for (name, e) in figuren {
                if von == Some(e) {
                    continue;
                }
                let (mut f, mut st) = figur(von);
                let x = sinus(BLOCK * 60, 200.0);
                let mut y = x.clone();
                let mut an = None;
                for (i, b) in y.chunks_mut(BLOCK).enumerate() {
                    if i == 20 {
                        assert!(st.setzen(Some(e)));
                    }
                    f.verarbeite(b).unwrap();
                    if i >= 20 && an.is_none() && f.quelle == quelle_fuer(Some(e)) {
                        an = Some(i);
                    }
                }
                let an = an.expect("schaltet nie um");
                let warm_ms = (an - 20 + 1) * 10;
                let pegel = |k: usize| (y[k * BLOCK..(k + 1) * BLOCK].iter().map(|v| v * v).sum::<f32>() / BLOCK as f32).sqrt();
                let ruhig = (an + 10..an + 20).map(pegel).sum::<f32>() / 10.0;
                assert!(an + 20 <= 60, "{name}: zu spät umgeschaltet");
                let erster = pegel(an + 1) / ruhig;
                // Sprung am Wechsel gegen den normalen Sprung von alter und neuer Stimme.
                let sprung = groesster_sprung(&y[BLOCK * (an - 1)..BLOCK * (an + 2)]);
                let normal = groesster_sprung(&y[BLOCK * (an + 3)..BLOCK * 60]).max(groesster_sprung(&y[BLOCK * 10..BLOCK * (an - 1)]));
                let leiseste = (20..an + 5).map(|k| y[k * BLOCK..(k + 1) * BLOCK].iter().fold(0.0f32, |m, v| m.max(v.abs()))).fold(1.0f32, f32::min);
                println!("{:?} → {name}: warm {warm_ms} ms, erster Block {erster:.2} vom Pegel, Sprung {sprung:.3} (normal {normal:.3}), leisester Block {leiseste:.2}", von.map(|v| v.verfahren));
                assert!(warm_ms <= 60, "{name}: {warm_ms} ms warm");
                assert!(erster > 0.8 && erster < 1.25, "{name}: erster Block {erster} vom Pegel");
                assert!(sprung < normal * 1.5 + 0.02, "{name}: Sprung {sprung}, normal {normal}");
                assert!(leiseste > 0.2, "{name}: Lücke, leisester Block {leiseste}");
            }
        }
    }

    /// Rechenzeit je Block mit aktiver Figur (nur das gehörte Verfahren rechnet):
    /// cargo test --lib figur_rechenzeit -- --ignored --nocapture
    #[test]
    #[ignore]
    fn figur_rechenzeit() {
        let x = klang(BLOCK * 1000, 150.0);
        for (name, e) in [("aus", None), ("PSOLA", Some(OPA)), ("Signalsmith", Some(RIESE)), ("WSOLA +12", Some(HELIUM)), ("WSOLA +30", Some(FigurStimme { halbtoene: 30.0, ..HELIUM }))] {
            let (mut f, _st) = figur(e);
            let mut y = x.clone();
            let t = std::time::Instant::now();
            for b in y.chunks_mut(BLOCK) {
                f.verarbeite(b).unwrap();
            }
            println!("{name:12}: {:.4} ms je Block", t.elapsed().as_secs_f64() * 1000.0 / 1000.0);
        }
    }

    #[test]
    fn tonhoehe_wird_begrenzt() {
        let def = |halbtoene: f32, verfahren: Verfahren| FigurDef { name: "x".into(), halbtoene, formant: 0.0, bild: None, bild_ausschnitt: None, umgebung: None, umgebung_staerke: None, verfahren, ki: None, ki_an: false, hell: 0.0 };
        assert_eq!(def(99.0, Verfahren::Psola).einstellung().halbtoene, 30.0);
        assert_eq!(def(-99.0, Verfahren::Wsola).einstellung().halbtoene, -15.0);
        assert_eq!(def(f32::NAN, Verfahren::Signalsmith).einstellung().halbtoene, 0.0);
        assert_eq!(def(29.95, Verfahren::Wsola).einstellung().halbtoene, 29.95);
    }

    /// KI vor dem Verfahren (Dan, 21.10.2026). Mit „Ohne“ bekommt die KI
    /// Tonhöhe und Formant (Feinabstimmung, eigene Grenzen im Motor), sonst
    /// das Verfahren, und die KI bleibt bei ihrer Ziel-Tonhöhe.
    #[test]
    fn ki_vor_dem_verfahren() {
        let wahl = KiWahl { modell: "thorsten".into(), stimmen: vec![], laut_anteil: 0.25, tonhoehe_auto: true };
        let def = |verfahren: Verfahren, ki_an: bool| FigurDef {
            name: "x".into(),
            halbtoene: 5.0,
            formant: 2.0,
            bild: None,
            bild_ausschnitt: None,
            umgebung: None,
            umgebung_staerke: None,
            verfahren,
            ki: Some(wahl.clone()),
            ki_an,
            hell: 0.0,
        };
        let e = def(Verfahren::Ohne, true).einstellung();
        assert_eq!(e.ki, Some(KiTon { halbtoene: 5.0, formant: 2.0 }));
        assert_eq!(e.laut_anteil, 0.25);
        let e = def(Verfahren::Psola, true).einstellung();
        assert_eq!(e.ki, Some(KiTon { halbtoene: 0.0, formant: 0.0 }));
        assert_eq!((e.halbtoene, e.formant), (5.0, 2.0));
        assert_eq!(def(Verfahren::Psola, false).einstellung().ki, None);
    }

    #[test]
    fn verfahren_als_text() {
        assert_eq!(serde_json::to_string(&Verfahren::Wsola).unwrap(), "\"wsola\"");
        assert_eq!(serde_json::from_str::<Verfahren>("\"wsola\"").unwrap(), Verfahren::Wsola);
    }
}
