//! Kaputte Gerätenamen von Windows reparieren.
//!
//! Bluetooth-Kopfhörer heißen manchmal „Kopfhörer (%1%0\r\n)“ statt
//! „Kopfhörer (EDIFIER WH950NB)“: Windows füllt den Platzhalter im Namen
//! nicht aus (Dans Kopfhörer am 20. und 21.10.2026, mal so, mal so). Dann
//! fand MaskCue das gespeicherte Gerät nicht mehr. Der echte Name steht am
//! Bluetooth-Gerät im selben Geräte-Container. Nachstellen:
//! `cargo run --release --example ausgaenge_liste`.

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GETIDLIST_FILTER_CLASS, CM_GETIDLIST_FILTER_PRESENT, CM_Get_DevNode_PropertyW, CM_Get_Device_ID_List_SizeW, CM_Get_Device_ID_ListW,
    CM_LOCATE_DEVNODE_NORMAL, CM_Locate_DevNodeW, CR_SUCCESS,
};
use windows::Win32::Devices::Properties::{DEVPKEY_Device_ContainerId, DEVPKEY_Device_FriendlyName, DEVPROPTYPE};
use windows::Win32::Foundation::DEVPROPKEY;
use windows::core::{GUID, HSTRING};

/// Geräteklasse „Bluetooth“.
const KLASSE_BLUETOOTH: &str = "{e0cbf06c-cd8b-4647-bb8a-263b43f0f974}";

/// Name eines Ausgangs. `endpunkt_id` ist die ID von `IMMDevice::GetId`.
pub fn reparieren(roh: &str, endpunkt_id: &str) -> String {
    if !kaputt(roh) {
        return roh.to_string();
    }
    let geraet = unsafe { container_name(endpunkt_id) };
    zusammensetzen(roh, geraet.as_deref())
}

fn kaputt(name: &str) -> bool {
    name.contains("%1") || name.contains("%0")
}

/// „Kopfhörer (%1%0…)“ + „EDIFIER WH950NB“ → „Kopfhörer (EDIFIER WH950NB)“.
/// Ohne Gerätenamen bleibt nur der Teil vor der Klammer.
fn zusammensetzen(roh: &str, geraet: Option<&str>) -> String {
    let vorne = roh.split('(').next().unwrap_or("").trim();
    let vorne = if kaputt(vorne) { "" } else { vorne };
    match (vorne, geraet) {
        ("", Some(g)) => g.to_string(),
        (v, Some(g)) => format!("{v} ({g})"),
        ("", None) => "Unbekanntes Gerät".to_string(),
        (v, None) => v.to_string(),
    }
}

unsafe fn eigenschaft(knoten: u32, schluessel: &DEVPROPKEY) -> Option<Vec<u8>> {
    unsafe {
        let mut typ = DEVPROPTYPE::default();
        let mut groesse = 0u32;
        let _ = CM_Get_DevNode_PropertyW(knoten, schluessel, &mut typ, None, &mut groesse, 0);
        if groesse == 0 {
            return None;
        }
        let mut puffer = vec![0u8; groesse as usize];
        (CM_Get_DevNode_PropertyW(knoten, schluessel, &mut typ, Some(puffer.as_mut_ptr()), &mut groesse, 0) == CR_SUCCESS).then_some(puffer)
    }
}

unsafe fn knoten(instanz: &str) -> Option<u32> {
    unsafe {
        let mut k = 0u32;
        (CM_Locate_DevNodeW(&mut k, &HSTRING::from(instanz), CM_LOCATE_DEVNODE_NORMAL) == CR_SUCCESS).then_some(k)
    }
}

unsafe fn container(k: u32) -> Option<GUID> {
    unsafe {
        let b = eigenschaft(k, &DEVPKEY_Device_ContainerId)?;
        (b.len() == 16).then(|| GUID::from_u128(u128::from_be_bytes(guid_bytes(&b))))
    }
}

/// GUID im Speicher (Data1–3 little endian) → Reihenfolge für `from_u128`.
fn guid_bytes(b: &[u8]) -> [u8; 16] {
    let mut o = [0u8; 16];
    o[0..4].copy_from_slice(&[b[3], b[2], b[1], b[0]]);
    o[4..6].copy_from_slice(&[b[5], b[4]]);
    o[6..8].copy_from_slice(&[b[7], b[6]]);
    o[8..16].copy_from_slice(&b[8..16]);
    o
}

fn text(b: &[u8]) -> String {
    let w: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
    String::from_utf16_lossy(&w).trim().to_string()
}

/// Name des Bluetooth-Geräts im selben Container wie der Audio-Endpunkt.
unsafe fn container_name(endpunkt_id: &str) -> Option<String> {
    unsafe {
        let ziel = container(knoten(&format!("SWD\\MMDEVAPI\\{endpunkt_id}"))?)?;
        let filter = HSTRING::from(KLASSE_BLUETOOTH);
        let flags = CM_GETIDLIST_FILTER_CLASS | CM_GETIDLIST_FILTER_PRESENT;
        let mut laenge = 0u32;
        if CM_Get_Device_ID_List_SizeW(&mut laenge, &filter, flags) != CR_SUCCESS || laenge == 0 {
            return None;
        }
        let mut liste = vec![0u16; laenge as usize];
        if CM_Get_Device_ID_ListW(&filter, &mut liste, flags) != CR_SUCCESS {
            return None;
        }
        String::from_utf16_lossy(&liste).split('\0').filter(|s| !s.is_empty()).find_map(|id| {
            let k = knoten(id)?;
            if container(k)? != ziel {
                return None;
            }
            let n = text(&eigenschaft(k, &DEVPKEY_Device_FriendlyName)?);
            (!n.is_empty() && !kaputt(&n)).then_some(n)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heile_namen_bleiben() {
        assert_eq!(reparieren("MaskCue (Elgato Virtual Audio)", "egal"), "MaskCue (Elgato Virtual Audio)");
    }

    #[test]
    fn platzhalter_wird_zum_geraetenamen() {
        assert_eq!(zusammensetzen("Kopfhörer (%1%0\r\n)", Some("EDIFIER WH950NB")), "Kopfhörer (EDIFIER WH950NB)");
        assert_eq!(zusammensetzen("Kopfhörer (%1%0\r\n)", None), "Kopfhörer");
        assert_eq!(zusammensetzen("%1", Some("EDIFIER WH950NB")), "EDIFIER WH950NB");
        assert_eq!(zusammensetzen("%1", None), "Unbekanntes Gerät");
    }

    #[test]
    fn guid_aus_speicher() {
        let g = GUID::from_u128(0x04083F90_5A5E_5174_AA47_208E13193DFB);
        let speicher: [u8; 16] = unsafe { std::mem::transmute(g) };
        assert_eq!(GUID::from_u128(u128::from_be_bytes(guid_bytes(&speicher))), g);
    }
}
