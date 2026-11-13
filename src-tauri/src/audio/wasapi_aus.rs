//! Eigene WASAPI-Ausgabe mit kleiner Vorlage.
//!
//! cpal füllt bei jedem Aufruf den ganzen Windows-Puffer (ca. 22 ms). Diese
//! Ausgabe legt nur so viel vor wie nötig (Standard: 1 Block + 2 ms). Das
//! spart ca. 10 ms Verzögerung.
//!
//! Ablauf: eigener Faden mit MMCSS-Priorität „Pro Audio“. Windows weckt ihn,
//! sobald es einen Block abgeholt hat. Der Faden füllt bis zur Vorlage auf.

use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::Threading::{AvSetMmThreadCharacteristicsW, CreateEventW, WaitForSingleObject};
use windows::core::{GUID, w};

use super::geraete;
use super::pipe::{INTERN_RATE, Stats};

const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const SUBTYPE_IEEE_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

#[derive(Clone, Debug)]
pub struct AusInfo {
    pub name: String,
    pub kanaele: u16,
    /// Größe des Windows-Puffers in Frames.
    pub puffer_frames: u32,
    /// Block, den Windows pro Durchgang abholt (Frames).
    pub periode_frames: u32,
    /// So viele Frames liegen nach dem Auffüllen bereit.
    pub vorlage_frames: u32,
    /// Feste Verzögerung im Windows-Audio (ms).
    pub strom_latenz_ms: f64,
}

/// Läuft, solange der Wert lebt.
pub struct WasapiAusgang {
    stop: Arc<AtomicBool>,
    faden: Option<JoinHandle<()>>,
    pub info: AusInfo,
}

impl Drop for WasapiAusgang {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(f) = self.faden.take() {
            let _ = f.join();
        }
    }
}

/// Namen aller aktiven Ausgänge (kaputte Bluetooth-Namen repariert, siehe
/// `geraet_name.rs`).
pub fn ausgaenge() -> Result<Vec<String>, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let (_, namen) = ausgaenge_com().map_err(|e| e.to_string())?;
        Ok(namen)
    }
}

unsafe fn ausgaenge_com() -> windows::core::Result<(Vec<IMMDevice>, Vec<String>)> {
    unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let sammlung = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        let mut geraete = Vec::new();
        let mut namen = Vec::new();
        for i in 0..sammlung.GetCount()? {
            let g = sammlung.Item(i)?;
            let roh = g.OpenPropertyStore(STGM_READ)?.GetValue(&PKEY_Device_FriendlyName)?.to_string();
            let id = g.GetId()?;
            let id_text = id.to_string().unwrap_or_default();
            CoTaskMemFree(Some(id.0 as *const _));
            namen.push(super::geraet_name::reparieren(&roh, &id_text));
            geraete.push(g);
        }
        Ok((geraete, namen))
    }
}

/// Startet die Ausgabe auf dem Gerät `name`.
///
/// `render(puffer, kanaele, wiedergabe_ns)` füllt den Puffer. `wiedergabe_ns`
/// ist der Zeitpunkt (QPC-Zeit, wie bei cpal), an dem der erste Frame
/// abgespielt wird. `vorlage_frames`: None = 1 Block + 2 ms.
pub fn start<F>(name: &str, vorlage_frames: Option<u32>, stats: Arc<Stats>, render: F) -> Result<WasapiAusgang, String>
where
    F: FnMut(&mut [f32], usize, u64) + Send + 'static,
{
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let name = name.to_string();
    let stop_faden = stop.clone();
    let faden = std::thread::Builder::new()
        .name("maskcue-ausgang".into())
        .spawn(move || {
            if let Err(e) = unsafe { lauf(&name, vorlage_frames, &stop_faden, stats, render, &tx) } {
                let _ = tx.send(Err(e));
            }
        })
        .map_err(|e| e.to_string())?;
    match rx.recv() {
        Ok(Ok(info)) => Ok(WasapiAusgang { stop, faden: Some(faden), info }),
        Ok(Err(e)) => {
            let _ = faden.join();
            Err(format!("Ausgang: {e}"))
        }
        Err(_) => Err("Ausgang: Faden ohne Antwort beendet".into()),
    }
}

/// Liest die Kanal-Maske aus dem Mix-Format (falls vorhanden).
unsafe fn kanal_maske(mix: *const WAVEFORMATEX) -> u32 {
    unsafe {
        let f = std::ptr::read_unaligned(mix);
        if f.wFormatTag == WAVE_FORMAT_EXTENSIBLE {
            let ext = std::ptr::read_unaligned(mix as *const WAVEFORMATEXTENSIBLE);
            ext.dwChannelMask
        } else if f.nChannels == 2 {
            0x3 // vorne links + rechts
        } else {
            0
        }
    }
}

