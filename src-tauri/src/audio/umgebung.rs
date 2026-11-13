//! Stimm-Umgebungen: setzt die Stimme in einen Raum oder ein Gerät
//! (Halle, Höhle, Funkgerät, …). Läuft nach der Figur, nur im Ausgang „Spiel“.
//!
//! Alles selbst gebaut, ohne KI: Hall (8 Leitungen mit Rückkopplung),
//! Filter, Echo, Verzerrung, Schwebung (Chorus/Flanger), Ringmodulator.
//! Keine zusätzliche Verzögerung: Die Stimme selbst kommt sofort, nur der
//! Nachklang kommt später.
//!
//! Umschalten: Es gibt drei Plätze. Der neue Platz bekommt die Stimme
//! (Einblenden 20 ms). Der alte bekommt keine Stimme mehr, klingt aber aus.
//! So knackt nichts, und ein Hall bricht beim Ausschalten nicht ab. Erst wer
//! viermal in wenigen Sekunden umschaltet, schneidet einen Nachhall ab.

use std::f32::consts::TAU;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::eq::{Band, Biquad};
use super::pipe::INTERN_RATE;
use super::psola::{Psola, PsolaEinstellung};
use super::verarbeitung::Effekt;

const FS: f32 = INTERN_RATE as f32;
/// Ein- und Ausblenden beim Umschalten (Sekunden).
const BLENDE_S: f32 = 0.02;
/// Unter diesem Pegel gilt ein ausklingender Platz als still.
const STILL: f32 = 1e-5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Umgebung {
    /// Großes Gebäude (Halle, Kirche).
    Halle,
    /// Tropfsteinhöhle.
    Hoehle,
    /// Funkgerät / Radio.
    Funk,
    /// Telepathie / Gedanken.
    Telepathie,
    Roboter,
    /// Maskiert (Stimme hinter einer Maske).
    Maske,
    /// Raumschiff: Stimme im Raumhelm, über Bordfunk, leiser Ton am Ende (wie
    /// NASA-Funk). Heißt intern noch „Raumhelm“ (gespeicherte Einstellungen).
    Raumhelm,
    /// Weltraum: weite, dunkle Leere mit schimmerndem Hall.
    Weltall,
    /// Megafon / Lautsprecher-Tüte.
    Megafon,
}

impl Umgebung {
    pub const ALLE: [Umgebung; 9] = [
        Umgebung::Halle,
        Umgebung::Hoehle,
        Umgebung::Funk,
        Umgebung::Telepathie,
        Umgebung::Roboter,
        Umgebung::Maske,
        Umgebung::Raumhelm,
        Umgebung::Weltall,
        Umgebung::Megafon,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Umgebung::Halle => "Großes Gebäude",
            Umgebung::Hoehle => "Tropfsteinhöhle",
            Umgebung::Funk => "Funkgerät",
            Umgebung::Telepathie => "Telepathie",
            Umgebung::Roboter => "Roboter",
            Umgebung::Maske => "Maskiert",
            Umgebung::Raumhelm => "Raumschiff",
            Umgebung::Weltall => "Weltraum",
            Umgebung::Megafon => "Megafon",
        }
    }

    /// Stärke ab Werk (0 = trocken, 1 = voll).
    pub fn staerke_standard(self) -> f32 {
        match self {
            Umgebung::Halle | Umgebung::Hoehle | Umgebung::Weltall => 0.7,
            _ => 1.0,
        }
    }
}

