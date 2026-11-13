//! Bausteine des Beatrice-Netzes, Bild für Bild gerechnet (ein Bild = 10 ms).
//!
//! Alle Faltungen sind kausal: Ein Bild braucht nur frühere Bilder (und bei
//! „Verzögerung“ d ein paar spätere). So lässt sich das Netz live rechnen und
//! gibt dasselbe wie der Trainer, der die ganze Datei auf einmal rechnet.
//! Gewichte liegen wie im Trainer, nach `merge_weights()`: LayerNorm ohne
//! eigene Faktoren, wo der Trainer sie in die nächste Schicht gefaltet hat.

use super::gewichte::{Leser, f16_zu_f32};

// ---------- Rechnen ----------

/// y = W·x. W liegt zeilenweise: [aus][ein].
pub fn matvec(w: &[f32], x: &[f32], y: &mut [f32]) {
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") {
        // SAFETY: AVX2 und FMA sind laut Prüfung vorhanden.
        unsafe { matvec_avx2(w, x, y) };
        return;
    }
    matvec_kern(w, x, y);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn matvec_avx2(w: &[f32], x: &[f32], y: &mut [f32]) {
    let n = x.len();
    for (zeile, y) in w.chunks_exact(n).zip(y.iter_mut()) {
        *y = punkt_fma(zeile, x);
    }
}

/// Wie [`punkt`], mit FMA. Nur in Funktionen mit `fma`-Merkmal aufrufen,
/// sonst wird `mul_add` langsam.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn punkt_fma(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let (a, b) = (&a[..n], &b[..n]);
    let mut s = [0.0f32; 16];
    let voll = n / 16 * 16;
    for (ca, cb) in a[..voll].chunks_exact(16).zip(b[..voll].chunks_exact(16)) {
        for i in 0..16 {
            s[i] = ca[i].mul_add(cb[i], s[i]);
        }
    }
    let mut r = 0.0;
    for v in s {
        r += v;
    }
    for i in voll..n {
        r += a[i] * b[i];
    }
    r
}

#[inline(always)]
fn matvec_kern(w: &[f32], x: &[f32], y: &mut [f32]) {
    let n = x.len();
    debug_assert_eq!(w.len(), n * y.len());
    for (zeile, y) in w.chunks_exact(n).zip(y.iter_mut()) {
        *y = punkt(zeile, x);
    }
}

/// Gewichts-Matrix in fp16, wie im Modell gespeichert (verlustfrei, halber
/// Speicher). Der Prozessor wandelt beim Rechnen um (F16C). Das halbiert den
/// Weg durch den Arbeitsspeicher: Die Gewichte (ca. 18 MB) passen nicht in
/// den Zwischenspeicher und werden alle 10 ms neu gelesen.
pub struct Matrix {
    w: Vec<u16>,
    ein: usize,
}

impl Matrix {
    pub fn neu(w: Vec<u16>, ein: usize) -> Self {
        debug_assert_eq!(w.len() % ein.max(1), 0);
        Self { w, ein }
    }

