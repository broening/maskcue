//! Tonhöhen-Schätzer von Beatrice: Merkmale aus 35-ms-Fenstern (16 kHz),
//! dann ein ConvNeXt-Netz, das 448 Tonhöhen-Stufen bewertet
//! (96 Stufen je Oktave ab 55 Hz, Stufe 0 = stimmlos).

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

use super::gewichte::Leser;
use super::netz::{Art, Linear, Stapel, StapelBau, gelu, softmax};

const HOP: usize = 160;
const FENSTER: usize = 560;
const MAX_KORR: usize = 256;
const KORR_FENSTER: usize = 304;
const BINS: usize = 64;
const RAND: usize = (FENSTER - HOP) / 2;
pub const STUFEN: usize = 448;
pub const STUFEN_JE_OKTAVE: usize = 96;
const BAND: usize = 4;

/// Ergebnis für ein Bild.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TonBild {
    /// Stufe 1..447 (nie 0, wie im Trainer).
    pub stufe: usize,
    pub stimmlos: f32,
    pub halb: f32,
    pub doppelt: f32,
}

pub struct Tonhoehe {
    /// Eingang mit 200 Nullen vorne, die letzten FENSTER Samples.
    puffer: Vec<f32>,
    gesehen: usize,
    fft: Arc<dyn RealToComplex<f32>>,
    fft_ein: Vec<f32>,
    fft_aus: Vec<Complex<f32>>,
    fft_tmp: Vec<Complex<f32>>,
    vorher: Option<Vec<Complex<f32>>>,
    kosinus: Vec<f32>,
    if0: Linear,
    if1: Linear,
    k0: Linear,
    k1: Linear,
    stapel: Stapel,
    kopf: Linear,
    merkmal_if: Vec<f32>,
    merkmal_k: Vec<f32>,
    a: Vec<f32>,
    b: Vec<f32>,
    x: Vec<f32>,
    y: Vec<f32>,
    pub logits: Vec<f32>,
}

/// Was nach einem Sample neu ist.
#[derive(Default)]
pub struct TonNeu {
    pub energie: Option<f32>,
    pub ton: Option<TonBild>,
}