impl fmt::Display for Umgebung {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Was der Effekt tun soll: Umgebung und Stärke (0…1), oder `None` = aus.
pub type UmgebungWahl = Option<(Umgebung, f32)>;

// ---------------------------------------------------------------- Bausteine

/// Verzögerungs-Leitung (Ringpuffer, Länge eine Zweierpotenz).
struct Linie {
    buf: Vec<f32>,
    maske: usize,
    pos: usize,
}

impl Linie {
    fn new(max: usize) -> Self {
        let n = (max + 2).next_power_of_two();
        Self { buf: vec![0.0; n], maske: n - 1, pos: 0 }
    }
    fn leeren(&mut self) {
        self.buf.fill(0.0);
    }
    #[inline]
    fn schreiben(&mut self, x: f32) {
        self.pos = (self.pos + 1) & self.maske;
        self.buf[self.pos] = x;
    }
    /// Wert von vor `d` Samples (`d` ≥ 1; 1 = zuletzt geschrieben).
    #[inline]
    fn lesen(&self, d: usize) -> f32 {
        self.buf[self.pos.wrapping_sub(d - 1) & self.maske]
    }
    /// Wie [`lesen`](Self::lesen), mit Bruchteil (linear).
    #[inline]
    fn lesen_fein(&self, d: f32) -> f32 {
        let d = d.max(1.0);
        let i = d as usize;
        let t = d - i as f32;
        let a = self.lesen(i);
        a + (self.lesen(i + 1) - a) * t
    }
}

fn samples(ms: f32) -> usize {
    ((ms * 0.001 * FS) as usize).max(1)
}

/// Allpass (Schroeder): verwischt, ohne den Klang zu färben.
struct Allpass {
    l: Linie,
    d: usize,
    g: f32,
}

impl Allpass {
    fn new(ms: f32, g: f32) -> Self {
        let d = samples(ms);
        Self { l: Linie::new(d), d, g }
    }
    #[inline]
    fn schritt(&mut self, x: f32) -> f32 {
        let v = self.l.lesen(self.d);
        let w = x + self.g * v;
        self.l.schreiben(w);
        v - self.g * w
    }
}

/// Länge der 8 Hall-Leitungen bei Größe 1 (ms). Teilerfremd, damit sich
/// keine Töne bilden.
const HALL_MS: [f32; 8] = [29.7, 37.1, 41.1, 43.7, 53.3, 59.9, 67.1, 73.7];
const HALL_GROESSE_MAX: f32 = 2.5;
const HALL_VOR_MS_MAX: f32 = 80.0;
/// Vorzeichen, mit denen die Stimme in die Leitungen geht.
const HALL_EIN: [f32; 8] = [1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, -1.0];

/// Hall: 8 Leitungen, die sich über eine Householder-Matrix gegenseitig
/// speisen (Feedback Delay Network). Jede Leitung dämpft die Höhen.
struct Hall {
    vor: Linie,
    vor_d: usize,
    diffus: [Allpass; 4],
    linien: Vec<Linie>,
    laengen: [usize; 8],
    rueck: [f32; 8],
    tp: [f32; 8],
    tp_k: f32,
}

impl Hall {
    fn new() -> Self {
        let max = HALL_MS.iter().map(|&ms| samples(ms * HALL_GROESSE_MAX)).max().unwrap_or(1);
        Self {
            vor: Linie::new(samples(HALL_VOR_MS_MAX)),
            vor_d: 1,
            diffus: [Allpass::new(4.77, 0.6), Allpass::new(3.59, 0.6), Allpass::new(12.73, 0.55), Allpass::new(9.31, 0.55)],
            linien: (0..8).map(|_| Linie::new(max)).collect(),
            laengen: [1; 8],
            rueck: [0.0; 8],
            tp: [0.0; 8],
            tp_k: 0.0,
        }
    }

    /// `groesse`: Raumgröße (1 = mittel). `nachhall_s`: Zeit bis −60 dB.
    /// `hoehen_hz`: Höhen über dieser Frequenz klingen schneller ab.
    fn einstellen(&mut self, groesse: f32, nachhall_s: f32, hoehen_hz: f32, vor_ms: f32) {
        let groesse = groesse.clamp(0.1, HALL_GROESSE_MAX);
        for i in 0..8 {
            self.laengen[i] = samples(HALL_MS[i] * groesse);
            self.rueck[i] = 10f32.powf(-3.0 * self.laengen[i] as f32 / (nachhall_s.max(0.05) * FS));
        }
        self.vor_d = samples(vor_ms.clamp(0.0, HALL_VOR_MS_MAX));
        self.tp_k = (-TAU * hoehen_hz / FS).exp();
    }

    fn leeren(&mut self) {
        self.vor.leeren();
        for a in &mut self.diffus {
            a.l.leeren();
        }
        for l in &mut self.linien {
            l.leeren();
        }
        self.tp = [0.0; 8];
    }