    /// y = W·x.
    pub fn mal(&self, x: &[f32], y: &mut [f32]) {
        debug_assert_eq!(x.len(), self.ein);
        debug_assert_eq!(self.w.len(), self.ein * y.len());
        #[cfg(target_arch = "x86_64")]
        if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") && std::is_x86_feature_detected!("f16c") {
            // SAFETY: AVX2, FMA und F16C sind laut Prüfung vorhanden.
            unsafe { matvec_f16_avx2(&self.w, x, y) };
            return;
        }
        for (zeile, y) in self.w.chunks_exact(self.ein).zip(y.iter_mut()) {
            *y = zeile.iter().zip(x).map(|(&h, v)| f16_zu_f32(h) * v).sum();
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma,f16c")]
unsafe fn matvec_f16_avx2(w: &[u16], x: &[f32], y: &mut [f32]) {
    use std::arch::x86_64::*;
    let n = x.len();
    let voll = n / 16 * 16;
    for (zeile, y) in w.chunks_exact(n).zip(y.iter_mut()) {
        let mut a0 = _mm256_setzero_ps();
        let mut a1 = _mm256_setzero_ps();
        let mut i = 0;
        while i < voll {
            // SAFETY: i + 16 <= n = zeile.len() = x.len().
            unsafe {
                let h0 = _mm_loadu_si128(zeile.as_ptr().add(i) as *const __m128i);
                let h1 = _mm_loadu_si128(zeile.as_ptr().add(i + 8) as *const __m128i);
                a0 = _mm256_fmadd_ps(_mm256_cvtph_ps(h0), _mm256_loadu_ps(x.as_ptr().add(i)), a0);
                a1 = _mm256_fmadd_ps(_mm256_cvtph_ps(h1), _mm256_loadu_ps(x.as_ptr().add(i + 8)), a1);
            }
            i += 16;
        }
        let s = _mm256_add_ps(a0, a1);
        let hoch = _mm256_extractf128_ps(s, 1);
        let s4 = _mm_add_ps(_mm256_castps256_ps128(s), hoch);
        let s2 = _mm_add_ps(s4, _mm_movehl_ps(s4, s4));
        let s1 = _mm_add_ss(s2, _mm_shuffle_ps(s2, s2, 1));
        let mut r = _mm_cvtss_f32(s1);
        for j in voll..n {
            r += f16_zu_f32(zeile[j]) * x[j];
        }
        *y = r;
    }
}

/// Skalarprodukt mit 16 Teilsummen (der Übersetzer macht daraus SIMD).
#[inline(always)]
pub fn punkt(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let (a, b) = (&a[..n], &b[..n]);
    let mut s = [0.0f32; 16];
    let voll = n / 16 * 16;
    for (ca, cb) in a[..voll].chunks_exact(16).zip(b[..voll].chunks_exact(16)) {
        for i in 0..16 {
            s[i] += ca[i] * cb[i];
        }
    }
    let mut r = 0.0;
    for v in s {
        r += v;
    }
    for i in voll..n {
        r += a[i] * b[i];
    }
    r
}

pub fn gelu(x: &mut [f32]) {
    const K: f32 = 0.797_884_6; // sqrt(2/π)
    for v in x {
        let u = *v;
        *v = 0.5 * u * (1.0 + (K * (u + 0.044715 * u * u * u)).tanh());
    }
}

pub fn silu(x: &mut [f32]) {
    for v in x {
        *v /= 1.0 + (-*v).exp();
    }
}

/// LayerNorm über die Kanäle, ohne eigene Faktoren (eps wie PyTorch).
pub fn normieren(x: &mut [f32]) {
    let n = x.len() as f32;
    let mittel = x.iter().sum::<f32>() / n;
    let var = x.iter().map(|v| (v - mittel) * (v - mittel)).sum::<f32>() / n;
    let f = 1.0 / (var + 1e-5).sqrt();
    for v in x {
        *v = (*v - mittel) * f;
    }
}

pub fn softmax(x: &mut [f32]) {
    let m = x.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));
    let mut s = 0.0;
    for v in x.iter_mut() {
        *v = (*v - m).exp();
        s += *v;
    }
    let f = 1.0 / s;
    for v in x {
        *v *= f;
    }
}

// ---------- Schichten ----------

/// Volle Verbindung (auch 1×1-Faltung).
pub struct Linear {
    w: Matrix,
    pub b: Vec<f32>,
    pub ein: usize,
    pub aus: usize,
}

impl Linear {
    pub fn lesen(l: &mut Leser, ein: usize, aus: usize, bias: bool) -> Result<Self, String> {
        let w = Matrix::neu(l.nimm_roh(ein * aus)?.to_vec(), ein);
        let b = if bias { l.nimm(aus)? } else { Vec::new() };
        Ok(Self { w, b, ein, aus })
    }