impl Tonhoehe {
    pub fn lesen(l: &mut Leser) -> Result<Self, String> {
        let c = 192;
        let if0 = Linear::lesen(l, BINS * 3, c, true)?;
        let if1 = Linear::lesen(l, c, c, true)?;
        let k0 = Linear::lesen(l, MAX_KORR, c, true)?;
        let k1 = Linear::lesen(l, c, c, true)?;
        let stapel = Stapel::lesen(
            l,
            StapelBau { ein: c, c, zwischen: c * 2, bloecke: 9, verzoegerung: 1, k_embed: 3, k: 33, ws: false, art: Art::Keine },
        )?;
        let kopf = Linear::lesen(l, c, STUFEN, true)?;
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FENSTER);
        let mut t = Self {
            puffer: vec![0.0; FENSTER],
            gesehen: 0,
            fft_ein: fft.make_input_vec(),
            fft_aus: fft.make_output_vec(),
            fft_tmp: fft.make_scratch_vec(),
            fft,
            vorher: None,
            kosinus: (0..FENSTER).map(|n| (std::f32::consts::PI * (n as f32 + 0.5) / FENSTER as f32).sin()).collect(),
            if0,
            if1,
            k0,
            k1,
            stapel,
            kopf,
            merkmal_if: vec![0.0; BINS * 3],
            merkmal_k: vec![0.0; MAX_KORR],
            a: vec![0.0; c],
            b: vec![0.0; c],
            x: vec![0.0; c],
            y: vec![0.0; c],
            logits: vec![0.0; STUFEN],
        };
        // Nullen vorne wie im Trainer.
        for _ in 0..RAND {
            t.schieben(0.0);
        }
        Ok(t)
    }

    /// Ein Sample (16 kHz). Alle 160 Samples kommt ein Energie-Wert und,
    /// ein Bild später, eine Tonhöhe.
    pub fn schieben(&mut self, s: f32) -> TonNeu {
        self.puffer.copy_within(1.., 0);
        self.puffer[FENSTER - 1] = s;
        self.gesehen += 1;
        let mut neu = TonNeu::default();
        if self.gesehen < FENSTER || (self.gesehen - FENSTER) % HOP != 0 {
            return neu;
        }
        neu.energie = Some(self.merkmale());
        gelu(&mut self.a);
        self.if1.rechne(&self.a, &mut self.x);
        gelu(&mut self.b);
        self.k1.rechne(&self.b, &mut self.y);
        for (x, y) in self.x.iter_mut().zip(&self.y) {
            *x += y;
        }
        gelu(&mut self.x);
        if self.stapel.schritt(&self.x, &mut self.y) {
            self.kopf.rechne(&self.y, &mut self.logits);
            neu.ton = Some(stufe_waehlen(&self.logits));
        }
        neu
    }

    /// Merkmale des aktuellen Fensters. Schreibt die ersten Schichten nach
    /// `a` (Spektrum) und `b` (Korrelation), gibt die Energie zurück.
    fn merkmale(&mut self) -> f32 {
        let f = &self.puffer;
        // Spektrum: log-Leistung und Phasen-Änderung zum letzten Bild.
        self.fft_ein.copy_from_slice(f);
        let _ = self.fft.process_with_scratch(&mut self.fft_ein, &mut self.fft_aus, &mut self.fft_tmp);
        let spek = &self.fft_aus[..BINS];
        for (i, z) in spek.iter().enumerate() {
            self.merkmal_if[i] = (z.norm() + 1e-5).log10();
        }
        match &self.vorher {
            Some(v) => {
                for i in 0..BINS {
                    let d = spek[i] * v[i].conj();
                    let d = d / (d.norm() + 1e-5);
                    self.merkmal_if[BINS + i] = d.re;
                    self.merkmal_if[2 * BINS + i] = d.im;
                }
            }
            None => self.merkmal_if[BINS..].fill(0.0),
        }
        self.vorher = Some(spek.to_vec());

        // Differenz-Funktion (wie YIN) für 256 Verschiebungen.
        let hinten = &f[FENSTER - KORR_FENSTER..];
        let e0: f32 = hinten.iter().map(|v| v * v).sum();
        // Energie des verschobenen Fensters f[255-τ .. 559-τ), laufend nachgeführt.
        let mut e: f32 = f[MAX_KORR - 1..MAX_KORR - 1 + KORR_FENSTER].iter().map(|v| v * v).sum();
        for tau in 0..MAX_KORR {
            let start = MAX_KORR - 1 - tau;
            if tau > 0 {
                e += f[start] * f[start] - f[start + KORR_FENSTER] * f[start + KORR_FENSTER];
            }
            let c = super::netz::punkt(&f[start..start + KORR_FENSTER], hinten);
            let d = (e0 + e - 2.0 * c).max(0.0) * (2.0 / KORR_FENSTER as f32);
            self.merkmal_k[tau] = d.sqrt();
        }

        let energie: f32 = f.iter().zip(&self.kosinus).map(|(v, w)| (v * w) * (v * w)).sum();
        self.if0.rechne(&self.merkmal_if, &mut self.a);
        self.k0.rechne(&self.merkmal_k, &mut self.b);
        energie.max(1e-3).log10() * 0.5
    }
}

/// `sample_pitch` des Trainers für ein Bild.
pub fn stufe_waehlen(logits: &[f32]) -> TonBild {
    let mut p = logits.to_vec();
    softmax(&mut p);
    let stimmlos = p[0];
    p[0] = -100.0;
    let n_band = STUFEN - BAND + 1;
    let mut band = [0.0f32; STUFEN];
    for i in 0..n_band {
        band[i] = p[i..i + BAND].iter().sum();
    }
    // Erstes Maximum, wie torch.argmax.
    let mut q = 0;
    for i in 1..n_band {
        if band[i] > band[q] {
            q = i;
        }
    }
    let staerke = band[q];
    let halb = if q <= STUFEN_JE_OKTAVE { 0.0 } else { band[(q - STUFEN_JE_OKTAVE).max(1)] / (staerke + 1e-6) };
    let doppelt = if q > STUFEN - BAND - STUFEN_JE_OKTAVE {
        0.0
    } else {
        band[(q + STUFEN_JE_OKTAVE).min(STUFEN - BAND)] / (staerke + 1e-6)
    };
    let mut stufe = q;
    for i in q..q + BAND {
        if p[i] > p[stufe] {
            stufe = i;
        }
    }
    TonBild { stufe, stimmlos, halb, doppelt }
}
