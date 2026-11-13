//! Windows-Teil: Low-Level-Hook für Tastatur und Maus, Programm im Vordergrund.
//!
//! Der Hook verschluckt keine Taste: Jede Taste geht weiter ans Spiel. Er
//! schickt nur gedrückte Kombinationen in eine Warteschlange. Windows wirft
//! einen Hook hinaus, der zu langsam antwortet. Darum macht der Hook selbst
//! fast nichts.

use std::cell::RefCell;
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThreadId, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG,
    MSLLHOOKSTRUCT, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WH_MOUSE_LL, WM_KEYDOWN, WM_MBUTTONDOWN, WM_QUIT, WM_SYSKEYDOWN, WM_XBUTTONDOWN,
};
use windows::core::PWSTR;

use super::taste::{Kombination, MausTaste, Mods, Taste, ist_zusatz};

thread_local! {
    static ZIEL: RefCell<Option<Box<dyn Fn(Kombination)>>> = const { RefCell::new(None) };
}

fn gedrueckt(vk: i32) -> bool {
    unsafe { GetAsyncKeyState(vk) < 0 }
}

/// Zusatz-Tasten, die gerade gedrückt sind. Im Hook ist der Zustand der
/// aktuellen Taste noch nicht aktualisiert, der früher gedrückter Tasten schon.
fn mods_jetzt() -> Mods {
    let mut m = Mods::default();
    if gedrueckt(VK_CONTROL.0 as i32) {
        m = m | Mods::STRG;
    }
    if gedrueckt(VK_MENU.0 as i32) {
        m = m | Mods::ALT;
    }
    if gedrueckt(VK_SHIFT.0 as i32) {
        m = m | Mods::UMSCHALT;
    }
    if gedrueckt(VK_LWIN.0 as i32) || gedrueckt(VK_RWIN.0 as i32) {
        m = m | Mods::WIN;
    }
    m
}

fn melden(k: Kombination) {
    ZIEL.with(|z| {
        if let Some(f) = z.borrow().as_ref() {
            f(k);
        }
    });
}