    pub fn rechne(&self, x: &[f32], y: &mut [f32]) {
        self.w.mal(x, y);
        for (y, b) in y.iter_mut().zip(&self.b) {
            *y += b;
        }
    }
}

/// LayerNorm mit Faktor und Versatz.
pub struct Norm {
    w: Vec<f32>,
    b: Vec<f32>,
}

impl Norm {
    pub fn lesen(l: &mut Leser, c: usize) -> Result<Self, String> {
        Ok(Self { w: l.nimm(c)?, b: l.nimm(c)? })
    }

    pub fn rechne(&self, x: &mut [f32]) {
        normieren(x);
        for ((v, w), b) in x.iter_mut().zip(&self.w).zip(&self.b) {
            *v = *v * w + b;
        }
    }
}

/// Die letzten `k` Bilder, das neueste zuletzt. Am Anfang Nullen.
pub struct Verlauf {
    bilder: Vec<f32>,
    c: usize,
    k: usize,
    pos: usize,
}

impl Verlauf {
    pub fn new(c: usize, k: usize) -> Self {
        Self { bilder: vec![0.0; c * k], c, k, pos: 0 }
    }

    pub fn schieben(&mut self, x: &[f32]) {
        let p = self.pos;
        self.bilder[p * self.c..(p + 1) * self.c].copy_from_slice(x);
        self.pos = (p + 1) % self.k;
    }

    /// Bild j (0 = ältestes, k-1 = neuestes).
    pub fn bild(&self, j: usize) -> &[f32] {
        let p = (self.pos + j) % self.k;
        &self.bilder[p * self.c..(p + 1) * self.c]
    }
}

/// Kausale Faltung über die Zeit (alle Kanäle). Gewichte je Abgriff als Matrix.
pub struct Faltung {
    w: Vec<Matrix>,
    b: Vec<f32>,
    aus: usize,
    verlauf: Verlauf,
    tmp: Vec<f32>,
}

impl Faltung {
    pub fn lesen(l: &mut Leser, ein: usize, aus: usize, k: usize, bias: bool) -> Result<Self, String> {
        let w = je_abgriff(l.nimm_roh(aus * ein * k)?, ein, aus, k);
        let b = if bias { l.nimm(aus)? } else { vec![0.0; aus] };
        Ok(Self { w, b, aus, verlauf: Verlauf::new(ein, k), tmp: vec![0.0; aus] })
    }

    /// Nimmt das neueste Eingangs-Bild und rechnet ein Ausgangs-Bild.
    pub fn schritt(&mut self, x: &[f32], y: &mut [f32]) {
        self.verlauf.schieben(x);
        y.copy_from_slice(&self.b);
        for (j, w) in self.w.iter().enumerate() {
            w.mal(self.verlauf.bild(j), &mut self.tmp);
            for (y, t) in y.iter_mut().zip(&self.tmp) {
                *y += t;
            }
        }
    }

    pub fn aus(&self) -> usize {
        self.aus
    }
}

/// Kausale Faltung je Kanal (depthwise).
pub struct Tiefenfaltung {
    /// [k][c]
    w: Vec<f32>,
    b: Vec<f32>,
    k: usize,
    verlauf: Verlauf,
}

impl Tiefenfaltung {
    pub fn lesen(l: &mut Leser, c: usize, k: usize) -> Result<Self, String> {
        let roh = l.nimm(c * k)?;
        let mut w = vec![0.0; c * k];
        for ch in 0..c {
            for j in 0..k {
                w[j * c + ch] = roh[ch * k + j];
            }
        }
        Ok(Self { w, b: l.nimm(c)?, k, verlauf: Verlauf::new(c, k) })
    }

    pub fn schritt(&mut self, x: &[f32], y: &mut [f32]) {
        self.verlauf.schieben(x);
        let c = x.len();
        y.copy_from_slice(&self.b);
        for j in 0..self.k {
            let bild = self.verlauf.bild(j);
            for ((y, w), v) in y.iter_mut().zip(&self.w[j * c..(j + 1) * c]).zip(bild) {
                *y += w * v;
            }
        }
    }
}

