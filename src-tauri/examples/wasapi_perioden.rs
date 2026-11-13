//! Zeigt, welche Blockgrößen (Perioden) Windows für jedes Gerät erlaubt.
//! Kleinere Perioden = weniger Verzögerung (IAudioClient3, Windows 10+).
//!
//! cargo run --release --example wasapi_perioden

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::core::Interface;

fn main() -> windows::core::Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let geraete = enumerator.EnumAudioEndpoints(eAll, DEVICE_STATE_ACTIVE)?;
        println!("{:<62} {:>8} {:>9} {:>8} {:>8}", "Gerät", "Richtung", "Standard", "Minimum", "Schritt");
        for i in 0..geraete.GetCount()? {
            let geraet = geraete.Item(i)?;
            let name = geraet.OpenPropertyStore(STGM_READ)?.GetValue(&PKEY_Device_FriendlyName)?.to_string();
            let richtung = match geraet.cast::<IMMEndpoint>()?.GetDataFlow()? {
                f if f == eRender => "Ausgang",
                _ => "Eingang",
            };
            let client: IAudioClient3 = match geraet.Activate(CLSCTX_ALL, None) {
                Ok(c) => c,
                Err(e) => {
                    println!("{name:<62} {richtung:>8} kein IAudioClient3: {e}");
                    continue;
                }
            };
            let format = client.GetMixFormat()?;
            let rate = (*format).nSamplesPerSec as f64 / 1000.0;
            let (mut std, mut grund, mut min, mut max) = (0u32, 0u32, 0u32, 0u32);
            client.GetSharedModeEnginePeriod(format, &mut std, &mut grund, &mut min, &mut max)?;
            println!(
                "{name:<62} {richtung:>8} {:>7.2}ms {:>6.2}ms {:>6.2}ms",
                std as f64 / rate,
                min as f64 / rate,
                grund as f64 / rate
            );
            CoTaskMemFree(Some(format as _));
        }
    }
    Ok(())
}
