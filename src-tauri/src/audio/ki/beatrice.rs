//! Beatrice 2 live: 16 kHz rein, 24 kHz raus, in Bildern zu 10 ms.
//!
//! Ablauf je Bild t (wie `ConverterNetwork.forward` im Trainer):
//! 1. Laut-Merkmale (2,5 ms Vorgriff), auf den Ziel-Sprecher abgebildet (VQ).
//! 2. Tonhöhe (22,5 ms Vorgriff) und Energie (12,5 ms), passend verschoben.
//! 3. Alles zusammen → Vorstufe mit Blick auf die Sprecher-Einbettung
//!    (20 ms Vorgriff) → Impulsantwort, Rauschen und Nachfilter → Klang.
//!
//! Vorgriff der Laute (MaskCue, `glottix_beatrice.json` im Modell-Ordner,
//! `{"vorgriff": n}`): Bild t bekommt die Laut-Merkmale von Bild t+n. So
//! hört die Laut-Erkennung n × 10 ms in die Zukunft, bevor ein Laut
//! entschieden ist; die Verzögerung wächst um genauso viel. Das Modell muss
//! mit derselben Verschiebung trainiert sein (Trainer: GLOTTIX_VORGRIFF).

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;

use super::gewichte::{Leser, ModellInfo, f16_zu_f32, info};
use super::klang::{HOP, Klang, Xorshift, Zufall};
use super::laute::{KANAELE, Laute};
use super::netz::{Art, Aufmerksamkeit, Kv, KvGewichte, Linear, Stapel, StapelBau, punkt, silu};
use super::tonhoehe::{STUFEN, STUFEN_JE_OKTAVE, TonBild, Tonhoehe};

const C: usize = 256;
const CODES: usize = 512;
const TOP: usize = 4;
const KV_LAENGE: usize = 384;
const KV_C: usize = 128;
const FORMANTEN: usize = 9;
/// Größter erlaubter Vorgriff der Laute (Bilder zu 10 ms).
const VORGRIFF_MAX: usize = 10;

#[derive(serde::Deserialize, Default)]
struct GlottixBeatrice {
    #[serde(default)]
    vorgriff: usize,
}

/// Eigene Datei im Modell-Ordner: `maskcue_<name>`, sonst der alte Name
/// `glottix_<name>` (Pakete und Trainer vor der Umbenennung am 01.11.2026).
pub fn modelldatei(ordner: &Path, name: &str) -> std::path::PathBuf {
    let neu = ordner.join(format!("maskcue_{name}"));
    if neu.exists() { neu } else { ordner.join(format!("glottix_{name}")) }
}

/// Vorgriff der Laute aus `glottix_beatrice.json` (fehlt die Datei: 0).
fn vorgriff_lesen(ordner: &Path) -> Result<usize, String> {
    let Ok(t) = std::fs::read_to_string(modelldatei(ordner, "beatrice.json")) else { return Ok(0) };
    let g: GlottixBeatrice = serde_json::from_str(&t).map_err(|e| format!("glottix_beatrice.json: {e}"))?;
    if g.vorgriff > VORGRIFF_MAX {
        return Err(format!("glottix_beatrice.json: Vorgriff {} zu groß (höchstens {VORGRIFF_MAX})", g.vorgriff));
    }
    Ok(g.vorgriff)
}

/// Eigenes, größeres Codebuch aus `glottix_codebuch.bin` (f16, [Stimmen][Codes][128]).
/// Mehr Codes = feinere Laute beim Ersetzen (512 im Original sind für Deutsch zu grob).
fn codebuch_lesen(ordner: &Path, n: usize) -> Result<Option<(Vec<u16>, usize)>, String> {
    let Ok(b) = std::fs::read(modelldatei(ordner, "codebuch.bin")) else { return Ok(None) };
    let je_code = n * KANAELE * 2;
    if b.is_empty() || b.len() % je_code != 0 || b.len() / je_code < TOP {
        return Err(format!("glottix_codebuch.bin: {} Bytes passen nicht zu {n} Stimmen", b.len()));
    }
    Ok(Some((b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect(), b.len() / je_code)))
}