/// Selbst-Aufmerksamkeit des Lautmerkmal-Netzes. Der Trainer teilt die Bilder
/// in 4 Folgen (Bild t gehört zu Folge t mod 4) und schaut in jeder Folge
/// nur zurück. Hier: je Folge ein Speicher der letzten Schlüssel und Werte.
pub struct Selbst {
    /// in_proj: Zeilen in der Reihenfolge [Kopf][q,k,v][hd].
    ein: Linear,
    aus: Linear,
    koepfe: usize,
    hd: usize,
    max: usize,
    /// Je Folge: Schlüssel und Werte [max][c], Ringpuffer.
    k: [Vec<f32>; 4],
    v: [Vec<f32>; 4],
    anzahl: [usize; 4],
    start: [usize; 4],
    qkv: Vec<f32>,
    werte: Vec<f32>,
    ergebnis: Vec<f32>,
}

impl Selbst {
    pub fn lesen(l: &mut Leser, c: usize, koepfe: usize, max: usize) -> Result<Self, String> {
        let ein = Linear::lesen(l, c, 3 * c, true)?;
        let aus = Linear::lesen(l, c, c, true)?;
        let leer = || vec![0.0; max * c];
        Ok(Self {
            ein,
            aus,
            koepfe,
            hd: c / koepfe,
            max,
            k: [leer(), leer(), leer(), leer()],
            v: [leer(), leer(), leer(), leer()],
            anzahl: [0; 4],
            start: [0; 4],
            qkv: vec![0.0; 3 * c],
            werte: vec![0.0; max],
            ergebnis: vec![0.0; c],
        })
    }

    /// x: normiertes Bild t. Schreibt das Ergebnis nach y.
    pub fn schritt(&mut self, x: &[f32], t: usize, y: &mut [f32]) {
        let (c, hd) = (x.len(), self.hd);
        self.ein.rechne(x, &mut self.qkv);
        let f = t % 4;
        // Neuen Eintrag speichern (ältester fällt raus, wenn voll).
        let platz = if self.anzahl[f] < self.max {
            self.anzahl[f] += 1;
            (self.start[f] + self.anzahl[f] - 1) % self.max
        } else {
            let p = self.start[f];
            self.start[f] = (p + 1) % self.max;
            p
        };
        for h in 0..self.koepfe {
            let basis = h * 3 * hd;
            self.k[f][platz * c + h * hd..platz * c + (h + 1) * hd].copy_from_slice(&self.qkv[basis + hd..basis + 2 * hd]);
            self.v[f][platz * c + h * hd..platz * c + (h + 1) * hd].copy_from_slice(&self.qkv[basis + 2 * hd..basis + 3 * hd]);
        }
        let n = self.anzahl[f];
        for h in 0..self.koepfe {
            let q = &self.qkv[h * 3 * hd..h * 3 * hd + hd];
            for i in 0..n {
                let p = (self.start[f] + i) % self.max;
                self.werte[i] = punkt(q, &self.k[f][p * c + h * hd..p * c + (h + 1) * hd]);
            }
            softmax(&mut self.werte[..n]);
            let ziel = &mut self.ergebnis[h * hd..(h + 1) * hd];
            ziel.fill(0.0);
            for i in 0..n {
                let p = (self.start[f] + i) % self.max;
                let g = self.werte[i];
                for (z, v) in ziel.iter_mut().zip(&self.v[f][p * c + h * hd..p * c + (h + 1) * hd]) {
                    *z += g * v;
                }
            }
        }
        self.aus.rechne(&self.ergebnis, y);
    }
}

/// Gewichte, die aus der Sprecher-Einbettung Schlüssel und Werte machen
/// (`embedding_setter.bin`): je Kopf [hd][kv_c], dazu Versätze.
pub struct KvGewichte {
    kw: Vec<Vec<f32>>,
    vw: Vec<Vec<f32>>,
    kb: Vec<Vec<f32>>,
    vb: Vec<Vec<f32>>,
    hd: usize,
}

