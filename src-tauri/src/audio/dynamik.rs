//! Gate und Kompressor fürs Mikrofon. Beide ohne Vorschau: 0 ms Verzögerung.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

use super::pipe::INTERN_RATE;
use super::regler::Einstellbar;
use super::verarbeitung::Effekt;

fn db_zu_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn lin_zu_db(lin: f32) -> f32 {
    20.0 * lin.max(1e-9).log10()
}

/// Glättungs-Faktor pro Sample für eine Zeitkonstante in ms.
fn koeffizient(ms: f32) -> f32 {
    if ms <= 0.0 { 0.0 } else { (-1.0 / (ms * 0.001 * INTERN_RATE as f32)).exp() }
}

/// Einstellungen des Gates.
#[derive(Clone, Copy, Debug)]
pub struct GateEinstellung {
    /// Ab diesem Pegel öffnet das Gate (dBFS).
    pub schwelle_db: f32,
    /// Das Gate schließt erst so viel dB unter der Schwelle (verhindert Flattern).
    pub hysterese_db: f32,
    /// So lange bleibt das Gate offen, nachdem der Pegel gefallen ist (ms).
    /// Schützt leise Wortenden.
    pub halten_ms: f32,
    /// So lange braucht das Gate, um ganz zu öffnen (ms).
    pub oeffnen_ms: f32,
    /// So lange braucht das Gate, um ganz zu schließen (ms). Es schließt
    /// gleichmäßig in dB, also z. B. 60 dB in 120 ms.
    pub schliessen_ms: f32,
    /// So stark dämpft das geschlossene Gate (dB, z. B. -60).
    pub tiefe_db: f32,
}

impl Default for GateEinstellung {
    fn default() -> Self {
        Self { schwelle_db: -45.0, hysterese_db: 6.0, halten_ms: 150.0, oeffnen_ms: 1.0, schliessen_ms: 120.0, tiefe_db: -60.0 }
    }
}

pub struct Gate {
    oeffnen_lin: f32,
    schliessen_lin: f32,
    halten: u32,
    /// dB-Schritt pro Sample beim Öffnen und Schließen.
    schritt_auf: f32,
    schritt_zu: f32,
    k_huelle: f32,
    tiefe_db: f32,
    huelle: f32,
    offen: bool,
    rest_halten: u32,
    /// Aktuelle Verstärkung in dB (0 = offen, tiefe_db = zu).
    verstaerkung_db: f32,
}

impl Gate {
    pub fn new(e: GateEinstellung) -> Self {
        let tiefe_db = e.tiefe_db.min(0.0);
        let samples = |ms: f32| (ms * 0.001 * INTERN_RATE as f32).max(1.0);
        Self {
            oeffnen_lin: db_zu_lin(e.schwelle_db),
            schliessen_lin: db_zu_lin(e.schwelle_db - e.hysterese_db.max(0.0)),
            halten: (e.halten_ms * 0.001 * INTERN_RATE as f32) as u32,
            schritt_auf: -tiefe_db / samples(e.oeffnen_ms),
            schritt_zu: -tiefe_db / samples(e.schliessen_ms),
            // Hülle fällt in ca. 10 ms ab. Spitzen werden sofort übernommen.
            k_huelle: koeffizient(10.0),
            tiefe_db,
            huelle: 0.0,
            offen: false,
            rest_halten: 0,
            verstaerkung_db: tiefe_db,
        }
    }

    /// Neue Werte. Hülle, offen/zu und aktuelle Dämpfung bleiben.
    fn werte_uebernehmen(&mut self, e: GateEinstellung) {
        let mut neu = Gate::new(e);
        neu.huelle = self.huelle;
        neu.offen = self.offen;
        neu.rest_halten = self.rest_halten.min(neu.halten);
        neu.verstaerkung_db = self.verstaerkung_db.max(neu.tiefe_db);
        *self = neu;
    }

    /// Ist das Gate gerade offen?
    pub fn offen(&self) -> bool {
        self.offen
    }
}