/// Zwischenwerte für den Vergleich mit dem Trainer (nur in Tests).
#[derive(Default)]
pub struct Mitschnitt {
    pub laute: Vec<Vec<f32>>,
    pub energie: Vec<f32>,
    pub logits: Vec<Vec<f32>>,
    pub vorstufe: Vec<Vec<f32>>,
    pub ir: Vec<Vec<f32>>,
    pub ap: Vec<Vec<f32>>,
    pub nf: Vec<Vec<f32>>,
}

/// Alles, was eine Ziel-Stimme ausmacht, für alle Stimmen des Modells.
/// Geteilt (Arc): Der Motor rechnet damit neue Mischungen vor, ohne den
/// laufenden Rechner anzuhalten.
pub struct StimmDaten {
    n: usize,
    sprecher_einbettung: Vec<f32>,
    formant_einbettung: Vec<f32>,
    codebuecher: Vec<u16>,
    /// Codes je Stimme (512, oder aus `glottix_codebuch.bin`).
    codes_n: usize,
    kv_einbettung: Vec<u16>,
    /// Je Block der Vorstufe.
    kv_gewichte: Vec<KvGewichte>,
}

/// Fertig gerechnete Mischung: kann ohne Rechenarbeit gesetzt werden.
pub struct StimmPaket {
    mischung: Vec<(usize, f32)>,
    codes: Vec<Vec<f32>>,
    sprecher: Vec<f32>,
    kv: Vec<Kv>,
}

impl StimmDaten {
    /// Mischung vorrechnen (ca. 50 Mio. Rechenschritte, nicht im Ton-Faden).
    /// Gewichte werden auf Summe 1 gebracht; leere Mischung = Stimme 0.
    pub fn paket(&self, stimmen: &[(usize, f32)]) -> StimmPaket {
        let mut m: Vec<(usize, f32)> = stimmen.iter().filter(|(_, g)| *g > 0.0).map(|&(nr, g)| (nr.min(self.n - 1), g)).collect();
        if m.is_empty() {
            m.push((0, 1.0));
        }
        let summe: f32 = m.iter().map(|(_, g)| g).sum();
        for (_, g) in &mut m {
            *g /= summe;
        }
        let codes = m
            .iter()
            .map(|&(nr, _)| self.codebuecher[nr * self.codes_n * KANAELE..(nr + 1) * self.codes_n * KANAELE].iter().map(|&h| f16_zu_f32(h)).collect())
            .collect();
        let mut e = vec![0.0f32; KV_LAENGE * KV_C];
        let mut sprecher = vec![0.0f32; C];
        for &(nr, g) in &m {
            for (z, &h) in e.iter_mut().zip(&self.kv_einbettung[nr * KV_LAENGE * KV_C..(nr + 1) * KV_LAENGE * KV_C]) {
                *z += g * f16_zu_f32(h);
            }
            for (v, a) in sprecher.iter_mut().zip(&self.sprecher_einbettung[nr * C..(nr + 1) * C]) {
                *v += g * a;
            }
        }
        let kv = self.kv_gewichte.iter().map(|g| g.rechnen(&e, KV_C)).collect();
        StimmPaket { mischung: m, codes, sprecher, kv }
    }
}

pub struct Rechner {
    pub info: ModellInfo,
    laute: Laute,
    ton: Tonhoehe,
    embed_laut: Linear,
    embed_stufe: Vec<f32>,
    embed_merkmale: Linear,
    daten: Arc<StimmDaten>,
    vorstufe: Stapel,
    ir_netz: Stapel,
    ir_post: Linear,
    ap_netz: Stapel,
    ap_post: Linear,
    nf_netz: Stapel,
    nf_post: Linear,
    klang: Klang,

    // Einstellung
    /// Ziel-Stimmen mit Gewicht (Summe 1).
    mischung: Vec<(usize, f32)>,
    formant: f32,
    halbtoene: f32,
    laut_anteil: f32,
    /// Codebuch je Stimme der Mischung [Codes][128].
    codes: Vec<Vec<f32>>,
    /// Gemischte Sprecher-Einbettung (ohne Formant).
    sprecher: Vec<f32>,
    sprecher_vek: Vec<f32>,

    // Warteschlangen (Index des ersten Eintrags, Einträge)
    laut_q: VecDeque<Vec<f32>>,
    laut_basis: usize,
    energie_q: VecDeque<f32>,
    energie_basis: usize,
    ton_q: VecDeque<TonBild>,
    ton_basis: usize,
    /// Nächstes Bild für den Wandler.
    t: usize,
    /// Laut-Merkmale von Bild t + vorgriff für Bild t.
    vorgriff: usize,