unsafe extern "system" fn tastatur(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && (wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN) {
        let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let vk = kb.vkCode;
        // Gedrückt halten schickt die Taste immer wieder. Der Zustand zeigt
        // hier noch den Stand vor diesem Ereignis: schon gedrückt = Wiederholung.
        if vk > 0 && vk < 255 && !ist_zusatz(vk as u8) && !gedrueckt(vk as i32) {
            melden(Kombination::neu(mods_jetzt(), Taste::Tastatur(vk as u8)));
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn maus(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let taste = match wparam.0 as u32 {
            WM_MBUTTONDOWN => Some(MausTaste::Mitte),
            WM_XBUTTONDOWN => {
                let ms = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
                match (ms.mouseData >> 16) & 0xFFFF {
                    1 => Some(MausTaste::Seite1),
                    2 => Some(MausTaste::Seite2),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(t) = taste {
            melden(Kombination::neu(mods_jetzt(), Taste::Maus(t)));
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Läuft, solange der Wert lebt. Eigener Faden mit Nachrichten-Schleife.
pub struct Hook {
    faden_id: u32,
    faden: Option<JoinHandle<()>>,
    pub maus: bool,
}

impl Hook {
    /// `ziel` wird im Hook-Faden aufgerufen und muss sofort zurückkehren.
    /// `maus`: auch Maustasten (Mitte, Seite). Nur einschalten, wenn nötig.
    pub fn start(maus: bool, ziel: impl Fn(Kombination) + Send + 'static) -> Result<Hook, String> {
        let (tx, rx) = mpsc::channel::<Result<u32, String>>();
        let faden = std::thread::Builder::new()
            .name("maskcue-hotkeys".into())
            .spawn(move || hook_faden(maus, Box::new(ziel), tx))
            .map_err(|e| e.to_string())?;
        match rx.recv() {
            Ok(Ok(faden_id)) => Ok(Hook { faden_id, faden: Some(faden), maus }),
            Ok(Err(e)) => {
                let _ = faden.join();
                Err(e)
            }
            Err(_) => Err("Hotkeys: Faden ohne Antwort beendet".into()),
        }
    }
}

impl Drop for Hook {
    fn drop(&mut self) {
        unsafe {
            let _ = PostThreadMessageW(self.faden_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(f) = self.faden.take() {
            let _ = f.join();
        }
    }
}

fn hook_faden(maus_an: bool, ziel: Box<dyn Fn(Kombination)>, tx: Sender<Result<u32, String>>) {
    ZIEL.with(|z| *z.borrow_mut() = Some(ziel));
    unsafe {
        let mut msg = MSG::default();
        // Nachrichten-Warteschlange anlegen, damit WM_QUIT ankommt.
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
        let modul = GetModuleHandleW(None).ok().map(|h| HINSTANCE(h.0));
        let hooks: Result<Vec<HHOOK>, String> = (|| {
            let mut v = vec![SetWindowsHookExW(WH_KEYBOARD_LL, Some(tastatur), modul, 0).map_err(|e| format!("Tastatur-Hook: {e}"))?];
            if maus_an {
                v.push(SetWindowsHookExW(WH_MOUSE_LL, Some(maus), modul, 0).map_err(|e| format!("Maus-Hook: {e}"))?);
            }
            Ok(v)
        })();
        let hooks = match hooks {
            Ok(h) => h,
            Err(e) => {
                let _ = tx.send(Err(e));
                return;
            }
        };
        let _ = tx.send(Ok(GetCurrentThreadId()));
        // Die Hooks laufen nur, solange dieser Faden Nachrichten abholt.
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        for h in hooks {
            let _ = UnhookWindowsHookEx(h);
        }
    }
    ZIEL.with(|z| *z.borrow_mut() = None);
}

/// Prozess-Nummer des Fensters im Vordergrund.
pub fn vordergrund_pid() -> Option<u32> {
    unsafe {
        let h = GetForegroundWindow();
        if h.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        (pid != 0).then_some(pid)
    }
}

struct Griff(HANDLE);
impl Drop for Griff {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Dateiname des Programms, z. B. "FiveM.exe".
pub fn programm_name(pid: u32) -> Option<String> {
    unsafe {
        let h = Griff(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?);
        let mut puffer = [0u16; 1024];
        let mut laenge = puffer.len() as u32;
        QueryFullProcessImageNameW(h.0, PROCESS_NAME_WIN32, PWSTR(puffer.as_mut_ptr()), &mut laenge).ok()?;
        let pfad = String::from_utf16_lossy(&puffer[..laenge as usize]);
        Some(pfad.rsplit(['\\', '/']).next().unwrap_or(&pfad).to_string())
    }
}

fn erhoeht_griff(h: HANDLE) -> Option<bool> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(h, TOKEN_QUERY, &mut token).ok()?;
        let token = Griff(token);
        let mut e = TOKEN_ELEVATION::default();
        let mut laenge = 0u32;
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some(&mut e as *mut _ as *mut _),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut laenge,
        )
        .ok()?;
        Some(e.TokenIsElevated != 0)
    }
}

/// Läuft das Programm als Administrator? `None`: Windows sagt es nicht
/// (meist, weil das Programm mehr Rechte hat als MaskCue).
pub fn ist_admin(pid: u32) -> Option<bool> {
    unsafe {
        let h = Griff(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?);
        erhoeht_griff(h.0)
    }
}

/// Läuft MaskCue selbst als Administrator?
pub fn selbst_admin() -> bool {
    erhoeht_griff(unsafe { GetCurrentProcess() }).unwrap_or(false)
}

/// Tests mit echten Tasten dürfen nicht gleichzeitig laufen: Jeder Hook
/// sieht auch die Tasten des anderen Tests.
#[cfg(test)]
pub(crate) static TASTEN_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Schickt eine echte Taste an Windows (nur für Tests).
#[cfg(test)]
pub(crate) fn taste_senden(vk: u16, hoch: bool) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY,
    };
    let ki = KEYBDINPUT { wVk: VIRTUAL_KEY(vk), dwFlags: if hoch { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() };
    let input = INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki } };
    unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    /// Echter Hook, echte Taste: F24 (gibt es auf keiner Tastatur, stört nichts).
    /// Gedrückt halten (3× runter) meldet nur einmal.
    #[test]
    #[ignore = "drückt echte Tasten (F24); von Hand starten: cargo test -- --ignored hook"]
    fn hook_sieht_f24_einmal() {
        let _sperre = TASTEN_TEST.lock().unwrap_or_else(|e| e.into_inner());
        let (tx, rx) = mpsc::channel();
        let _hook = Hook::start(false, move |k| {
            let _ = tx.send(k);
        })
        .unwrap();
        taste_senden(0x87, false);
        taste_senden(0x87, false);
        taste_senden(0x87, false);
        taste_senden(0x87, true);
        let k = rx.recv_timeout(Duration::from_secs(2)).expect("Hook meldet nichts");
        assert_eq!(k.to_string(), "F24");
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "Wiederholung gemeldet");
    }

    #[test]
    fn eigenes_programm_hat_namen() {
        let name = programm_name(std::process::id()).expect("Name");
        assert!(name.to_lowercase().ends_with(".exe"), "{name}");
        assert_eq!(ist_admin(std::process::id()), Some(selbst_admin()));
    }
}
