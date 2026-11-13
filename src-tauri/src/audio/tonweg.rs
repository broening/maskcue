//! Tonweg: Mikrofon → Verarbeitung (Denoise, …) → ein oder mehrere Ausgänge.

use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, Stream, StreamConfig};

use super::geraete;
use super::pipe::{INTERN_RATE, InputSide, PipeConfig, Stats, pipe, roh_ring};
use super::aufnahme::{AufnahmeQuelle, aufnahme};
use super::soundboard::{self, Soundboard};
use super::verarbeitung::{self, KettenBau, Verarbeitung, Ziel};
use super::wasapi_aus::{self, AusInfo, WasapiAusgang};

/// Läuft, solange der Wert lebt. Beim Löschen stoppen alle Ströme.
pub struct Tonweg {
    // Reihenfolge beim Löschen: Eingang, Verarbeitung, Ausgänge.
    _eingang: Stream,
    _verarbeitung: Verarbeitung,
    pub ausgaenge: Vec<Ausgang>,
    /// Sounds und Musik für alle Ausgänge.
    pub soundboard: Soundboard,
    /// Aufnahme-Abgriff am Ausgang „Spiel“ (dem ersten Ausgang).
    pub aufnahme: AufnahmeQuelle,
    /// Messwerte des Mikrofons (Fehler, Überlauf des Roh-Puffers, Verzögerung).
    pub eingang_stats: Arc<Stats>,
    pub info: TonwegInfo,
}

pub struct Ausgang {
    _strom: WasapiAusgang,
    pub info: AusInfo,
    pub stats: Arc<Stats>,
    /// Verzögerung aller Effekte auf diesem Weg (ms).
    pub effekt_verzoegerung_ms: f64,
}

#[derive(Clone, Debug)]
pub struct TonwegInfo {
    pub eingang: String,
    pub ein_rate: u32,
    pub ein_kanaele: u16,
    pub ein_format: SampleFormat,
    pub ein_block: Option<u32>,
}

/// Startet den Tonweg mit einem Mikrofon und mehreren Ausgängen.
/// `mit_effekten`: true, wenn `bau` Effekte liefert (dann muss das Mikrofon 48 kHz haben).
pub fn start(eingang: &str, ausgaenge: &[&str], config: PipeConfig, bau: KettenBau, mit_effekten: bool) -> Result<Tonweg, String> {
    if ausgaenge.is_empty() {
        return Err("Kein Ausgang angegeben".into());
    }
    let ein_geraet = geraete::eingang(eingang)?;
    let ein_cfg = ein_geraet.default_input_config().map_err(|e| format!("Eingang: {e}"))?;
    let ein_format = ein_cfg.sample_format();
    let ein_stream_cfg: StreamConfig = ein_cfg.into();
    let ein_rate = ein_stream_cfg.sample_rate;
    // Mikrofone mit anderer Rate (z. B. 44,1 kHz) rechnet der RohLeser auf 48 kHz um.
    let _ = mit_effekten;

    let eingang_stats = Arc::new(Stats::new());
    let (mut roh, roh_leser) = roh_ring(ein_rate, eingang_stats.clone());

    // Ausgänge zuerst: sie legen Stille vor, bis ihr Puffer voll genug ist.
    let mut stroeme = Vec::new();
    let mut ziele = Vec::new();
    let mut steuerungen = Vec::new();
    let (abgriff, aufnahme_quelle) = aufnahme();
    let mut abgriff = Some(abgriff);
    for name in ausgaenge {
        // Die Verarbeitung liefert immer 48 kHz (RohLeser rechnet um).
        let (puffer, mut output, stats) = pipe(INTERN_RATE, INTERN_RATE, config);
        let (mut mischer, steuerung) = soundboard::mischer();
        steuerungen.push(steuerung);
        let stumm = config.stumm;
        // Nur der erste Ausgang („Spiel“) bekommt den Aufnahme-Abgriff.
        let mut abgriff = abgriff.take();
        let strom = wasapi_aus::start(name, config.aus_vorlage, stats.clone(), move |data, kanaele, wiedergabe| {
            output.render(data, kanaele, wiedergabe);
            // Sounds erst hier dazu: sie warten nicht auf den Puffer der Stimme.
            mischer.mische(data, kanaele);
            if stumm {
                data.fill(0.0);
            }
            if let Some(a) = abgriff.as_mut() {
                a.abgreifen(data, kanaele);
            }
        })?;
        stroeme.push((strom, stats.clone()));
        ziele.push(Ziel { puffer, stats });
    }
    let verarbeitung = verarbeitung::start(roh_leser, ziele, bau, eingang_stats.clone())?;
    roh.wecke(verarbeitung.faden());

    let ausgaenge = stroeme
        .into_iter()
        .zip(&verarbeitung.verzoegerung)
        .map(|((strom, stats), &v)| Ausgang {
            info: strom.info.clone(),
            _strom: strom,
            stats,
            effekt_verzoegerung_ms: v as f64 * 1000.0 / INTERN_RATE as f64,
        })
        .collect();

    let ein_stream = match ein_format {
        SampleFormat::F32 => baue_eingang::<f32>(&ein_geraet, ein_stream_cfg, roh, eingang_stats.clone()),
        SampleFormat::I16 => baue_eingang::<i16>(&ein_geraet, ein_stream_cfg, roh, eingang_stats.clone()),
        SampleFormat::I32 => baue_eingang::<i32>(&ein_geraet, ein_stream_cfg, roh, eingang_stats.clone()),
        f => return Err(format!("Eingang: Format {f} wird nicht unterstützt")),
    }?;
    ein_stream.play().map_err(|e| format!("Eingang: {e}"))?;

    let info = TonwegInfo {
        eingang: ein_geraet.to_string(),
        ein_rate,
        ein_kanaele: ein_stream_cfg.channels,
        ein_format,
        ein_block: ein_stream.buffer_size().ok(),
    };
    Ok(Tonweg { _eingang: ein_stream, _verarbeitung: verarbeitung, ausgaenge, soundboard: Soundboard::new(steuerungen), aufnahme: aufnahme_quelle, eingang_stats, info })
}

fn baue_eingang<T>(geraet: &cpal::Device, cfg: StreamConfig, mut roh: InputSide, stats: Arc<Stats>) -> Result<Stream, String>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    let kanaele = cfg.channels as usize;
    let ein_stats = stats.clone();
    geraet
        .build_input_stream::<T, _, _>(
            cfg,
            move |data, info| {
                let ts = info.timestamp();
                let vz = ts.callback.saturating_duration_since(ts.capture).as_micros() as u64;
                ein_stats.ein_verzoegerung_us.store(vz, Relaxed);
                roh.push(data, kanaele, ts.capture.as_nanos() as u64, <f32 as FromSample<T>>::from_sample_);
            },
            move |e| {
                // „Xrun“ heißt bei WASAPI nur: Der Treiber hat das Paket als
                // Lücke markiert. Manche Mikrofone tun das bei jedem Paket,
                // ohne dass etwas fehlt (Dans „Tischmikrofon“, 20.10.2026:
                // 2000 von 2000 Paketen markiert, 20,000 s Ton in 20,002 s;
                // examples/mikro_xrun.rs). Echte Lücken misst der Roh-Puffer
                // selbst (Lücke, Überlauf). Darum keine Meldung.
                if e.kind() != cpal::ErrorKind::Xrun {
                    stats.fehler_merken(format!("Eingang: {e}"));
                }
            },
            None,
        )
        .map_err(|e| format!("Eingang: {e}"))
}