    // Zwischenspeicher
    x: Vec<f32>,
    tmp: Vec<f32>,
    y: Vec<f32>,
    h: Vec<f32>,
    ir: Vec<f32>,
    ap: Vec<f32>,
    nf: Vec<f32>,
    sims: Vec<(f32, usize)>,
    aus_bild: Vec<f32>,
    pub mitschnitt: Option<Mitschnitt>,
}

impl Rechner {
    pub fn laden(ordner: &Path) -> Result<Self, String> {
        let info = info(ordner)?;
        let n = info.stimmen.len();
        if n == 0 {
            return Err("Das Modell hat keine Stimme".into());
        }

        let mut l = Leser::datei(&ordner.join("phone_extractor.bin"))?;
        let laute = Laute::lesen(&mut l)?;
        l.fertig()?;

        let mut l = Leser::datei(&ordner.join("pitch_estimator.bin"))?;
        let ton = Tonhoehe::lesen(&mut l)?;
        l.fertig()?;

        let mut l = Leser::datei(&ordner.join("waveform_generator.bin"))?;
        let embed_laut = Linear::lesen(&mut l, KANAELE, C, true)?;
        let embed_stufe = l.nimm(STUFEN * C)?;
        let embed_merkmale = Linear::lesen(&mut l, 4, C, true)?;
        let vorstufe = Stapel::lesen(
            &mut l,
            StapelBau { ein: C, c: C, zwischen: C * 2, bloecke: 4, verzoegerung: 2, k_embed: 7, k: 33, ws: false, art: Art::Kreuz(KV_C, 4) },
        )?;
        let ws = |l: &mut Leser, bloecke| {
            Stapel::lesen(l, StapelBau { ein: C, c: C, zwischen: C * 2, bloecke, verzoegerung: 0, k_embed: 3, k: 33, ws: true, art: Art::Keine })
        };
        let ir_netz = ws(&mut l, 2)?;
        let ir_post = Linear::lesen(&mut l, C, 512, true)?;
        let fenster = l.nimm(512)?;
        let ap_netz = ws(&mut l, 1)?;
        let ap_post = Linear::lesen(&mut l, C, HOP, false)?;
        let nf_netz = ws(&mut l, 1)?;
        let nf_post = Linear::lesen(&mut l, C, 512, false)?;
        l.fertig()?;

        let mut l = Leser::datei(&ordner.join("speaker_embeddings.bin"))?;
        let mut codebuecher = l.nimm_roh(n * CODES * KANAELE)?.to_vec();
        let sprecher_einbettung = l.nimm(n * C)?;
        let formant_einbettung = l.nimm(FORMANTEN * C)?;
        let kv_einbettung = l.nimm_roh(n * KV_LAENGE * KV_C)?.to_vec();
        l.fertig()?;

        let mut l = Leser::datei(&ordner.join("embedding_setter.bin"))?;
        let mut kv_gewichte = Vec::new();
        for _ in &vorstufe.bloecke {
            kv_gewichte.push(KvGewichte::lesen(&mut l, 4, KV_C / 4, KV_C)?);
        }
        l.fertig()?;
        let vorgriff = vorgriff_lesen(ordner)?;
        let mut codes_n = CODES;
        if let Some((c, k)) = codebuch_lesen(ordner, n)? {
            (codebuecher, codes_n) = (c, k);
        }
        let daten = Arc::new(StimmDaten { n, sprecher_einbettung, formant_einbettung, codebuecher, codes_n, kv_einbettung, kv_gewichte });

        let mut r = Self {
            info,
            laute,
            ton,
            embed_laut,
            embed_stufe,
            embed_merkmale,
            daten,
            vorstufe,
            ir_netz,
            ir_post,
            ap_netz,
            ap_post,
            nf_netz,
            nf_post,
            klang: Klang::new(fenster, Box::new(Xorshift(0x9e37_79b9_7f4a_7c15))),
            mischung: Vec::new(),
            formant: 0.0,
            halbtoene: 0.0,
            laut_anteil: 1.0,
            codes: Vec::new(),
            sprecher: vec![0.0; C],
            sprecher_vek: vec![0.0; C],
            laut_q: VecDeque::new(),
            laut_basis: 0,
            energie_q: VecDeque::new(),
            energie_basis: 0,
            ton_q: VecDeque::new(),
            ton_basis: 0,
            t: 0,
            vorgriff,
            x: vec![0.0; C],
            tmp: vec![0.0; C],
            y: vec![0.0; C],
            h: vec![0.0; C],
            ir: vec![0.0; 512],
            ap: vec![0.0; HOP],
            nf: vec![0.0; 512],
            sims: Vec::with_capacity(CODES),
            aus_bild: vec![0.0; HOP],
            mitschnitt: None,
        };
        r.sprecher_setzen(0);
        Ok(r)
    }