/// Schlüssel und Werte je Kopf, je [n][hd].
#[derive(Clone)]
pub struct Kv {
    pub k: Vec<Vec<f32>>,
    pub v: Vec<Vec<f32>>,
}

impl KvGewichte {
    /// Gewichte aus `embedding_setter.bin` (Reihenfolge wie `dump_kv`).
    pub fn lesen(l: &mut Leser, koepfe: usize, hd: usize, kv_c: usize) -> Result<Self, String> {
        let mut g = Self { kw: Vec::new(), vw: Vec::new(), kb: Vec::new(), vb: Vec::new(), hd };
        for _ in 0..koepfe {
            g.kw.push(l.nimm(hd * kv_c)?);
            g.vw.push(l.nimm(hd * kv_c)?);
        }
        for _ in 0..koepfe {
            g.kb.push(l.nimm(hd)?);
            g.vb.push(l.nimm(hd)?);
        }
        Ok(g)
    }

    /// Sprecher-Einbettung [n][kv_c] → Schlüssel und Werte.
    pub fn rechnen(&self, e: &[f32], kv_c: usize) -> Kv {
        let n = e.len() / kv_c;
        let hd = self.hd;
        let koepfe = self.kw.len();
        let mut kv = Kv { k: vec![vec![0.0; n * hd]; koepfe], v: vec![vec![0.0; n * hd]; koepfe] };
        for h in 0..koepfe {
            for i in 0..n {
                let zeile = &e[i * kv_c..(i + 1) * kv_c];
                matvec(&self.kw[h], zeile, &mut kv.k[h][i * hd..(i + 1) * hd]);
                matvec(&self.vw[h], zeile, &mut kv.v[h][i * hd..(i + 1) * hd]);
                for j in 0..hd {
                    kv.k[h][i * hd + j] += self.kb[h][j];
                    kv.v[h][i * hd + j] += self.vb[h][j];
                }
            }
        }
        kv
    }
}

/// Kreuz-Aufmerksamkeit auf die Sprecher-Einbettung (384 Einträge). Schlüssel
/// und Werte hängen nur vom Sprecher ab; sie kommen fertig von außen.
pub struct Kreuz {
    q: Linear,
    aus: Linear,
    koepfe: usize,
    hd: usize,
    kv: Kv,
    n: usize,
    qv: Vec<f32>,
    werte: Vec<f32>,
    ergebnis: Vec<f32>,
}

impl Kreuz {
    pub fn lesen(l: &mut Leser, c: usize, ac: usize, koepfe: usize) -> Result<Self, String> {
        let q = Linear::lesen(l, c, ac, true)?;
        let aus = Linear::lesen(l, ac, c, true)?;
        Ok(Self {
            q,
            aus,
            koepfe,
            hd: ac / koepfe,
            kv: Kv { k: Vec::new(), v: Vec::new() },
            n: 0,
            qv: vec![0.0; ac],
            werte: Vec::new(),
            ergebnis: vec![0.0; ac],
        })
    }

    /// Fertige Schlüssel und Werte setzen (billig, geht im Ton-Faden).
    pub fn kv_setzen(&mut self, kv: Kv) {
        self.n = kv.k.first().map_or(0, |k| k.len() / self.hd);
        self.werte.resize(self.n, 0.0);
        self.kv = kv;
    }

    pub fn schritt(&mut self, x: &[f32], y: &mut [f32]) {
        let hd = self.hd;
        self.q.rechne(x, &mut self.qv);
        for h in 0..self.koepfe {
            let q = &self.qv[h * hd..(h + 1) * hd];
            for i in 0..self.n {
                self.werte[i] = punkt(q, &self.kv.k[h][i * hd..(i + 1) * hd]);
            }
            softmax(&mut self.werte);
            let ziel = &mut self.ergebnis[h * hd..(h + 1) * hd];
            ziel.fill(0.0);
            for i in 0..self.n {
                let g = self.werte[i];
                for (z, v) in ziel.iter_mut().zip(&self.kv.v[h][i * hd..(i + 1) * hd]) {
                    *z += g * v;
                }
            }
        }
        self.aus.rechne(&self.ergebnis, y);
    }
}

