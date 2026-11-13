//! Mitschnitt einer Stufe der Kette als WAV (zur Fehlersuche).
//!
//! Sammelt im Verarbeitungs-Faden (dort darf Speicher angelegt werden) und
//! schreibt beim Beenden eine WAV-Datei (32-Bit-Float, mono, 48 kHz).

use std::io::Write;
use std::path::PathBuf;

use super::pipe::INTERN_RATE;
use super::verarbeitung::Effekt;

pub struct Mitschnitt {
    datei: PathBuf,
    daten: Vec<f32>,
}

impl Mitschnitt {
    pub fn new(datei: PathBuf) -> Self {
        // Platz für 5 Minuten vorab, damit selten nachgelegt werden muss.
        Self { datei, daten: Vec::with_capacity(INTERN_RATE as usize * 300) }
    }
}

impl Effekt for Mitschnitt {
    fn verarbeite(&mut self, block: &mut [f32]) -> Result<(), String> {
        self.daten.extend_from_slice(block);
        Ok(())
    }
    fn verzoegerung(&self) -> usize {
        0
    }
}

impl Drop for Mitschnitt {
    fn drop(&mut self) {
        if let Err(e) = wav_schreiben(&self.datei, &self.daten) {
            eprintln!("Mitschnitt {}: {e}", self.datei.display());
        }
    }
}

/// Schreibt Samples als WAV (Format 3 = IEEE-Float).
pub fn wav_schreiben(datei: &PathBuf, daten: &[f32]) -> std::io::Result<()> {
    let bytes = (daten.len() * 4) as u32;
    let mut f = std::io::BufWriter::new(std::fs::File::create(datei)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + bytes).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&3u16.to_le_bytes())?; // Float
    f.write_all(&1u16.to_le_bytes())?; // mono
    f.write_all(&INTERN_RATE.to_le_bytes())?;
    f.write_all(&(INTERN_RATE * 4).to_le_bytes())?;
    f.write_all(&4u16.to_le_bytes())?;
    f.write_all(&32u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&bytes.to_le_bytes())?;
    for s in daten {
        f.write_all(&s.to_le_bytes())?;
    }
    f.flush()
}