    /// Vorgriff der Laute in Bildern (10 ms).
    pub fn vorgriff(&self) -> usize {
        self.vorgriff
    }

    pub fn zufall_setzen(&mut self, z: Box<dyn Zufall>) {
        self.klang.zufall = z;
    }

    /// Ziel-Stimme (Nummer im Modell).
    pub fn sprecher_setzen(&mut self, nr: usize) {
        self.mischung_setzen(&[(nr, 1.0)]);
    }

    /// Mehrere Stimmen mischen: (Nummer, Gewicht). Die Gewichte werden auf
    /// Summe 1 gebracht. Einbettungen werden gewichtet gemittelt; die Laute
    /// bildet jede Stimme für sich ab, danach werden sie gewichtet gemittelt.
    /// Rechnet ca. 20 ms: im Ton-Faden besser [`StimmDaten::paket`] vorher
    /// woanders rechnen und [`Rechner::paket_setzen`] nehmen.
    pub fn mischung_setzen(&mut self, stimmen: &[(usize, f32)]) {
        let p = self.daten.paket(stimmen);
        self.paket_setzen(p);
    }

    /// Fertige Mischung setzen. Das Netz läuft weiter (kein Neustart).
    pub fn paket_setzen(&mut self, p: StimmPaket) {
        self.mischung = p.mischung;
        self.codes = p.codes;
        self.sprecher = p.sprecher;
        let mut kv = p.kv.into_iter();
        for b in &mut self.vorstufe.bloecke {
            if let (Aufmerksamkeit::Kreuz(k), Some(x)) = (&mut b.aufmerksamkeit, kv.next()) {
                k.kv_setzen(x);
            }
        }
        self.sprecher_vek_rechnen();
    }

    /// Stimm-Daten des Modells (zum Vorrechnen neuer Mischungen).
    pub fn stimm_daten(&self) -> Arc<StimmDaten> {
        self.daten.clone()
    }

    /// Formant in Halbtönen (−2 … +2, Schritte 0,5).
    pub fn formant_setzen(&mut self, halbtoene: f32) {
        self.formant = halbtoene.clamp(-2.0, 2.0);
        self.sprecher_vek_rechnen();
    }

    /// Tonhöhe in Halbtönen.
    pub fn tonhoehe_setzen(&mut self, halbtoene: f32) {
        self.halbtoene = halbtoene;
    }

    /// Wie stark die Laute auf den Ziel-Sprecher abgebildet werden (0 … 1).
    /// 1 = nur Laute des Ziel-Sprechers (klingt am meisten nach ihm, aber
    /// mit seinem Akzent). 0 = eigene Laute (eigene Aussprache, aber etwas
    /// von der eigenen Stimme bleibt).
    pub fn laut_anteil_setzen(&mut self, a: f32) {
        self.laut_anteil = a.clamp(0.0, 1.0);
    }

    fn sprecher_vek_rechnen(&mut self) {
        let f = (((self.formant + 2.0) * 2.0).round_ties_even() as usize).min(FORMANTEN - 1);
        let fe = &self.daten.formant_einbettung[f * C..(f + 1) * C];
        for ((v, a), b) in self.sprecher_vek.iter_mut().zip(&self.sprecher).zip(fe) {
            *v = a + b;
        }
    }