    /// Gibt nur den Hall zurück (ohne die trockene Stimme).
    #[inline]
    fn schritt(&mut self, x: f32) -> f32 {
        let v = self.vor.lesen(self.vor_d);
        self.vor.schreiben(x);
        let mut d = v;
        for a in &mut self.diffus {
            d = a.schritt(d);
        }
        let mut o = [0.0f32; 8];
        let mut summe = 0.0;
        for i in 0..8 {
            let r = self.linien[i].lesen(self.laengen[i]);
            self.tp[i] = (1.0 - self.tp_k) * r + self.tp_k * self.tp[i];
            o[i] = self.tp[i] * self.rueck[i];
            summe += o[i];
        }
        let h = summe * (2.0 / 8.0);
        for i in 0..8 {
            self.linien[i].schreiben(o[i] - h + d * HALL_EIN[i]);
        }
        (o[0] - o[1] + o[2] - o[3] + o[4] - o[5] + o[6] - o[7]) * 0.35
    }
}

/// Echo mit Rückkopplung, jedes Echo dumpfer als das davor.
struct Echo {
    l: Linie,
    d: usize,
    rueck: f32,
    tp: f32,
    tp_k: f32,
}

impl Echo {
    fn new(max_ms: f32) -> Self {
        Self { l: Linie::new(samples(max_ms)), d: 1, rueck: 0.0, tp: 0.0, tp_k: 0.0 }
    }
    fn einstellen(&mut self, ms: f32, rueck: f32, hoehen_hz: f32) {
        self.d = samples(ms).min(self.l.maske - 1);
        self.rueck = rueck;
        self.tp_k = (-TAU * hoehen_hz / FS).exp();
    }
    /// Gibt nur die Echos zurück.
    #[inline]
    fn schritt(&mut self, x: f32) -> f32 {
        let v = self.l.lesen(self.d);
        self.tp = (1.0 - self.tp_k) * v + self.tp_k * self.tp;
        self.l.schreiben(x + self.tp * self.rueck);
        v
    }
}

/// Schwebung: Verzögerung, die langsam schwankt. Lang = Chorus, kurz mit
/// Rückkopplung = Flanger.
struct Schwebung {
    l: Linie,
    basis: f32,
    tiefe: f32,
    phase: f32,
    schritt: f32,
    rueck: f32,
}

impl Schwebung {
    fn new(max_ms: f32) -> Self {
        Self { l: Linie::new(samples(max_ms)), basis: 1.0, tiefe: 0.0, phase: 0.0, schritt: 0.0, rueck: 0.0 }
    }
    fn einstellen(&mut self, basis_ms: f32, tiefe_ms: f32, hz: f32, rueck: f32) {
        self.basis = basis_ms * 0.001 * FS;
        self.tiefe = tiefe_ms * 0.001 * FS;
        self.schritt = hz / FS;
        self.rueck = rueck;
    }
    /// Gibt nur die verschobene Stimme zurück.
    #[inline]
    fn schritt(&mut self, x: f32) -> f32 {
        let d = self.basis + self.tiefe * (0.5 + 0.5 * (TAU * self.phase).sin());
        self.phase += self.schritt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let v = self.l.lesen_fein(d);
        self.l.schreiben(x + v * self.rueck);
        v
    }
}

/// Kammfilter mit Rückkopplung: metallischer oder hohler Klang.
struct Kamm {
    l: Linie,
    d: usize,
    rueck: f32,
}

impl Kamm {
    fn new(max_ms: f32) -> Self {
        Self { l: Linie::new(samples(max_ms)), d: 1, rueck: 0.0 }
    }
    fn einstellen(&mut self, ms: f32, rueck: f32) {
        self.d = samples(ms).min(self.l.maske - 1);
        self.rueck = rueck;
    }
    #[inline]
    fn schritt(&mut self, x: f32) -> f32 {
        let y = x + self.rueck * self.l.lesen(self.d);
        self.l.schreiben(y);
        y
    }
}

/// Weißes Rauschen (Xorshift), −1…1.
struct Rauschen(u32);

impl Rauschen {
    #[inline]
    fn schritt(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Schwingung 0…1 (Sägezahn) für Ringmodulator, Tremolo usw.
#[derive(Default)]
struct Phase(f32);

impl Phase {
    #[inline]
    fn sinus(&mut self, hz: f32) -> f32 {
        let s = (TAU * self.0).sin();
        self.0 += hz / FS;
        if self.0 >= 1.0 {
            self.0 -= 1.0;
        }
        s
    }
}

// ---------------------------------------------------------------- Funkgerät

/// Über diesem Pegel „sendet“ das Funkgerät (ca. −44 dBFS).
const FUNK_SCHWELLE: f32 = 0.006;
/// So lange nach dem letzten Laut sendet es noch (Sekunden).
const FUNK_HALTEN_S: f32 = 0.2;
const FUNK_KLICK_S: f32 = 0.006;
const FUNK_KSCHH_S: f32 = 0.16;
/// Raumhelm: Ton am Ende des Sendens, wie der Aus-Ton („Quindar“) des
/// NASA-Funks (2475 Hz). Der echte war 250 ms lang; das nervte beim Zuhören
/// (Dan), darum kürzer und leise. Ein An-Ton fehlt mit Absicht: Er käme hier
/// erst mit dem ersten Laut und läge über dem ersten Wort.
const HELM_TON_S: f32 = 0.15;
const HELM_AUS_HZ: f32 = 2475.0;
/// Pegel des Tons vor dem Funk-Band (danach ca. −40 dBFS, gut 12 dB unter
/// normaler Stimme).
const HELM_TON_PEGEL: f32 = 0.01;
/// Ein- und Ausblenden des Tons (Samples, 5 ms), sonst knackt er.
const HELM_TON_BLENDE: f32 = 240.0;
/// Funk öffnet: kurzes, leises Rauschen statt Ton (Sekunden).
const HELM_OEFFNEN_S: f32 = 0.025;

// ---------------------------------------------------------------- Platz

/// Ein Platz: eine Umgebung mit allen Bausteinen (Speicher einmal angelegt).
struct Platz {
    art: Option<Umgebung>,
    staerke: f32,
    staerke_ziel: f32,
    /// Wie viel Stimme hineingeht (Ein- und Ausblenden).
    pegel: f32,
    pegel_ziel: f32,
    /// Blöcke in Folge, in denen der ausklingende Platz still war.
    still: u32,
    hall: Hall,
    echo: Echo,
    schwebung: Schwebung,
    kamm: Kamm,
    filter: [Biquad; 5],
    rauschen: Rauschen,
    ring: Phase,
    huelle: f32,
    psola: Option<Psola>,
    ein: Vec<f32>,
    doppel: Vec<f32>,
    // Funkgerät
    sendet: bool,
    halten: usize,
    klick: usize,
    kschh: usize,
}

impl Platz {
    fn new() -> Self {
        Self {
            art: None,
            staerke: 1.0,
            staerke_ziel: 1.0,
            pegel: 0.0,
            pegel_ziel: 0.0,
            still: u32::MAX,
            hall: Hall::new(),
            echo: Echo::new(600.0),
            schwebung: Schwebung::new(40.0),
            kamm: Kamm::new(20.0),
            filter: [Biquad::new(Band::Tiefpass { hz: 20_000.0 }); 5],
            rauschen: Rauschen(0x9e37_79b9),
            ring: Phase::default(),
            huelle: 0.0,
            psola: None,
            ein: Vec::new(),
            doppel: Vec::new(),
            sendet: false,
            halten: 0,
            klick: 0,
            kschh: 0,
        }
    }

    /// Läuft der Platz noch (Stimme drin oder Nachklang)?
    fn laeuft(&self) -> bool {
        self.pegel > 0.0 || self.pegel_ziel > 0.0 || self.still < 3
    }

    /// Setzt eine neue Umgebung. Leert alle Speicher (der Platz war still
    /// oder wird gerade überschrieben).
    fn einstellen(&mut self, wahl: UmgebungWahl) {
        self.art = wahl.map(|(a, _)| a);
        let s = wahl.map_or(1.0, |(_, s)| s.clamp(0.0, 1.0));
        self.staerke = s;
        self.staerke_ziel = s;
        self.still = 0;
        self.hall.leeren();
        self.echo.l.leeren();
        self.schwebung.l.leeren();
        self.kamm.l.leeren();
        self.huelle = 0.0;
        self.sendet = false;
        self.halten = 0;
        self.klick = 0;
        self.kschh = 0;
        self.psola = None;
        let aus = Biquad::new(Band::Tiefpass { hz: 20_000.0 });
        let f = |b: Band| Biquad::new(b);
        match self.art {
            None => {}
            Some(Umgebung::Halle) => {
                self.hall.einstellen(1.6, 2.4, 6000.0, 22.0);
            }
            Some(Umgebung::Hoehle) => {
                self.hall.einstellen(2.3, 4.5, 3500.0, 45.0);
                self.echo.einstellen(380.0, 0.38, 2500.0);
                self.filter = [f(Band::Tiefpass { hz: 4500.0 }), aus, aus, aus, aus];
            }
            Some(Umgebung::Funk) => {
                self.filter = [
                    f(Band::Hochpass { hz: 450.0 }),
                    f(Band::Hochpass { hz: 450.0 }),
                    f(Band::Tiefpass { hz: 2800.0 }),
                    f(Band::Tiefpass { hz: 2800.0 }),
                    f(Band::Tiefpass { hz: 3000.0 }),
                ];
            }
            Some(Umgebung::Telepathie) => {
                self.filter = [f(Band::Hochpass { hz: 280.0 }), f(Band::Hochpass { hz: 120.0 }), aus, aus, aus];
                self.schwebung.einstellen(12.0, 5.0, 0.7, 0.0);
                self.hall.einstellen(0.9, 1.8, 5000.0, 8.0);
                self.psola = Some(Psola::new(PsolaEinstellung { halbtoene: -12.0, formant: -4.0 }));
            }
            Some(Umgebung::Roboter) => {
                self.filter = [f(Band::Hochpass { hz: 150.0 }), aus, aus, aus, aus];
                self.kamm.einstellen(5.3, 0.55);
            }
            Some(Umgebung::Maske) => {
                // Stoff vor dem Mund: Höhen steil weg (24 dB/Oktave ab 1,8 kHz),
                // tiefe Mitten etwas lauter, ein ganz kurzer Hohlraum. Kein Hall.
                self.filter = [
                    f(Band::Tiefpass { hz: 1800.0 }),
                    f(Band::Tiefpass { hz: 1800.0 }),
                    f(Band::Glocke { hz: 450.0, db: 4.0, q: 0.9 }),
                    f(Band::Glocke { hz: 2800.0, db: -8.0, q: 0.7 }),
                    aus,
                ];
                self.kamm.einstellen(0.6, 0.25);
            }
            Some(Umgebung::Raumhelm) => {
                // Glaskuppel vor dem Gesicht: ganz kurzer, dichter Nachklang.
                // Danach Funk, etwas breiter als das Funkgerät.
                self.kamm.einstellen(1.1, 0.35);
                self.hall.einstellen(0.12, 0.25, 7000.0, 0.0);
                self.filter = [
                    f(Band::Hochpass { hz: 300.0 }),
                    f(Band::Hochpass { hz: 300.0 }),
                    f(Band::Tiefpass { hz: 3400.0 }),
                    f(Band::Tiefpass { hz: 3400.0 }),
                    f(Band::Tiefpass { hz: 3800.0 }),
                ];
            }
            Some(Umgebung::Weltall) => {
                // Weit und dunkel: langer Hall ohne Höhen, eine leise Oktave
                // darüber nur im Hall (Schimmer), langsames Schweben.
                self.hall.einstellen(2.5, 6.0, 2800.0, 60.0);
                self.schwebung.einstellen(14.0, 4.0, 0.15, 0.0);
                self.filter = [f(Band::Tiefpass { hz: 6000.0 }), f(Band::Hochpass { hz: 700.0 }), f(Band::Hochpass { hz: 90.0 }), aus, aus];
                self.psola = Some(Psola::new(PsolaEinstellung { halbtoene: 12.0, formant: 4.0 }));
            }
            Some(Umgebung::Megafon) => {
                // Tüte: nur Mitten, hupige Spitze um 1,5 kHz, stark verzerrt.
                // Dazu ein kurzes Echo von Hauswänden.
                self.filter = [
                    f(Band::Hochpass { hz: 500.0 }),
                    f(Band::Hochpass { hz: 500.0 }),
                    f(Band::Glocke { hz: 1500.0, db: 8.0, q: 1.2 }),
                    f(Band::Tiefpass { hz: 4000.0 }),
                    f(Band::Tiefpass { hz: 4500.0 }),
                ];
                self.kamm.einstellen(0.9, 0.3);
                self.echo.einstellen(140.0, 0.25, 2500.0);
                self.hall.einstellen(1.2, 1.0, 3000.0, 25.0);
            }
        }
    }

    /// Sender wie beim Funkgerät: an ab [`FUNK_SCHWELLE`], aus nach
    /// [`FUNK_HALTEN_S`] Stille. Beim Anschalten läuft `klick` mit `start`
    /// Samples los, beim Abschalten `kschh` mit `ende` Samples.
    #[inline]
    fn senden_folgen(&mut self, h: f32, start: usize, ende: usize) {
        if h > FUNK_SCHWELLE {
            if !self.sendet {
                self.sendet = true;
                self.klick = start;
                // Neues Senden beendet einen noch laufenden Schluss-Ton.
                self.kschh = 0;
            }
            self.halten = (FUNK_HALTEN_S * FS) as usize;
        } else if self.sendet {
            self.halten = self.halten.saturating_sub(1);
            if self.halten == 0 {
                self.sendet = false;
                self.kschh = ende;
            }
        }
    }

    /// Piepton des Raumhelms: `rest` Samples übrig von `gesamt`, sanft ein und aus.
    #[inline]
    fn piep(&mut self, rest: usize, hz: f32) -> f32 {
        let gesamt = HELM_TON_S * FS;
        let schon = gesamt - rest as f32;
        let blende = (schon / HELM_TON_BLENDE).min(rest as f32 / HELM_TON_BLENDE).min(1.0);
        self.ring.sinus(hz) * HELM_TON_PEGEL * blende
    }

    #[inline]
    fn huelle_folgen(&mut self, x: f32) -> f32 {
        // Anstieg ca. 2 ms, Abfall ca. 60 ms.
        let a = x.abs();
        let k = if a > self.huelle { 0.99 } else { 0.9997 };
        self.huelle = k * self.huelle + (1.0 - k) * a;
        self.huelle
    }

    /// Die volle Umgebung für ein Sample (ohne Mischung mit trocken).
    #[inline]
    fn nass(&mut self, x: f32, doppel: f32) -> f32 {
        match self.art {
            None => x,
            Some(Umgebung::Halle) => {
                let r = self.hall.schritt(x);
                x * 0.85 + r * 0.9
            }
            Some(Umgebung::Hoehle) => {
                let e = self.echo.schritt(x);
                let r = self.hall.schritt(x + e * 0.5);
                self.filter[0].schritt(x * 0.7 + r + e * 0.6)
            }
            Some(Umgebung::Funk) => {
                let h = self.huelle_folgen(x);
                self.senden_folgen(h, (FUNK_KLICK_S * FS) as usize, (FUNK_KSCHH_S * FS) as usize);
                let mut v = x;
                for b in &mut self.filter[..4] {
                    v = b.schritt(v);
                }
                let v = (v * 5.0).tanh() * 0.45;
                let n = self.rauschen.schritt();
                let mut geraeusch = if self.sendet { n * 0.006 } else { 0.0 };
                if self.klick > 0 {
                    self.klick -= 1;
                    geraeusch += n * 0.2;
                }
                if self.kschh > 0 {
                    let t = self.kschh as f32 / (FUNK_KSCHH_S * FS);
                    self.kschh -= 1;
                    geraeusch += n * 0.12 * t;
                }
                // Nach der Verzerrung nochmal begrenzen: Funk hat keine hohen Töne.
                self.filter[4].schritt(v + geraeusch)
            }
            Some(Umgebung::Telepathie) => {
                let h = self.filter[0].schritt(x);
                let c = self.schwebung.schritt(h);
                let d = self.filter[1].schritt(doppel);
                let m = h * 0.6 + c * 0.5 + d * 0.5;
                let r = self.hall.schritt(m);
                m * 0.8 + r * 0.6
            }
            Some(Umgebung::Roboter) => {
                let rm = x * self.ring.sinus(55.0);
                let v = self.filter[0].schritt(x * 0.25 + rm * 0.9);
                let k = self.kamm.schritt(v);
                (k * 1.6).tanh() * 0.62
            }
            Some(Umgebung::Maske) => {
                let mut v = x;
                for b in &mut self.filter[..4] {
                    v = b.schritt(v);
                }
                self.kamm.schritt(v) * 0.9
            }
            Some(Umgebung::Raumhelm) => {
                let h = self.huelle_folgen(x);
                let oeffnen = (HELM_OEFFNEN_S * FS) as usize;
                self.senden_folgen(h, oeffnen, (HELM_TON_S * FS) as usize);
                let helm = self.kamm.schritt(x) * 0.75;
                let r = self.hall.schritt(helm);
                let mut v = helm + r * 0.5;
                // Rauschen und Ton laufen durch das Funk-Band wie die Stimme:
                // So klingen sie gesendet, nicht darübergelegt.
                let n = self.rauschen.schritt();
                if self.sendet {
                    v += n * 0.004;
                }
                if self.klick > 0 {
                    self.klick -= 1;
                    v += n * 0.04 * (self.klick as f32 / oeffnen as f32);
                }
                if self.kschh > 0 {
                    self.kschh -= 1;
                    v += self.piep(self.kschh, HELM_AUS_HZ);
                }
                for b in &mut self.filter[..4] {
                    v = b.schritt(v);
                }
                let v = (v * 3.0).tanh() * 0.5;
                self.filter[4].schritt(v)
            }
            Some(Umgebung::Weltall) => {
                let c = self.schwebung.schritt(x);
                let nah = x * 0.55 + c * 0.45;
                let schimmer = self.filter[1].schritt(doppel) * 0.3;
                let r = self.hall.schritt(self.filter[2].schritt(nah * 0.6 + schimmer));
                self.filter[0].schritt(nah * 0.6 + r * 1.2)
            }
            Some(Umgebung::Megafon) => {
                let mut v = x;
                for b in &mut self.filter[..3] {
                    v = b.schritt(v);
                }
                let v = self.kamm.schritt((v * 6.0).tanh() * 0.4);
                let v = self.filter[3].schritt(v);
                let e = self.echo.schritt(v);
                let r = self.hall.schritt(v);
                self.filter[4].schritt(v + e * 0.35 + r * 0.25)
            }
        }
    }

    /// Rechnet einen Block und addiert ihn auf `aus`.
    fn verarbeite(&mut self, ein: &[f32], aus: &mut [f32]) -> Result<(), String> {
        let n = ein.len();
        let blende = 1.0 / (BLENDE_S * FS);
        if self.ein.len() != n {
            self.ein.resize(n, 0.0);
            self.doppel.resize(n, 0.0);
        }
        for (e, &x) in self.ein.iter_mut().zip(ein) {
            if self.pegel < self.pegel_ziel {
                self.pegel = (self.pegel + blende).min(self.pegel_ziel);
            } else if self.pegel > self.pegel_ziel {
                self.pegel = (self.pegel - blende).max(self.pegel_ziel);
            }
            *e = x * self.pegel;
        }
        if self.art.is_none() {
            for (a, &e) in aus.iter_mut().zip(&self.ein) {
                *a += e;
            }
        } else {
            if let Some(p) = self.psola.as_mut() {
                self.doppel.copy_from_slice(&self.ein);
                p.verarbeite(&mut self.doppel)?;
            }
            let mut spitze = 0.0f32;
            for i in 0..n {
                if self.staerke != self.staerke_ziel {
                    let d = (self.staerke_ziel - self.staerke).clamp(-blende, blende);
                    self.staerke += d;
                }
                let x = self.ein[i];
                let mut y = self.nass(x, self.doppel[i]);
                if !y.is_finite() {
                    y = 0.0;
                }
                let y = x * (1.0 - self.staerke) + y * self.staerke;
                spitze = spitze.max(y.abs());
                aus[i] += y;
            }
            if self.pegel == 0.0 && self.pegel_ziel == 0.0 {
                self.still = if spitze < STILL { self.still.saturating_add(1) } else { 0 };
            }
            return Ok(());
        }
        if self.pegel == 0.0 && self.pegel_ziel == 0.0 {
            self.still = self.still.saturating_add(1);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- Effekt

/// Schickt die Wahl in den Verarbeitungs-Faden.
pub struct UmgebungSteuerung {
    tx: rtrb::Producer<UmgebungWahl>,
}

impl UmgebungSteuerung {
    /// Gibt `false` zurück, wenn die Warteschlange voll ist.
    pub fn setzen(&mut self, w: UmgebungWahl) -> bool {
        self.tx.push(w).is_ok()
    }
}

pub struct UmgebungEffekt {
    plaetze: Vec<Platz>,
    aktiv: usize,
    /// Wann jeder Platz zuletzt die Stimme bekam (Zähler).
    seit: Vec<u64>,
    zaehler: u64,
    rx: rtrb::Consumer<UmgebungWahl>,
    trocken: Vec<f32>,
}

/// Neuer Umgebungs-Effekt. `start`: Umgebung beim Start (`None` = aus).
pub fn umgebung(start: UmgebungWahl) -> (UmgebungEffekt, UmgebungSteuerung) {
    let (tx, rx) = rtrb::RingBuffer::new(16);
    let mut plaetze: Vec<Platz> = (0..3).map(|_| Platz::new()).collect();
    plaetze[0].einstellen(start);
    plaetze[0].pegel = 1.0;
    plaetze[0].pegel_ziel = 1.0;
    let e = UmgebungEffekt { plaetze, aktiv: 0, seit: vec![0; 3], zaehler: 0, rx, trocken: Vec::new() };
    (e, UmgebungSteuerung { tx })
}

impl UmgebungEffekt {
    fn wechseln(&mut self, w: UmgebungWahl) {
        let a = &mut self.plaetze[self.aktiv];
        if a.art == w.map(|(u, _)| u) {
            // Gleiche Umgebung: nur die Stärke (gleitend).
            if let Some((_, s)) = w {
                a.staerke_ziel = s.clamp(0.0, 1.0);
            }
            return;
        }
        a.pegel_ziel = 0.0;
        // Ein freier Platz, sonst der, der am längsten ausklingt (sein Rest
        // wird abgeschnitten).
        let frei = (0..self.plaetze.len()).filter(|&i| i != self.aktiv).find(|&i| !self.plaetze[i].laeuft());
        let aeltester = (0..self.plaetze.len()).filter(|&i| i != self.aktiv).min_by_key(|&i| self.seit[i]).unwrap_or(0);
        self.aktiv = frei.unwrap_or(aeltester);
        self.zaehler += 1;
        self.seit[self.aktiv] = self.zaehler;
        let n = &mut self.plaetze[self.aktiv];
        n.einstellen(w);
        n.pegel = 0.0;
        n.pegel_ziel = 1.0;
    }
}

impl Effekt for UmgebungEffekt {
    fn verarbeite(&mut self, block: &mut [f32]) -> Result<(), String> {
        while let Ok(w) = self.rx.pop() {
            self.wechseln(w);
        }
        let nachklang = self.plaetze.iter().enumerate().any(|(i, p)| i != self.aktiv && p.laeuft());
        let a = &self.plaetze[self.aktiv];
        // Häufigster Fall: aus und nichts klingt nach. Block bleibt, wie er ist.
        if a.art.is_none() && a.pegel == 1.0 && a.pegel_ziel == 1.0 && !nachklang {
            return Ok(());
        }
        if self.trocken.len() != block.len() {
            self.trocken.resize(block.len(), 0.0);
        }
        self.trocken.copy_from_slice(block);
        block.fill(0.0);
        for p in &mut self.plaetze {
            if p.laeuft() {
                p.verarbeite(&self.trocken, block)?;
            }
        }
        Ok(())
    }

    fn verzoegerung(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::denoise::BLOCK;

    /// Sprach-ähnliches Signal: Grundton 140 Hz mit Obertönen, Silben 4 pro
    /// Sekunde, dazwischen kurze Zisch-Laute.
    fn sprache(sekunden: f32) -> Vec<f32> {
        let n = (sekunden * FS) as usize / BLOCK * BLOCK;
        let mut r = Rauschen(12345);
        (0..n)
            .map(|i| {
                let t = i as f32 / FS;
                let silbe = (0.5 - 0.5 * (TAU * 4.0 * t).cos()).powf(1.5);
                let ton: f32 = (1..=8).map(|k| (TAU * 140.0 * k as f32 * t).sin() / k as f32).sum();
                let zisch = if (t * 4.0).fract() > 0.85 { r.schritt() * 0.3 } else { 0.0 };
                0.12 * silbe * ton + 0.1 * zisch
            })
            .collect()
    }

    fn rechnen(e: &mut UmgebungEffekt, x: &[f32]) -> Vec<f32> {
        let mut y = x.to_vec();
        for b in y.chunks_mut(BLOCK) {
            e.verarbeite(b).unwrap();
        }
        y
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn db(a: f32, b: f32) -> f32 {
        20.0 * (a.max(1e-12) / b.max(1e-12)).log10()
    }

    #[test]
    fn aus_gibt_eingang_unveraendert_aus() {
        let (mut e, _st) = umgebung(None);
        let x = sprache(1.0);
        assert_eq!(rechnen(&mut e, &x), x);
        assert_eq!(e.verzoegerung(), 0);
    }

    #[test]
    fn staerke_null_ist_trocken() {
        let (mut e, _st) = umgebung(Some((Umgebung::Maske, 0.0)));
        let x = sprache(1.0);
        assert_eq!(rechnen(&mut e, &x), x);
    }

    #[test]
    fn jede_umgebung_bleibt_begrenzt_und_aehnlich_laut() {
        let mut x = sprache(4.0);
        x.extend(vec![0.0; 10 * BLOCK * 100]);
        for u in Umgebung::ALLE {
            let (mut e, _st) = umgebung(Some((u, 1.0)));
            let y = rechnen(&mut e, &x);
            assert!(y.iter().all(|s| s.is_finite()), "{u}: nicht endlich");
            let spitze = y.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(spitze < 1.0, "{u}: Spitze {spitze}");
            let sprechen = (4.0 * FS) as usize;
            let l = db(rms(&y[..sprechen]), rms(&x[..sprechen]));
            println!("{u}: {l:+.1} dB, Spitze {spitze:.2}");
            assert!((-6.0..=4.0).contains(&l), "{u}: {l:.1} dB");
            // 10 s nach dem letzten Laut ist alles verklungen.
            let rest = rms(&y[y.len() - BLOCK * 50..]);
            assert!(rest < 1e-4, "{u}: Rest {rest}");
        }
    }

    /// Größter Sprung zwischen zwei Samples.
    fn groesster_sprung(y: &[f32]) -> f32 {
        y.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
    }

    #[test]
    fn umschalten_knackt_nicht() {
        let (mut e, mut st) = umgebung(None);
        let x: Vec<f32> = (0..BLOCK * 400).map(|i| 0.3 * (TAU * 200.0 * i as f32 / FS).sin()).collect();
        let mut y = x.clone();
        let wechsel = [(100, Some((Umgebung::Halle, 0.7))), (200, Some((Umgebung::Maske, 1.0))), (300, None)];
        // Normaler Sprung: im eingeschwungenen Zustand je Abschnitt.
        let mut normal = 0.0f32;
        for (i, b) in y.chunks_mut(BLOCK).enumerate() {
            if let Some(&(_, w)) = wechsel.iter().find(|(k, _)| *k == i) {
                assert!(st.setzen(w));
            }
            e.verarbeite(b).unwrap();
        }
        for k in [50, 150, 250, 350] {
            normal = normal.max(groesster_sprung(&y[BLOCK * k..BLOCK * (k + 40)]));
        }
        for &(k, _) in &wechsel {
            let s = groesster_sprung(&y[BLOCK * (k - 1)..BLOCK * (k + 5)]);
            assert!(s < normal * 1.5, "Wechsel bei Block {k}: Sprung {s}, normal {normal}");
        }
    }

    #[test]
    fn hall_klingt_nach_dem_ausschalten_aus() {
        let (mut e, mut st) = umgebung(Some((Umgebung::Halle, 1.0)));
        let mut x = sprache(1.0);
        let n = x.len();
        x.extend(vec![0.0; BLOCK * 600]);
        let mut y = x.clone();
        for (i, b) in y.chunks_mut(BLOCK).enumerate() {
            if i == n / BLOCK {
                assert!(st.setzen(None));
            }
            e.verarbeite(b).unwrap();
        }
        // 50–500 ms nach dem Ausschalten: Nachhall ist noch zu hören.
        let nach = rms(&y[n + BLOCK * 5..n + BLOCK * 50]);
        assert!(db(nach, rms(&x[..n])) > -40.0, "Nachhall abgeschnitten: {nach}");
        // Nach 6 s ist der Hall verklungen und der Platz wieder frei.
        assert!(e.plaetze.iter().enumerate().all(|(i, p)| i == e.aktiv || !p.laeuft()));
    }

    /// Stärke einer Frequenz in `x` (Goertzel), als Amplitude.
    fn ton_staerke(x: &[f32], hz: f32) -> f32 {
        let k = 2.0 * (TAU * hz / FS).cos();
        let (mut a, mut b) = (0.0f32, 0.0f32);
        for &v in x {
            let c = v + k * a - b;
            b = a;
            a = c;
        }
        ((a * a + b * b - k * a * b).max(0.0)).sqrt() * 2.0 / x.len() as f32
    }

    #[test]
    fn raumhelm_piept_nur_am_ende_und_leise() {
        let (mut e, _st) = umgebung(Some((Umgebung::Raumhelm, 1.0)));
        let mut x = vec![0.0f32; BLOCK * 20];
        let start = x.len();
        x.extend(sprache(1.0));
        let ende = x.len();
        x.extend(vec![0.0; BLOCK * 100]);
        let y = rechnen(&mut e, &x);
        // Vor dem Sprechen: still, kein Ton.
        assert!(rms(&y[..start]) < 1e-6);
        // Anfang: kein Ton über dem ersten Wort.
        let anfang = &y[start..start + 9600];
        assert!(ton_staerke(anfang, HELM_AUS_HZ) < 0.003, "Ton am Anfang: {}", ton_staerke(anfang, HELM_AUS_HZ));
        // Ende: 200 ms nach dem letzten Laut kommt der Aus-Ton.
        let aus_beginn = (ende..y.len() - BLOCK).step_by(BLOCK).find(|&i| ton_staerke(&y[i..i + BLOCK], HELM_AUS_HZ) > 0.005);
        let aus_beginn = aus_beginn.expect("kein Aus-Ton");
        let nach = (aus_beginn - ende) as f32 / FS;
        assert!((0.1..0.6).contains(&nach), "Aus-Ton {nach:.2} s nach dem Ende");
        // Leise: deutlich unter der Stimme (mindestens 12 dB).
        let ton = rms(&y[aus_beginn + BLOCK..aus_beginn + BLOCK * 6]);
        let stimme = rms(&y[start..ende]);
        println!("Aus-Ton {:.1} dB unter der Stimme", db(stimme, ton));
        assert!(db(stimme, ton) > 12.0, "Ton zu laut: {:.1} dB unter der Stimme", db(stimme, ton));
        // Danach ist der Helm still.
        assert!(rms(&y[y.len() - BLOCK * 20..]) < 1e-5);
    }

    #[test]
    fn funk_rauscht_nach_dem_sprechen_kurz() {
        let (mut e, _st) = umgebung(Some((Umgebung::Funk, 1.0)));
        let mut x = sprache(1.0);
        let n = x.len();
        x.extend(vec![0.0; BLOCK * 150]);
        let y = rechnen(&mut e, &x);
        // Ende des Sendens: Pegel fällt, 200 ms Halten, dann 160 ms „Kschh“.
        let s = |ms: f32| n + (ms * 0.001 * FS) as usize;
        let (beginn, kschh) = (s(150.0)..s(800.0))
            .step_by(BLOCK)
            .map(|i| (i, rms(&y[i..i + BLOCK])))
            .fold((0, 0.0f32), |a, b| if b.1 > a.1 { b } else { a });
        println!("Kschh {:.0} ms nach dem letzten Laut, RMS {kschh:.3}", (beginn - n) as f32 / FS * 1000.0);
        assert!(kschh > 0.02, "kein Kschh: {kschh}");
        assert!((beginn - n) as f32 / FS < 0.55, "Kschh zu spät");
        let danach = rms(&y[s(1000.0)..s(1400.0)]);
        assert!(danach < 1e-6, "rauscht weiter: {danach}");
    }
}