impl Einstellbar for Gate {
    type Werte = GateEinstellung;
    fn einstellen(&mut self, w: GateEinstellung) {
        self.werte_uebernehmen(w);
    }
}

impl Einstellbar for Kompressor {
    type Werte = KompressorEinstellung;
    fn einstellen(&mut self, w: KompressorEinstellung) {
        let absenkung = self.absenkung;
        *self = Kompressor::new(w);
        self.absenkung = absenkung;
    }
}

impl Effekt for Gate {
    fn verarbeite(&mut self, block: &mut [f32]) -> Result<(), String> {
        for x in block.iter_mut() {
            let a = x.abs();
            self.huelle = if a > self.huelle { a } else { self.huelle * self.k_huelle };
            if self.offen {
                if self.huelle >= self.schliessen_lin {
                    self.rest_halten = self.halten;
                } else if self.rest_halten > 0 {
                    self.rest_halten -= 1;
                } else {
                    self.offen = false;
                }
            } else if self.huelle >= self.oeffnen_lin {
                self.offen = true;
                self.rest_halten = self.halten;
            }
            self.verstaerkung_db = if self.offen {
                (self.verstaerkung_db + self.schritt_auf).min(0.0)
            } else {
                (self.verstaerkung_db - self.schritt_zu).max(self.tiefe_db)
            };
            if self.verstaerkung_db < 0.0 {
                *x *= db_zu_lin(self.verstaerkung_db);
            }
        }
        Ok(())
    }
    fn verzoegerung(&self) -> usize {
        0
    }
}

/// Einstellungen des Kompressors.
#[derive(Clone, Copy, Debug)]
pub struct KompressorEinstellung {
    /// Ab diesem Pegel wird leiser geregelt (dBFS).
    pub schwelle_db: f32,
    /// Verhältnis, z. B. 3 = über der Schwelle wird aus 3 dB mehr nur 1 dB mehr.
    pub verhaeltnis: f32,
    /// Weicher Übergang um die Schwelle (dB).
    pub knie_db: f32,
    /// So schnell greift der Kompressor (ms).
    pub angriff_ms: f32,
    /// So schnell lässt er wieder los (ms).
    pub freigabe_ms: f32,
    /// Lautstärke danach anheben (dB).
    pub aufholen_db: f32,
}

impl Default for KompressorEinstellung {
    fn default() -> Self {
        Self { schwelle_db: -20.0, verhaeltnis: 3.0, knie_db: 6.0, angriff_ms: 5.0, freigabe_ms: 100.0, aufholen_db: 3.0 }
    }
}

pub struct Kompressor {
    e: KompressorEinstellung,
    k_angriff: f32,
    k_freigabe: f32,
    /// Aktuelle Absenkung (dB, positiv).
    absenkung: f32,
}

impl Kompressor {
    pub fn new(e: KompressorEinstellung) -> Self {
        Self { e, k_angriff: koeffizient(e.angriff_ms), k_freigabe: koeffizient(e.freigabe_ms), absenkung: 0.0 }
    }

    /// Statische Kennlinie: Absenkung (dB, positiv) für einen Pegel (dB).
    fn kennlinie(&self, pegel: f32) -> f32 {
        let (t, w) = (self.e.schwelle_db, self.e.knie_db.max(0.0));
        let steigung = 1.0 - 1.0 / self.e.verhaeltnis.max(1.0);
        let ueber = pegel - t;
        if 2.0 * ueber < -w {
            0.0
        } else if w > 0.0 && 2.0 * ueber.abs() <= w {
            steigung * (ueber + w / 2.0).powi(2) / (2.0 * w)
        } else {
            steigung * ueber
        }
    }

    /// Aktuelle Absenkung in dB (für eine Anzeige).
    pub fn absenkung_db(&self) -> f32 {
        self.absenkung
    }
}

impl Effekt for Kompressor {
    fn verarbeite(&mut self, block: &mut [f32]) -> Result<(), String> {
        for x in block.iter_mut() {
            let ziel = self.kennlinie(lin_zu_db(x.abs()));
            let k = if ziel > self.absenkung { self.k_angriff } else { self.k_freigabe };
            self.absenkung = ziel + (self.absenkung - ziel) * k;
            *x *= db_zu_lin(self.e.aufholen_db - self.absenkung);
        }
        Ok(())
    }
    fn verzoegerung(&self) -> usize {
        0
    }
}

