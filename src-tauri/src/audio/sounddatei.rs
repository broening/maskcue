//! Sound-Dateien laden (MP3, WAV, OGG, FLAC) und auf 48 kHz bringen.
//!
//! Läuft nie im Ton-Faden. Das Ergebnis liegt ganz im Speicher, damit der
//! Mischer nur noch addieren muss.

use std::path::Path;

use rubato::{FftFixedIn, Resampler};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use super::pipe::INTERN_RATE;

/// Längste Datei, die geladen wird (Sekunden). 10 Minuten Stereo ≈ 230 MB.
pub const MAX_SEKUNDEN: f64 = 600.0;

/// Erlaubte Abtastraten. Die Rate steht in der Datei und kann gefälscht sein:
/// Eine Rate wie 4 294 967 291 Hz ließe die Umrechnung Gigabytes anfordern,
/// eine sehr hohe Rate ließe die 10-Minuten-Grenze erst nach Gigabytes greifen.
pub const RATE_MIN: u32 = 8_000;
pub const RATE_MAX: u32 = 192_000;
/// Mehr Kanäle hat keine normale Ton-Datei (FLAC kann höchstens 8).
pub const KANAELE_MAX: usize = 8;

/// Fertig geladener Sound: 48 kHz, 1 oder 2 Kanäle, verschachtelt.
#[derive(Debug)]
pub struct SoundDaten {
    pub name: String,
    pub kanaele: usize,
    pub samples: Vec<f32>,
}

impl SoundDaten {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.kanaele.max(1)
    }

    pub fn sekunden(&self) -> f64 {
        self.frames() as f64 / INTERN_RATE as f64
    }
}

/// Prüft Rate und Kanäle aus der Datei.
fn format_pruefen(rate: u32, kanaele: usize) -> Result<(), String> {
    if !(RATE_MIN..=RATE_MAX).contains(&rate) {
        return Err(format!("Abtastrate {rate} Hz geht nicht ({RATE_MIN}–{RATE_MAX} Hz)"));
    }
    if kanaele == 0 || kanaele > KANAELE_MAX {
        return Err(format!("{kanaele} Kanäle gehen nicht (höchstens {KANAELE_MAX})"));
    }
    Ok(())
}

pub fn laden(pfad: &Path) -> Result<SoundDaten, String> {
    let name = pfad.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let fehler = |t: String| format!("{name}: {t}");
    let datei = std::fs::File::open(pfad).map_err(|e| fehler(e.to_string()))?;
    let mss = MediaSourceStream::new(Box::new(datei), Default::default());
    let mut hint = Hint::new();
    if let Some(endung) = pfad.extension().and_then(|e| e.to_str()) {
        hint.with_extension(endung);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| fehler(format!("Format nicht erkannt ({e})")))?;
    let track = format.default_track(TrackType::Audio).ok_or_else(|| fehler("keine Tonspur".into()))?;
    let track_id = track.id;
    let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or_else(|| fehler("keine Ton-Parameter".into()))?;
    // Früh prüfen, was der Kopf der Datei sagt (vor dem Entpacken).
    if let Some(r) = params.sample_rate {
        format_pruefen(r, params.channels.as_ref().map_or(1, |c| c.count())).map_err(fehler)?;
    }
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| fehler(format!("Codec nicht unterstützt ({e})")))?;

    // Rate und Kanäle des ersten Blocks; alle weiteren müssen gleich sein.
    let mut spec: Option<(u32, usize)> = None;
    // Schon beim Entpacken nur links/rechts behalten: Mehr Kanäle kosten so
    // keinen Speicher.
    let mut ebenen: Vec<Vec<f32>> = Vec::new();
    let mut puffer: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(Error::ResetRequired) => break,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(fehler(e.to_string())),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                let jetzt = (buf.spec().rate(), buf.spec().channels().count());
                match spec {
                    None => {
                        format_pruefen(jetzt.0, jetzt.1).map_err(fehler)?;
                        spec = Some(jetzt);
                        ebenen = vec![Vec::new(); jetzt.1.min(2)];
                    }
                    Some(s) if s != jetzt => return Err(fehler("Rate oder Kanäle wechseln in der Datei".into())),
                    Some(_) => {}
                }
                let (rate, kanaele_datei) = jetzt;
                puffer.resize(buf.samples_interleaved(), 0.0);
                buf.copy_to_slice_interleaved(&mut puffer);
                for frame in puffer.chunks_exact(kanaele_datei) {
                    for (e, v) in ebenen.iter_mut().zip(frame) {
                        e.push(*v);
                    }
                }
                // Rate ist hier geprüft (höchstens RATE_MAX): Die Grenze greift
                // nach höchstens 10 min × 192 kHz × 2 Kanäle ≈ 0,9 GB.
                if ebenen[0].len() as f64 / rate as f64 > MAX_SEKUNDEN {
                    return Err(fehler(format!("länger als {} Minuten", MAX_SEKUNDEN / 60.0)));
                }
            }
            Err(Error::DecodeError(_)) | Err(Error::IoError(_)) => continue,
            Err(e) => return Err(fehler(e.to_string())),
        }
    }
    let Some((rate, _)) = spec else { return Err(fehler("keine Samples gefunden".into())) };
    if ebenen[0].is_empty() {
        return Err(fehler("keine Samples gefunden".into()));
    }
    let kanaele = ebenen.len();

    if rate != INTERN_RATE {
        ebenen = umrechnen(&ebenen, rate as usize).map_err(fehler)?;
    }
    let frames = ebenen[0].len();
    let mut samples = Vec::with_capacity(frames * kanaele);
    for i in 0..frames {
        for e in &ebenen {
            samples.push(e[i]);
        }
    }
    Ok(SoundDaten { name, kanaele, samples })
}