    /// Samples mit 16 kHz rein. Fertige Samples (24 kHz) kommen hinten an `aus`.
    pub fn schieben(&mut self, x16: &[f32], aus: &mut Vec<f32>) {
        for &s in x16 {
            if self.laute.schieben(s) {
                let mut bild = self.laute.bild.clone();
                self.vq(&mut bild);
                self.laut_q.push_back(bild);
            }
            let neu = self.ton.schieben(s);
            if let Some(e) = neu.energie {
                self.energie_q.push_back(e);
                if let Some(m) = &mut self.mitschnitt {
                    m.energie.push(e);
                }
            }
            if let Some(t) = neu.ton {
                self.ton_q.push_back(t);
                if let Some(m) = &mut self.mitschnitt {
                    m.logits.push(self.ton.logits.clone());
                }
            }
            while self.wandler_bereit() {
                self.wandler(aus);
            }
        }
    }

    fn energie_index(t: usize) -> usize {
        if t == 0 { 1 } else { t - 1 }
    }

    fn ton_index(t: usize) -> usize {
        match t {
            0 => 2,
            1 => 1,
            _ => t - 2,
        }
    }

    fn wandler_bereit(&self) -> bool {
        let t = self.t;
        t + self.vorgriff < self.laut_basis + self.laut_q.len()
            && Self::energie_index(t) < self.energie_basis + self.energie_q.len()
            && Self::ton_index(t) < self.ton_basis + self.ton_q.len()
    }

    /// Laut-Bild auf die Codes des Ziel-Sprechers abbilden: Mittel der 4 ähnlichsten.
    fn vq(&mut self, x: &mut [f32]) {
        let a = self.laut_anteil;
        let roh = if a < 1.0 { Some(x.to_vec()) } else { None };
        let laenge = x.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
        let q: Vec<f32> = x.iter().map(|v| v / laenge).collect();
        x.fill(0.0);
        for (codes, &(_, g)) in self.codes.iter().zip(&self.mischung) {
            self.sims.clear();
            for (i, code) in codes.chunks_exact(KANAELE).enumerate() {
                self.sims.push((punkt(&q, code), i));
            }
            self.sims.select_nth_unstable_by(TOP - 1, |a, b| b.0.total_cmp(&a.0));
            let f = g / TOP as f32;
            for &(_, i) in &self.sims[..TOP] {
                for (v, c) in x.iter_mut().zip(&codes[i * KANAELE..(i + 1) * KANAELE]) {
                    *v += f * c;
                }
            }
        }
        if let Some(roh) = roh {
            // Mischen auf gleicher Lautstärke (RMS), der Wandler normiert danach.
            let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt().max(1e-6);
            let (fq, fr) = (a / rms(x), (1.0 - a) / rms(&roh));
            for (v, r) in x.iter_mut().zip(&roh) {
                *v = *v * fq + r * fr;
            }
        }
        if let Some(m) = &mut self.mitschnitt {
            m.laute.push(x.to_vec());
        }
    }

    fn stufe_verschoben(&self, b: &TonBild) -> usize {
        let v = (self.halbtoene * (STUFEN_JE_OKTAVE as f32 / 12.0)).round_ties_even() as isize;
        (b.stufe as isize + v).clamp(1, STUFEN as isize - 1) as usize
    }

