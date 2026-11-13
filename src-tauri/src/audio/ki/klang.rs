//! Klang-Erzeugung von Beatrice (24 kHz, 240 Samples je Bild):
//! - stimmhaft: an jeder Grundton-Periode eine Impulsantwort (512 Samples)
//!   aus dem Netz, versetzt um den Bruchteil eines Samples;
//! - Rauschen: weißes Rauschen, je Bild im Spektrum geformt;
//! - Nachfilter: beides zusammen durch ein Filter (512) aus dem Netz.
//!
//! Nach Bild f sind die Samples bis zum Anfang von Bild f+1 fertig. Der
//! Trainer schneidet vorne 120 Samples ab; hier kommen sie mit heraus. Die
//! Ausgabe ist also 120 Samples später als beim Trainer.

use std::f32::consts::{PI, TAU};
use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

pub const HOP: usize = 240;
pub const RATE: f32 = 24_000.0;
/// So viele Samples ist die Ausgabe später als beim Trainer.
pub const VORLAUF: usize = 120;
const IR: usize = 512;
const NF: usize = 768;

/// Zufall für Anfangs-Phase und Rauschen. Austauschbar für Tests.
pub trait Zufall: Send {
    /// Gleichverteilt in [0, 1).
    fn naechste(&mut self) -> f32;
}

pub struct Xorshift(pub u64);

impl Zufall for Xorshift {
    fn naechste(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }
}

pub struct Klang {
    ir_inv: Arc<dyn ComplexToReal<f32>>,
    rausch_vor: Arc<dyn RealToComplex<f32>>,
    rausch_inv: Arc<dyn ComplexToReal<f32>>,
    nf_vor: Arc<dyn RealToComplex<f32>>,
    nf_inv: Arc<dyn ComplexToReal<f32>>,
    fenster: Vec<f32>,
    hann: Vec<f32>,
    /// Phase des Grundtons beim letzten Sample [0, 1).
    phase: f64,
    angefangen: bool,
    rauschen_an: bool,
    /// Betrag und Phase der Impulsantwort: letztes und aktuelles Bild.
    vorher: (Vec<f32>, Vec<f32>),
    jetzt: (Vec<f32>, Vec<f32>),
    /// Nachfilter des letzten Bilds (Zeit, mit +1 bei 0).
    nf_vorher: Vec<f32>,
    /// Stimmhafter Teil; Index 1 = Anfang des aktuellen Bilds.
    stimmhaft: Vec<f32>,
    /// Rauschen ab Anfang des aktuellen Bilds.
    rauschen: Vec<f32>,
    anregung: Vec<f32>,
    /// Nach dem Nachfilter, ab Anfang des aktuellen Bilds.
    nach: Vec<f32>,
    spek: Vec<Complex<f32>>,
    zeit: Vec<f32>,
    r_spek: Vec<Complex<f32>>,
    r_zeit: Vec<f32>,
    n_spek: Vec<Complex<f32>>,
    n_zeit: Vec<f32>,
    pf_spek: Vec<Complex<f32>>,
    tmp: Vec<Complex<f32>>,
    pub zufall: Box<dyn Zufall>,
}

impl Klang {
    pub fn new(fenster: Vec<f32>, zufall: Box<dyn Zufall>) -> Self {
        assert_eq!(fenster.len(), IR);
        let mut p = RealFftPlanner::<f32>::new();
        let ir_inv = p.plan_fft_inverse(IR);
        let rausch_vor = p.plan_fft_forward(2 * HOP);
        let rausch_inv = p.plan_fft_inverse(2 * HOP);
        let nf_vor = p.plan_fft_forward(NF);
        let nf_inv = p.plan_fft_inverse(NF);
        let tmp_len = [ir_inv.get_scratch_len(), rausch_vor.get_scratch_len(), rausch_inv.get_scratch_len(), nf_vor.get_scratch_len(), nf_inv.get_scratch_len()]
            .into_iter()
            .max()
            .unwrap_or(0);
        Self {
            spek: ir_inv.make_input_vec(),
            zeit: ir_inv.make_output_vec(),
            r_spek: rausch_vor.make_output_vec(),
            r_zeit: rausch_vor.make_input_vec(),
            n_spek: nf_vor.make_output_vec(),
            n_zeit: nf_vor.make_input_vec(),
            pf_spek: nf_vor.make_output_vec(),
            tmp: vec![Complex::default(); tmp_len],
            ir_inv,
            rausch_vor,
            rausch_inv,
            nf_vor,
            nf_inv,
            fenster,
            hann: (0..2 * HOP).map(|n| 0.5 - 0.5 * (TAU * n as f32 / (2 * HOP) as f32).cos()).collect(),
            phase: 0.0,
            angefangen: false,
            rauschen_an: false,
            vorher: (vec![0.0; IR / 2 + 1], vec![0.0; IR / 2 + 1]),
            jetzt: (vec![0.0; IR / 2 + 1], vec![0.0; IR / 2 + 1]),
            nf_vorher: vec![0.0; IR],
            stimmhaft: vec![0.0; 1 + HOP + IR + HOP],
            rauschen: vec![0.0; 2 * HOP],
            anregung: vec![0.0; HOP],
            nach: vec![0.0; NF + HOP],
            zufall,
        }
    }