pub enum Aufmerksamkeit {
    Keine,
    Selbst(Selbst),
    Kreuz(Kreuz),
}

/// ConvNeXt-Block: (Aufmerksamkeit) → Tiefenfaltung → (Norm) → 1×1 → GELU → 1×1 → Rest.
pub struct Block {
    pub aufmerksamkeit: Aufmerksamkeit,
    dw: Tiefenfaltung,
    pw1: Linear,
    pw2: Linear,
    /// Weight Standardization: ohne LayerNorm.
    ws: bool,
    a: Vec<f32>,
    b: Vec<f32>,
    m: Vec<f32>,
}

/// Art der Aufmerksamkeit beim Lesen.
#[derive(Clone, Copy)]
pub enum Art {
    Keine,
    /// Selbst-Aufmerksamkeit, Köpfe, Speicher je Folge.
    Selbst(usize, usize),
    /// Kreuz-Aufmerksamkeit: Kanäle der Aufmerksamkeit, Köpfe.
    Kreuz(usize, usize),
}

impl Block {
    pub fn lesen(l: &mut Leser, c: usize, zwischen: usize, k: usize, ws: bool, art: Art) -> Result<Self, String> {
        let aufmerksamkeit = match art {
            Art::Keine => Aufmerksamkeit::Keine,
            Art::Selbst(koepfe, max) => Aufmerksamkeit::Selbst(Selbst::lesen(l, c, koepfe, max)?),
            Art::Kreuz(ac, koepfe) => Aufmerksamkeit::Kreuz(Kreuz::lesen(l, c, ac, koepfe)?),
        };
        Ok(Self {
            aufmerksamkeit,
            dw: Tiefenfaltung::lesen(l, c, k)?,
            pw1: Linear::lesen(l, c, zwischen, true)?,
            pw2: Linear::lesen(l, zwischen, c, true)?,
            ws,
            a: vec![0.0; c],
            b: vec![0.0; c],
            m: vec![0.0; zwischen],
        })
    }

    /// Bild t (für die Selbst-Aufmerksamkeit) an Ort und Stelle.
    pub fn schritt(&mut self, x: &mut [f32], t: usize) {
        match &mut self.aufmerksamkeit {
            Aufmerksamkeit::Keine => {}
            Aufmerksamkeit::Selbst(s) => {
                self.a.copy_from_slice(x);
                normieren(&mut self.a);
                s.schritt(&self.a, t, &mut self.b);
                for (v, d) in x.iter_mut().zip(&self.b) {
                    *v += d;
                }
            }
            Aufmerksamkeit::Kreuz(s) => {
                self.a.copy_from_slice(x);
                normieren(&mut self.a);
                s.schritt(&self.a, &mut self.b);
                for (v, d) in x.iter_mut().zip(&self.b) {
                    *v += d;
                }
            }
        }
        self.dw.schritt(x, &mut self.a);
        if !self.ws {
            normieren(&mut self.a);
        }
        self.pw1.rechne(&self.a, &mut self.m);
        gelu(&mut self.m);
        self.pw2.rechne(&self.m, &mut self.b);
        for (v, d) in x.iter_mut().zip(&self.b) {
            *v += d;
        }
    }
}

/// Stapel: Eingangs-Faltung (mit Verzögerung d Bilder) → Norm → Blöcke → Norm.
pub struct Stapel {
    embed: Faltung,
    norm: Option<Norm>,
    pub bloecke: Vec<Block>,
    schluss: Option<Norm>,
    /// So viele Bilder später kommt das erste Ergebnis.
    pub verzoegerung: usize,
    schritte: usize,
    /// Nummer des nächsten Ausgangs-Bilds.
    t: usize,
}

