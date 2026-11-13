//! Klick-Töne der Oberfläche. Standard: aus (PLAN.md).
//!
//! Sie spielen nur auf einem fest gewählten Gerät (Kopfhörer), nie auf dem
//! Windows-Standardgerät: Das ist oft ein Wave-Link-Kanal und landet dann im
//! Stream. Die Töne werden hier berechnet, es gibt keine Dateien.

use std::sync::Arc;

use crate::audio::pipe::{INTERN_RATE, Stats};
use crate::audio::sounddatei::SoundDaten;
use crate::audio::soundboard::{Nochmal, SoundEinstellung, Soundboard, mischer};
use crate::audio::wasapi_aus::{self, WasapiAusgang};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ton {
    /// Kurzer Klick (Knopf, Auswahl).
    Klick,
    /// Zwei Töne aufwärts (etwas eingeschaltet).
    An,
    /// Zwei Töne abwärts (etwas ausgeschaltet).
    Aus,
}

impl Ton {
    pub fn aus_text(t: &str) -> Option<Ton> {
        match t {
            "klick" => Some(Ton::Klick),
            "an" => Some(Ton::An),
            "aus" => Some(Ton::Aus),
            _ => None,
        }
    }
}

/// Sinus-Ton mit schnellem Ansatz und abklingendem Ende.
fn ton(teile: &[(f32, f32)], pegel_db: f32) -> SoundDaten {
    let a = 10f32.powf(pegel_db / 20.0);
    let rate = INTERN_RATE as f32;
    let mut samples = Vec::new();
    for &(hz, ms) in teile {
        let n = (ms * 0.001 * rate) as usize;
        for i in 0..n {
            let t = i as f32 / rate;
            let ansatz = (i as f32 / (0.002 * rate)).min(1.0);
            let abklang = (-t / (ms * 0.001 * 0.35)).exp();
            samples.push(a * ansatz * abklang * (std::f32::consts::TAU * hz * t).sin());
        }
    }
    SoundDaten { name: "ui".into(), kanaele: 1, samples }
}

pub struct UiToene {
    _strom: WasapiAusgang,
    sb: Soundboard,
    ids: [u32; 3],
    pub geraet: String,
}

impl UiToene {
    pub fn start(geraet: &str) -> Result<UiToene, String> {
        let (mut m, st) = mischer();
        let strom = wasapi_aus::start(geraet, None, Arc::new(Stats::new()), move |data, kanaele, _| {
            data.fill(0.0);
            m.mische(data, kanaele);
        })?;
        let mut sb = Soundboard::new(vec![st]);
        let e = SoundEinstellung { nochmal: Nochmal::NeuStarten, ..Default::default() };
        let ids = [
            sb.hinzufuegen(ton(&[(1800.0, 25.0)], -26.0), e),
            sb.hinzufuegen(ton(&[(880.0, 60.0), (1320.0, 90.0)], -24.0), e),
            sb.hinzufuegen(ton(&[(1320.0, 60.0), (880.0, 90.0)], -24.0), e),
        ];
        Ok(UiToene { _strom: strom, sb, ids, geraet: geraet.to_string() })
    }

    pub fn spielen(&mut self, t: Ton) {
        self.sb.aufraeumen();
        let id = match t {
            Ton::Klick => self.ids[0],
            Ton::An => self.ids[1],
            Ton::Aus => self.ids[2],
        };
        self.sb.ausloesen(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toene_sind_kurz_und_leise() {
        let k = ton(&[(1800.0, 25.0)], -26.0);
        assert_eq!(k.samples.len(), 1200);
        let spitze = k.samples.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(spitze <= 10f32.powf(-26.0 / 20.0) + 1e-6, "{spitze}");
        // Klingt aus: Ende viel leiser als der Anfang.
        let ende = k.samples[1100..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(ende < spitze * 0.1, "{ende}");
        assert_eq!(Ton::aus_text("an"), Some(Ton::An));
        assert_eq!(Ton::aus_text("x"), None);
    }
}