    /// Ein Bild. `ir`: 512 Werte aus dem Netz, `ap`: 240, `nf`: 512,
    /// `hz`: Grundton. Schreibt 240 fertige Samples nach `aus`.
    pub fn bild(&mut self, ir: &[f32], ap: &[f32], nf: &[f32], hz: f32, aus: &mut [f32]) {
        // Impulsantwort: Betrag = exp(...), Phase mit Nullen am Rand, jede ungerade + π.
        let n = IR / 2 + 1;
        std::mem::swap(&mut self.vorher, &mut self.jetzt);
        for k in 0..n {
            self.jetzt.0[k] = ir[k].exp();
            let mut ph = if k == 0 || k == n - 1 { 0.0 } else { ir[n + k - 1] };
            if k % 2 == 1 {
                ph += PI;
            }
            self.jetzt.1[k] = ph;
        }

        // Perioden-Marken: Die Phase läuft zwischen Sample m und m+1 über 1.
        let schritt = (hz / RATE) as f64;
        for i in 0..HOP {
            if !self.angefangen {
                self.angefangen = true;
                self.phase = self.zufall.naechste() as f64;
                continue;
            }
            let alt = self.phase;
            let neu = (alt + schritt) % 1.0;
            self.phase = neu;
            if alt > neu {
                let z = (1.0 - alt) as f32;
                let bruch = z / (z + neu as f32);
                // Marke beim Sample davor: relativ zum Bild-Anfang i-1.
                self.marke(i as isize - 1, bruch);
            }
        }

        // Rauschen: Rechteck-Fenster rein, Hann-Fenster raus.
        if !self.rauschen_an {
            // Allererstes Bild: auch die erste Hälfte ist neu.
            self.rauschen_an = true;
            for v in self.anregung.iter_mut() {
                *v = self.zufall.naechste() - 0.5;
            }
        }
        self.r_zeit[..HOP].copy_from_slice(&self.anregung);
        for v in self.r_zeit[HOP..].iter_mut() {
            *v = self.zufall.naechste() - 0.5;
        }
        self.anregung.copy_from_slice(&self.r_zeit[HOP..]);
        let _ = self.rausch_vor.process_with_scratch(&mut self.r_zeit, &mut self.r_spek, &mut self.tmp);
        self.r_spek[0] = Complex::default();
        for (z, a) in self.r_spek[1..].iter_mut().zip(ap) {
            *z *= *a;
        }
        let letzte = self.r_spek.len() - 1;
        self.r_spek[letzte].im = 0.0;
        let _ = self.rausch_inv.process_with_scratch(&mut self.r_spek, &mut self.r_zeit, &mut self.tmp);
        let f = 1.0 / (2 * HOP) as f32;
        for (i, v) in self.r_zeit.iter().enumerate() {
            self.rauschen[i] += v * f * self.hann[i];
        }

        // Nachfilter auf (stimmhaft + Rauschen) dieses Bilds.
        self.n_zeit.fill(0.0);
        for i in 0..HOP {
            self.n_zeit[i] = self.stimmhaft[1 + i] + self.rauschen[i];
        }
        let _ = self.nf_vor.process_with_scratch(&mut self.n_zeit, &mut self.n_spek, &mut self.tmp);
        self.nf_vorher.copy_from_slice(nf);
        self.nf_vorher[0] += 1.0;
        self.n_zeit.fill(0.0);
        self.n_zeit[..IR].copy_from_slice(&self.nf_vorher);
        let _ = self.nf_vor.process_with_scratch(&mut self.n_zeit, &mut self.pf_spek, &mut self.tmp);
        for (a, b) in self.n_spek.iter_mut().zip(&self.pf_spek) {
            *a *= *b;
        }
        let letzte = self.n_spek.len() - 1;
        self.n_spek[0].im = 0.0;
        self.n_spek[letzte].im = 0.0;
        let _ = self.nf_inv.process_with_scratch(&mut self.n_spek, &mut self.n_zeit, &mut self.tmp);
        let f = 1.0 / NF as f32;
        for (i, v) in self.n_zeit.iter().enumerate() {
            self.nach[i] += v * f;
        }

        // Fertig ist alles vor dem nächsten Bild.
        aus.copy_from_slice(&self.nach[..HOP]);

        // Ein Bild weiter.
        schieben(&mut self.nach, HOP);
        schieben(&mut self.stimmhaft, HOP);
        schieben(&mut self.rauschen, HOP);
    }

    /// Impulsantwort ab Sample `r` (relativ zum Anfang des aktuellen Bilds,
    /// -1 = letztes Sample des Bilds davor), um `bruch` Samples verzögert.
    fn marke(&mut self, r: isize, bruch: f32) {
        let (betrag, phase) = if r < 0 { &self.vorher } else { &self.jetzt };
        let n = IR / 2 + 1;
        for k in 0..n {
            let ph = phase[k] - TAU / IR as f32 * k as f32 * bruch;
            self.spek[k] = Complex::from_polar(betrag[k], ph);
        }
        self.spek[0].im = 0.0;
        self.spek[n - 1].im = 0.0;
        let _ = self.ir_inv.process_with_scratch(&mut self.spek, &mut self.zeit, &mut self.tmp);
        let f = 1.0 / IR as f32;
        let start = (r + 1) as usize;
        for (j, (v, w)) in self.zeit.iter().zip(&self.fenster).enumerate() {
            self.stimmhaft[start + j] += v * f * w;
        }
        if r < 0 {
            // Das erste Sample gehört zum letzten Bild, das schon gefiltert
            // ist. Sein Beitrag kommt hier durch dessen Nachfilter dazu
            // (bis auf den Anteil beim schon ausgegebenen Sample).
            let v = self.zeit[0] * f * self.fenster[0];
            for j in 1..IR {
                self.nach[j - 1] += v * self.nf_vorher[j];
            }
        }
    }
}

fn schieben(v: &mut [f32], n: usize) {
    v.copy_within(n.., 0);
    let l = v.len();
    v[l - n..].fill(0.0);
}