unsafe fn lauf<F>(
    name: &str,
    vorlage: Option<u32>,
    stop: &AtomicBool,
    stats: Arc<Stats>,
    mut render: F,
    tx: &mpsc::Sender<Result<AusInfo, String>>,
) -> Result<(), String>
where
    F: FnMut(&mut [f32], usize, u64),
{
    unsafe {
        let fehler = |e: windows::core::Error| e.to_string();
        CoInitializeEx(None, COINIT_MULTITHREADED).ok().map_err(fehler)?;

        let (mut geraete, namen) = ausgaenge_com().map_err(fehler)?;
        let i = geraete::waehle(&namen, name, "Ausgang")?;
        let geraet = geraete.swap_remove(i);
        let client: IAudioClient = geraet.Activate(CLSCTX_ALL, None).map_err(fehler)?;

        // Format: f32, 48 kHz, Kanäle wie das Gerät.
        let mix = client.GetMixFormat().map_err(fehler)?;
        let kanaele = std::ptr::read_unaligned(mix).nChannels;
        let maske = kanal_maske(mix);
        CoTaskMemFree(Some(mix as _));
        let mut format = WAVEFORMATEXTENSIBLE::default();
        format.Format.wFormatTag = WAVE_FORMAT_EXTENSIBLE;
        format.Format.nChannels = kanaele;
        format.Format.nSamplesPerSec = INTERN_RATE;
        format.Format.wBitsPerSample = 32;
        format.Format.nBlockAlign = kanaele * 4;
        format.Format.nAvgBytesPerSec = INTERN_RATE * kanaele as u32 * 4;
        format.Format.cbSize = 22;
        format.Samples.wValidBitsPerSample = 32;
        format.dwChannelMask = maske;
        format.SubFormat = SUBTYPE_IEEE_FLOAT;

        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                0,
                0,
                &format as *const _ as *const WAVEFORMATEX,
                None,
            )
            .map_err(fehler)?;

        let ereignis: HANDLE = CreateEventW(None, false, false, None).map_err(fehler)?;
        let ergebnis = (|| -> Result<(), String> {
            client.SetEventHandle(ereignis).map_err(fehler)?;
            let puffer_frames = client.GetBufferSize().map_err(fehler)?;
            let mut periode_hns = 0i64;
            client.GetDevicePeriod(Some(&mut periode_hns), None).map_err(fehler)?;
            let periode_frames = (periode_hns as u64 * INTERN_RATE as u64 / 10_000_000) as u32;
            let strom_latenz_hns = client.GetStreamLatency().map_err(fehler)?;
            let vorlage = vorlage
                .unwrap_or(periode_frames + INTERN_RATE / 500)
                .clamp(periode_frames.max(1), puffer_frames);
            let render_client: IAudioRenderClient = client.GetService().map_err(fehler)?;

            let mut qpc_freq = 0i64;
            QueryPerformanceFrequency(&mut qpc_freq).map_err(fehler)?;
            let jetzt_ns = || {
                let mut z = 0i64;
                let _ = QueryPerformanceCounter(&mut z);
                (z as u128 * 1_000_000_000 / qpc_freq as u128) as u64
            };
            let strom_latenz_ns = strom_latenz_hns as u64 * 100;
            let ns_pro_frame = 1e9 / INTERN_RATE as f64;
            let k = kanaele as usize;

            // Erste Vorlage: Stille.
            let p = render_client.GetBuffer(vorlage).map_err(fehler)?;
            std::slice::from_raw_parts_mut(p as *mut f32, vorlage as usize * k).fill(0.0);
            render_client.ReleaseBuffer(vorlage, 0).map_err(fehler)?;

            let mut task = 0u32;
            let _ = AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut task);
            client.Start().map_err(fehler)?;

            let _ = tx.send(Ok(AusInfo {
                name: namen[i].clone(),
                kanaele,
                puffer_frames,
                periode_frames,
                vorlage_frames: vorlage,
                strom_latenz_ms: strom_latenz_hns as f64 / 10_000.0,
            }));

            while !stop.load(Relaxed) {
                if WaitForSingleObject(ereignis, 200) != WAIT_OBJECT_0 {
                    continue;
                }
                let belegt = match client.GetCurrentPadding() {
                    Ok(b) => b,
                    Err(e) => {
                        stats.fehler_merken(format!("Ausgang: {e}"));
                        break;
                    }
                };
                if belegt == 0 && vorlage > periode_frames {
                    // Windows hat mehr als einen Block abgeholt, bevor wir
                    // nachgelegt haben: wir kamen zu spät. (Bei Vorlage = 1 Block
                    // ist 0 normal und kein Fehler.)
                    stats.ausgang_leer.fetch_add(1, Relaxed);
                }
                if belegt >= vorlage {
                    continue;
                }
                let neu = vorlage - belegt;
                let p = match render_client.GetBuffer(neu) {
                    Ok(p) => p,
                    Err(e) => {
                        stats.fehler_merken(format!("Ausgang: {e}"));
                        continue;
                    }
                };
                let daten = std::slice::from_raw_parts_mut(p as *mut f32, neu as usize * k);
                let jetzt = jetzt_ns();
                let wiedergabe = jetzt + (belegt as f64 * ns_pro_frame) as u64 + strom_latenz_ns;
                stats.aus_verzoegerung_us.store((wiedergabe - jetzt) / 1000, Relaxed);
                render(daten, k, wiedergabe);
                if let Err(e) = render_client.ReleaseBuffer(neu, 0) {
                    stats.fehler_merken(format!("Ausgang: {e}"));
                }
            }
            let _ = client.Stop();
            Ok(())
        })();
        let _ = CloseHandle(ereignis);
        // COM-Objekte freigeben, bevor COM für diesen Faden endet.
        drop(client);
        drop(geraet);
        drop(geraete);
        CoUninitialize();
        ergebnis
    }
}