/// Merkt sich den leisesten und lautesten 10-ms-Block (RMS). Für eine Anzeige.
#[derive(Default)]
pub struct PegelAnzeige {
    // Bits eines positiven f32. Bei positiven Zahlen passt die Reihenfolge der
    // Bits zur Reihenfolge der Werte, darum gehen fetch_min/fetch_max direkt.
    leise: AtomicU32,
    laut: AtomicU32,
}

impl PegelAnzeige {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { leise: AtomicU32::new(u32::MAX), laut: AtomicU32::new(0) })
    }

    /// (leisester, lautester) Block seit dem letzten Aufruf in dBFS.
    pub fn fenster(&self) -> Option<(f32, f32)> {
        let leise = self.leise.swap(u32::MAX, Relaxed);
        let laut = self.laut.swap(0, Relaxed);
        (leise != u32::MAX).then(|| (lin_zu_db(f32::from_bits(leise)), lin_zu_db(f32::from_bits(laut))))
    }
}

/// Misst den Pegel, ändert nichts am Ton.
pub struct Pegelmesser(pub Arc<PegelAnzeige>);

impl Effekt for Pegelmesser {
    fn verarbeite(&mut self, block: &mut [f32]) -> Result<(), String> {
        let rms = (block.iter().map(|x| x * x).sum::<f32>() / block.len().max(1) as f32).sqrt();
        self.0.leise.fetch_min(rms.to_bits(), Relaxed);
        self.0.laut.fetch_max(rms.to_bits(), Relaxed);
        Ok(())
    }
    fn verzoegerung(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = INTERN_RATE as f32;

    fn sinus(hz: f32, db: f32, sekunden: f32) -> Vec<f32> {
        let a = db_zu_lin(db);
        (0..(sekunden * RATE) as usize).map(|i| a * (std::f32::consts::TAU * hz * i as f32 / RATE).sin()).collect()
    }

    fn rms_db(x: &[f32]) -> f32 {
        lin_zu_db((x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt())
    }

    fn verarbeite(e: &mut dyn Effekt, x: &mut [f32]) {
        for b in x.chunks_mut(480) {
            e.verarbeite(b).unwrap();
        }
    }

    #[test]
    fn gate_laesst_sprache_durch_und_daempft_rauschen() {
        let mut g = Gate::new(GateEinstellung::default());
        // Laut (-20 dB) → durch.
        let mut laut = sinus(200.0, -20.0, 0.5);
        verarbeite(&mut g, &mut laut);
        let vorher = rms_db(&sinus(200.0, -20.0, 0.5)[24000..]);
        assert!((rms_db(&laut[24000..]) - vorher).abs() < 0.2);
        // Danach leise (-70 dB) → nach Halten und Schließen um ca. 60 dB gedämpft.
        let mut leise = sinus(200.0, -70.0, 1.0);
        verarbeite(&mut g, &mut leise);
        assert!(!g.offen());
        let rest = rms_db(&leise[38400..]);
        assert!(rest < -70.0 - 3.0 - 55.0, "Rest {rest} dB");
    }

    #[test]
    fn gate_haelt_wortenden() {
        let mut g = Gate::new(GateEinstellung::default());
        let mut laut = sinus(200.0, -20.0, 0.2);
        verarbeite(&mut g, &mut laut);
        // Leises Wortende (-55 dB) direkt danach: in den ersten 100 ms nicht gedämpft.
        let mut ende = sinus(200.0, -55.0, 0.1);
        verarbeite(&mut g, &mut ende);
        assert!(g.offen());
        assert!((rms_db(&ende) - rms_db(&sinus(200.0, -55.0, 0.1))).abs() < 0.5);
    }

    #[test]
    fn gate_flattert_nicht_an_der_schwelle() {
        // Pegel pendelt knapp um die Schwelle (-45 ± 2 dB): Hysterese hält das Gate offen.
        let mut g = Gate::new(GateEinstellung::default());
        let mut start = sinus(200.0, -30.0, 0.1);
        verarbeite(&mut g, &mut start);
        for i in 0..20 {
            let mut x = sinus(200.0, if i % 2 == 0 { -43.0 } else { -47.0 }, 0.05);
            verarbeite(&mut g, &mut x);
            assert!(g.offen(), "geschlossen bei Schritt {i}");
        }
    }

    /// Rechteck: Betrag ist immer gleich. So lässt sich die Kennlinie genau messen.
    fn rechteck(hz: f32, db: f32, sekunden: f32) -> Vec<f32> {
        let a = db_zu_lin(db);
        let halbe = (RATE / hz / 2.0) as usize;
        (0..(sekunden * RATE) as usize).map(|i| if (i / halbe) % 2 == 0 { a } else { -a }).collect()
    }

    #[test]
    fn kompressor_kennlinie() {
        let e = KompressorEinstellung { knie_db: 0.0, aufholen_db: 0.0, ..Default::default() };
        // Unter der Schwelle: unverändert. Darüber: -10 dB bei Ratio 3 → -20 + 10/3.
        for (ein, aus) in [(-30.0, -30.0), (-10.0, -20.0 + 10.0 / 3.0)] {
            let mut k = Kompressor::new(e);
            let mut x = rechteck(100.0, ein, 1.0);
            verarbeite(&mut k, &mut x);
            let gemessen = rms_db(&x[24000..]);
            assert!((gemessen - aus).abs() < 0.05, "ein {ein}: {gemessen} statt {aus}");
        }
        // Aufholen hebt alles an.
        let mut k = Kompressor::new(KompressorEinstellung { knie_db: 0.0, aufholen_db: 3.0, ..Default::default() });
        let mut x = rechteck(100.0, -30.0, 0.5);
        verarbeite(&mut k, &mut x);
        assert!((rms_db(&x[12000..]) + 27.0).abs() < 0.05);
    }

    #[test]
    fn gate_schliesst_in_der_eingestellten_zeit() {
        let mut g = Gate::new(GateEinstellung::default());
        let mut laut = sinus(200.0, -20.0, 0.2);
        verarbeite(&mut g, &mut laut);
        // Stille: Hülle fällt unter die Schwelle, dann 150 ms Halten, dann 120 ms Schließen.
        let mut eins = vec![1e-3f32; (0.5 * RATE) as usize];
        verarbeite(&mut g, &mut eins);
        // 1e-3 = -60 dB liegt unter der Schließ-Schwelle (-51 dB).
        // Nach ca. 0,5 s ist das Gate ganz zu: 1e-3 * 1e-3 = 1e-6.
        assert!((eins[eins.len() - 1] - 1e-6).abs() < 1e-7, "{}", eins[eins.len() - 1]);
        // Die Hülle fällt in ca. 36 ms von -20 auf -51 dB (10 ms Zeitkonstante, 31 dB).
        // Danach 150 ms Halten. Nach weiteren 60 ms ist das Gate halb zu (-30 dB).
        let huelle_s = 10.0e-3 * (10f32.powf(31.0 / 20.0)).ln();
        let mitte = eins[((huelle_s + 0.15 + 0.06) * RATE) as usize];
        assert!(lin_zu_db(mitte / 1e-3) < -20.0 && lin_zu_db(mitte / 1e-3) > -40.0, "{}", lin_zu_db(mitte / 1e-3));
    }

    #[test]
    fn kompressor_knie_ist_stetig() {
        let k = Kompressor::new(KompressorEinstellung::default());
        let mut vorher = k.kennlinie(-40.0);
        for i in 1..400 {
            let jetzt = k.kennlinie(-40.0 + i as f32 * 0.1);
            assert!(jetzt >= vorher && jetzt - vorher < 0.1, "Sprung bei {} dB", -40.0 + i as f32 * 0.1);
            vorher = jetzt;
        }
    }
}