pub struct StapelBau {
    pub ein: usize,
    pub c: usize,
    pub zwischen: usize,
    pub bloecke: usize,
    pub verzoegerung: usize,
    pub k_embed: usize,
    pub k: usize,
    pub ws: bool,
    pub art: Art,
}

impl Stapel {
    pub fn lesen(l: &mut Leser, b: StapelBau) -> Result<Self, String> {
        let embed = Faltung::lesen(l, b.ein, b.c, b.k_embed, true)?;
        let norm = if b.ws { None } else { Some(Norm::lesen(l, b.c)?) };
        let mut bloecke = Vec::with_capacity(b.bloecke);
        for _ in 0..b.bloecke {
            bloecke.push(Block::lesen(l, b.c, b.zwischen, b.k, b.ws, b.art)?);
        }
        let schluss = if b.ws { None } else { Some(Norm::lesen(l, b.c)?) };
        Ok(Self { embed, norm, bloecke, schluss, verzoegerung: b.verzoegerung, schritte: 0, t: 0 })
    }

    /// Neues Eingangs-Bild. Gibt `true` zurück, wenn `y` ein Ausgangs-Bild
    /// enthält (erst nach `verzoegerung` Bildern).
    pub fn schritt(&mut self, x: &[f32], y: &mut [f32]) -> bool {
        self.embed.schritt(x, y);
        self.schritte += 1;
        if self.schritte <= self.verzoegerung {
            return false;
        }
        if let Some(n) = &self.norm {
            n.rechne(y);
        }
        let t = self.t;
        for b in &mut self.bloecke {
            b.schritt(y, t);
        }
        if let Some(n) = &self.schluss {
            n.rechne(y);
        }
        self.t += 1;
        true
    }

    pub fn kanaele(&self) -> usize {
        self.embed.aus()
    }
}

/// Faltung mit Schrittweite (für die Merkmale aus dem Rohsignal). Ohne Versatz.
pub struct Schrittfaltung {
    /// Je Abgriff [aus][ein].
    w: Vec<Matrix>,
    k: usize,
    s: usize,
    verlauf: Verlauf,
    gesehen: usize,
    tmp: Vec<f32>,
    pub aus_bild: Vec<f32>,
}

impl Schrittfaltung {
    pub fn lesen(l: &mut Leser, ein: usize, aus: usize, k: usize, s: usize) -> Result<Self, String> {
        let w = je_abgriff(l.nimm_roh(aus * ein * k)?, ein, aus, k);
        Ok(Self { w, k, s, verlauf: Verlauf::new(ein, k), gesehen: 0, tmp: vec![0.0; aus], aus_bild: vec![0.0; aus] })
    }

    /// Neues Eingangs-Bild. `true`: `aus_bild` ist neu (danach GELU).
    pub fn schieben(&mut self, x: &[f32]) -> bool {
        self.verlauf.schieben(x);
        let i = self.gesehen;
        self.gesehen += 1;
        if i + 1 < self.k || (i + 1 - self.k) % self.s != 0 {
            return false;
        }
        self.aus_bild.fill(0.0);
        for j in 0..self.k {
            self.w[j].mal(self.verlauf.bild(j), &mut self.tmp);
            for (y, t) in self.aus_bild.iter_mut().zip(&self.tmp) {
                *y += t;
            }
        }
        gelu(&mut self.aus_bild);
        true
    }
}

/// Trainer: [aus][ein][k] → je Abgriff eine Matrix [aus][ein].
fn je_abgriff(roh: &[u16], ein: usize, aus: usize, k: usize) -> Vec<Matrix> {
    (0..k)
        .map(|j| {
            let mut w = vec![0u16; aus * ein];
            for o in 0..aus {
                for i in 0..ein {
                    w[o * ein + i] = roh[(o * ein + i) * k + j];
                }
            }
            Matrix::neu(w, ein)
        })
        .collect()
}