/// Rechnet jede Ebene von `rate` auf 48 kHz um (FFT-Umrechnung, hohe Qualität).
fn umrechnen(ebenen: &[Vec<f32>], rate: usize) -> Result<Vec<Vec<f32>>, String> {
    // Schutz auch für Aufrufer ohne `laden`: Rubato legt Puffer der Größe
    // rate / ggT(rate, 48000) an.
    if !(RATE_MIN as usize..=RATE_MAX as usize).contains(&rate) {
        return Err(format!("Abtastrate {rate} Hz geht nicht"));
    }
    let kanaele = ebenen.len();
    let laenge = ebenen[0].len();
    let block = 1024;
    let mut r = FftFixedIn::<f32>::new(rate, INTERN_RATE as usize, block, 2, kanaele).map_err(|e| e.to_string())?;
    let verzoegerung = r.output_delay();
    let soll = (laenge as f64 * INTERN_RATE as f64 / rate as f64).round() as usize;
    let mut aus: Vec<Vec<f32>> = vec![Vec::with_capacity(soll + verzoegerung + 2 * block); kanaele];
    let mut pos = 0;
    while pos + block <= laenge {
        let stueck: Vec<&[f32]> = ebenen.iter().map(|e| &e[pos..pos + block]).collect();
        let ergebnis = r.process(&stueck, None).map_err(|e| e.to_string())?;
        for (a, e) in aus.iter_mut().zip(ergebnis) {
            a.extend(e);
        }
        pos += block;
    }
    // Rest und Ausklingen: mit Stille auffüllen, bis die Soll-Länge erreicht ist.
    // Ein leerer Rest (Länge genau ein Vielfaches des Blocks) darf nicht an
    // Rubato gehen, sonst bricht es ab.
    if pos < laenge {
        let rest: Vec<&[f32]> = ebenen.iter().map(|e| &e[pos..]).collect();
        let ergebnis = r.process_partial(Some(&rest), None).map_err(|e| e.to_string())?;
        for (a, e) in aus.iter_mut().zip(ergebnis) {
            a.extend(e);
        }
    }
    while aus[0].len() < soll + verzoegerung {
        let ergebnis = r.process_partial::<&[f32]>(None, None).map_err(|e| e.to_string())?;
        for (a, e) in aus.iter_mut().zip(ergebnis) {
            a.extend(e);
        }
    }
    Ok(aus.into_iter().map(|a| a[verzoegerung..verzoegerung + soll].to_vec()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::mitschnitt::wav_schreiben;
    use std::path::PathBuf;

    fn sinus(rate: f32, hz: f32, sekunden: f32) -> Vec<f32> {
        (0..(rate * sekunden) as usize).map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / rate).sin()).collect()
    }

    /// WAV (Float) mit beliebiger Rate und Kanalzahl von Hand schreiben.
    fn wav_roh(pfad: &PathBuf, rate: u32, kanaele: u16, x: &[f32]) {
        let mut b = Vec::new();
        b.extend(b"RIFF");
        b.extend((36 + x.len() as u32 * 4).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(3u16.to_le_bytes());
        b.extend(kanaele.to_le_bytes());
        b.extend(rate.to_le_bytes());
        b.extend(rate.wrapping_mul(4 * kanaele as u32).to_le_bytes());
        b.extend((4 * kanaele).to_le_bytes());
        b.extend(32u16.to_le_bytes());
        b.extend(b"data");
        b.extend((x.len() as u32 * 4).to_le_bytes());
        for v in x {
            b.extend(v.to_le_bytes());
        }
        std::fs::write(pfad, b).unwrap();
    }

    #[test]
    fn wav_48k_wird_unveraendert_geladen() {
        let ordner = std::env::temp_dir().join("maskcue_test_sound_48");
        std::fs::create_dir_all(&ordner).unwrap();
        let pfad = ordner.join("ton.wav");
        let x = sinus(48_000.0, 440.0, 0.5);
        wav_schreiben(&pfad, &x).unwrap();
        let s = laden(&pfad).unwrap();
        assert_eq!(s.kanaele, 1);
        assert_eq!(s.samples.len(), x.len());
        assert!(s.samples.iter().zip(&x).all(|(a, b)| (a - b).abs() < 1e-6));
    }

    #[test]
    fn fremde_rate_wird_umgerechnet() {
        let ordner = std::env::temp_dir().join("maskcue_test_sound_441");
        std::fs::create_dir_all(&ordner).unwrap();
        let pfad = ordner.join("ton.wav");
        // WAV mit 44,1 kHz von Hand schreiben (wav_schreiben schreibt immer 48 kHz).
        let x = sinus(44_100.0, 1000.0, 1.0);
        wav_roh(&pfad, 44_100, 1, &x);
        let s = laden(&pfad).unwrap();
        assert_eq!(s.samples.len(), 48_000);
        // Frequenz bleibt 1000 Hz: 1000 Nulldurchgänge nach oben in 1 s (± Rand).
        let n = s.samples[100..47_900].windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert!((n as i32 - 996).abs() <= 3, "{n}");
        // Pegel bleibt gleich (Spitze ca. 0,5).
        let spitze = s.samples[1000..47_000].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((spitze - 0.5).abs() < 0.02, "{spitze}");
    }

    #[test]
    fn laenge_genau_vielfaches_des_blocks() {
        // Gefunden mit Dans Dateien: Rest von 0 Samples am Ende ließ die
        // Umrechnung abbrechen.
        for frames in [1024 * 40, 1024 * 40 + 1, 1023] {
            let ebene: Vec<f32> = (0..frames).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
            let aus = umrechnen(&[ebene.clone(), ebene], 44_100).expect("Umrechnung");
            let soll = (frames as f64 * 48_000.0 / 44_100.0).round() as usize;
            assert_eq!(aus[0].len(), soll, "{frames} Frames");
            assert_eq!(aus[1].len(), soll);
        }
    }

    #[test]
    fn kaputte_datei_gibt_fehler() {
        let ordner = std::env::temp_dir().join("maskcue_test_sound_kaputt");
        std::fs::create_dir_all(&ordner).unwrap();
        let pfad = ordner.join("kaputt.mp3");
        std::fs::write(&pfad, b"das ist kein mp3").unwrap();
        assert!(laden(&pfad).is_err());
    }

    // Neu: gefälschte Rate und Kanäle.

    #[test]
    fn falsche_rate_gibt_fehler() {
        let ordner = std::env::temp_dir().join("maskcue_test_sound_rate");
        std::fs::create_dir_all(&ordner).unwrap();
        for rate in [4_294_967_291u32, 1_000_003, 192_001, 7_999, 1] {
            let pfad = ordner.join(format!("r{rate}.wav"));
            wav_roh(&pfad, rate, 1, &[0.1; 4096]);
            let f = laden(&pfad).unwrap_err();
            assert!(f.contains("Abtastrate"), "{rate}: {f}");
        }
        assert!(umrechnen(&[vec![0.0; 10]], 4_294_967_291).is_err());
    }

    #[test]
    fn grenzraten_gehen() {
        let ordner = std::env::temp_dir().join("maskcue_test_sound_grenze");
        std::fs::create_dir_all(&ordner).unwrap();
        for rate in [8_000u32, 22_050, 96_000, 191_999, 192_000] {
            let pfad = ordner.join(format!("r{rate}.wav"));
            let x = sinus(rate as f32, 440.0, 0.25);
            wav_roh(&pfad, rate, 1, &x);
            let s = laden(&pfad).unwrap_or_else(|f| panic!("{rate}: {f}"));
            let soll = (x.len() as f64 * 48_000.0 / rate as f64).round() as usize;
            assert_eq!(s.samples.len(), soll, "{rate}");
        }
    }

    #[test]
    fn mehr_kanaele_nur_links_rechts() {
        let ordner = std::env::temp_dir().join("maskcue_test_sound_kanaele");
        std::fs::create_dir_all(&ordner).unwrap();
        let pfad = ordner.join("k6.wav");
        // 6 Kanäle, Kanal k hat den Wert k/10.
        let x: Vec<f32> = (0..4800 * 6).map(|i| (i % 6) as f32 / 10.0).collect();
        wav_roh(&pfad, 48_000, 6, &x);
        let s = laden(&pfad).unwrap();
        assert_eq!(s.kanaele, 2);
        assert_eq!(s.frames(), 4800);
        assert!(s.samples.chunks(2).all(|f| f[0] == 0.0 && (f[1] - 0.1).abs() < 1e-6));
    }
}