    fn wandler(&mut self, aus: &mut Vec<f32>) {
        let t = self.t;
        let laut = &self.laut_q[t + self.vorgriff - self.laut_basis];
        let e = self.energie_q[Self::energie_index(t) - self.energie_basis];
        let tb = self.ton_q[Self::ton_index(t) - self.ton_basis];

        // Laut-Bild: RMS auf 1.
        let rms = (laut.iter().map(|v| v * v).sum::<f32>() / laut.len() as f32 + f32::EPSILON).sqrt();
        let laut_n: Vec<f32> = laut.iter().map(|v| v / rms).collect();
        self.embed_laut.rechne(&laut_n, &mut self.x);
        let stufe = self.stufe_verschoben(&tb);
        for (x, s) in self.x.iter_mut().zip(&self.embed_stufe[stufe * C..(stufe + 1) * C]) {
            *x += s;
        }
        self.embed_merkmale.rechne(&[e, tb.stimmlos, tb.halb, tb.doppelt], &mut self.tmp);
        for ((x, a), b) in self.x.iter_mut().zip(&self.tmp).zip(&self.sprecher_vek) {
            *x += a + b;
        }
        silu(&mut self.x);

        if self.vorstufe.schritt(&self.x, &mut self.y) {
            // Vocoder-Bild f = t - 2; Grundton aus der (nicht verschobenen) Tonhöhe von f.
            let f = t - 2;
            let tf = self.ton_q[f - self.ton_basis];
            let hz = 55.0 * 2f32.powf(self.stufe_verschoben(&tf) as f32 / STUFEN_JE_OKTAVE as f32);

            self.ir_netz.schritt(&self.y, &mut self.h);
            silu(&mut self.h);
            self.ir_post.rechne(&self.h, &mut self.ir);
            self.ap_netz.schritt(&self.y, &mut self.h);
            silu(&mut self.h);
            self.ap_post.rechne(&self.h, &mut self.ap);
            self.nf_netz.schritt(&self.y, &mut self.h);
            silu(&mut self.h);
            self.nf_post.rechne(&self.h, &mut self.nf);
            if let Some(m) = &mut self.mitschnitt {
                m.vorstufe.push(self.y.clone());
                m.ir.push(self.ir.clone());
                m.ap.push(self.ap.clone());
                m.nf.push(self.nf.clone());
            }
            self.klang.bild(&self.ir, &self.ap, &self.nf, hz, &mut self.aus_bild);
            aus.extend_from_slice(&self.aus_bild);
        }

        self.t += 1;
        // Alte Einträge weg: gebraucht werden ab jetzt nur Bilder ≥ t-2.
        let behalten = self.t.saturating_sub(2);
        while self.laut_basis < self.t + self.vorgriff && !self.laut_q.is_empty() {
            self.laut_q.pop_front();
            self.laut_basis += 1;
        }
        while self.energie_basis + 1 < behalten && !self.energie_q.is_empty() {
            self.energie_q.pop_front();
            self.energie_basis += 1;
        }
        while self.ton_basis < behalten.saturating_sub(1) && !self.ton_q.is_empty() {
            self.ton_q.pop_front();
            self.ton_basis += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ordner(name: &str) -> std::path::PathBuf {
        let o = std::env::temp_dir().join(format!("maskcue_beatrice_test_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&o);
        std::fs::create_dir_all(&o).unwrap();
        o
    }

    #[test]
    fn codebuch_aus_datei() {
        let o = ordner("cb");
        assert_eq!(codebuch_lesen(&o, 2).unwrap(), None);
        // 2 Stimmen × 1024 Codes × 128 Kanäle, f16.
        std::fs::write(o.join("glottix_codebuch.bin"), vec![0u8; 2 * 1024 * KANAELE * 2]).unwrap();
        let (c, k) = codebuch_lesen(&o, 2).unwrap().unwrap();
        assert_eq!((c.len(), k), (2 * 1024 * KANAELE, 1024));
        // Passt nicht zu 3 Stimmen.
        assert!(codebuch_lesen(&o, 3).is_err());
        // Weniger Codes als TOP geht nicht.
        std::fs::write(o.join("glottix_codebuch.bin"), vec![0u8; (TOP - 1) * KANAELE * 2]).unwrap();
        assert!(codebuch_lesen(&o, 1).is_err());
        let _ = std::fs::remove_dir_all(&o);
    }

    #[test]
    fn neuer_dateiname_gewinnt_gegen_alten() {
        let o = ordner("maskcue_name");
        std::fs::write(o.join("glottix_beatrice.json"), r#"{"vorgriff": 3}"#).unwrap();
        assert_eq!(vorgriff_lesen(&o).unwrap(), 3);
        std::fs::write(o.join("maskcue_beatrice.json"), r#"{"vorgriff": 5}"#).unwrap();
        assert_eq!(vorgriff_lesen(&o).unwrap(), 5);
        let _ = std::fs::remove_dir_all(&o);
    }

    #[test]
    fn vorgriff_aus_datei() {
        let o = ordner("vg");
        assert_eq!(vorgriff_lesen(&o).unwrap(), 0);
        std::fs::write(o.join("glottix_beatrice.json"), r#"{"vorgriff": 3}"#).unwrap();
        assert_eq!(vorgriff_lesen(&o).unwrap(), 3);
        std::fs::write(o.join("glottix_beatrice.json"), r#"{"vorgriff": 11}"#).unwrap();
        assert!(vorgriff_lesen(&o).is_err());
        let _ = std::fs::remove_dir_all(&o);
    }
}
