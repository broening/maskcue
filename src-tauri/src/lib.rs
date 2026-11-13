pub mod app;
pub mod audio;
pub mod hotkeys;

use std::sync::Mutex;

use serde::Serialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, RunEvent, State, WebviewUrl, WebviewWindowBuilder, WindowEvent, Wry};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use app::einstellungen::{self, Einstellungen};
use app::motor::{Meldung, MotorGriff};
use hotkeys::profil::PROFIL_AUS;

struct AppZustand {
    motor: MotorGriff,
    e: Mutex<Einstellungen>,
    /// Zuletzt im Taskleisten-Symbol gezeigtes Profil.
    tray_profil: Mutex<String>,
    /// Gefundenes, noch nicht installiertes Update (Modus „Nur benachrichtigen“).
    update: Mutex<Option<tauri_plugin_updater::Update>>,
}

/// Neue Version für die Oberfläche.
#[derive(Clone, Serialize)]
struct UpdateInfo {
    version: String,
    notiz: String,
}

/// Adresse der Update-Beschreibung (latest.json im neuesten GitHub-Release).
/// MASKCUE_UPDATE_URL: andere Adresse zum Testen.
fn update_url() -> String {
    std::env::var("MASKCUE_UPDATE_URL").unwrap_or_else(|_| "https://github.com/broening/maskcue/releases/latest/download/latest.json".into())
}

/// Bei GitHub nach einer neueren Version fragen. Merkt sie sich für später.
async fn update_suchen(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let url = update_url().parse().map_err(|f| format!("{f}"))?;
    let u = app.updater_builder().endpoints(vec![url]).map_err(|f| f.to_string())?.build().map_err(|f| f.to_string())?;
    let neu = u.check().await.map_err(|f| f.to_string())?;
    let info = neu.as_ref().map(|n| UpdateInfo { version: n.version.clone(), notiz: n.body.clone().unwrap_or_default() });
    if let Some(z) = app.try_state::<AppZustand>()
        && let Ok(mut gemerkt) = z.update.lock()
    {
        *gemerkt = neu;
    }
    Ok(info)
}

/// Gemerktes Update laden, prüfen (Signatur) und installieren. Unter
/// Windows beendet sich MaskCue dafür; der Installer startet es neu.
async fn update_einspielen(app: &AppHandle) -> Result<(), String> {
    let update = app.state::<AppZustand>().update.lock().map_err(|f| f.to_string())?.take();
    let Some(update) = update else { return Err("Kein Update gefunden".into()) };
    let _ = app.emit("update-laedt", &update.version);
    let mut geladen = 0usize;
    let h = app.clone();
    update
        .download_and_install(
            move |stueck, gesamt| {
                geladen += stueck;
                if let Some(g) = gesamt {
                    let _ = h.emit("update-fortschritt", geladen as f64 / g as f64);
                }
            },
            || {},
        )
        .await
        .map_err(|f| f.to_string())?;
    app.restart();
}

/// Beim Start nachsehen (Modus „auto“ oder „hinweis“), ein paar Sekunden
/// später, damit der Start nicht langsamer wird.
fn update_beim_start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        warten(5).await;
        let modus = app.state::<AppZustand>().e.lock().map(|e| e.oberflaeche.updates.clone()).unwrap_or_default();
        if modus == "aus" {
            return;
        }
        match update_suchen(&app).await {
            Ok(Some(info)) => {
                let _ = app.emit("update", &info);
                if modus == "auto"
                    && let Err(f) = update_einspielen(&app).await
                {
                    let _ = app.emit("hinweis", format!("Update ging nicht: {f}"));
                }
            }
            Ok(None) => {}
            Err(f) => eprintln!("Update-Suche ging nicht: {f}"),
        }
    });
}

/// Ein paar Sekunden warten, ohne die Oberfläche aufzuhalten.
async fn warten(sekunden: u64) {
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(std::time::Duration::from_secs(sekunden))).await;
}

/// Einstellungen: jetzt nach einer neuen Version suchen.
#[tauri::command]
async fn update_pruefen(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    update_suchen(&app).await
}

/// Gefundenes Update jetzt installieren (MaskCue startet danach neu).
#[tauri::command]
async fn update_installieren(app: AppHandle) -> Result<(), String> {
    update_einspielen(&app).await
}

/// Schon gefundenes Update, falls die Oberfläche das Ereignis verpasst hat.
#[tauri::command]
fn update_stand(z: State<'_, AppZustand>) -> Option<UpdateInfo> {
    z.update.lock().ok()?.as_ref().map(|n| UpdateInfo { version: n.version.clone(), notiz: n.body.clone().unwrap_or_default() })
}

#[derive(Serialize)]
struct Geraete {
    eingaenge: Vec<String>,
    ausgaenge: Vec<String>,
}

#[derive(Serialize)]
struct Start {
    einstellungen: Einstellungen,
    geraete: Geraete,
    version: String,
    /// Ordner der eigenen Figuren-Bilder (für das Anzeigen über asset://).
    bilder_ordner: String,
}

fn geraete_lesen() -> Geraete {
    let eingaenge = audio::geraete::liste().map(|(e, _)| e).unwrap_or_default();
    let ausgaenge = audio::wasapi_aus::ausgaenge().unwrap_or_default();
    Geraete { eingaenge, ausgaenge }
}

#[tauri::command]
fn start(app: AppHandle, z: State<'_, AppZustand>) -> Start {
    Start {
        einstellungen: z.e.lock().expect("Einstellungen").clone(),
        geraete: geraete_lesen(),
        version: app.package_info().version.to_string(),
        bilder_ordner: einstellungen::bilder_ordner().to_string_lossy().into_owned(),
    }
}

#[tauri::command]
fn geraete() -> Geraete {
    geraete_lesen()
}

fn uebernehmen(app: &AppHandle, z: &AppZustand, e: Einstellungen) {
    let (profile_neu, overlay_neu) = {
        let mut alt = z.e.lock().expect("Einstellungen");
        let neu = alt.hotkeys.profile.iter().map(|p| &p.name).ne(e.hotkeys.profile.iter().map(|p| &p.name));
        let overlay = (alt.oberflaeche.overlay, &alt.oberflaeche.overlay_ecke) != (e.oberflaeche.overlay, &e.oberflaeche.overlay_ecke);
        *alt = e.clone();
        (neu, overlay)
    };
    // Das Overlay zeigt Figuren-Bilder und Musik: Es braucht die neuen Einstellungen.
    let _ = app.emit("einstellungen", &e);
    if overlay_neu {
        overlay_abgleichen(app, &e.oberflaeche);
    }
    z.motor.einstellungen(e);
    if profile_neu {
        tray_menue(app);
    }
}

const OVERLAY_BREITE: f64 = 260.0;
const OVERLAY_HOEHE: f64 = 52.0;

/// Overlay über dem Spiel: eigenes durchsichtiges Fenster, immer oben, Klicks
/// gehen durch, nimmt dem Spiel nie den Fokus. Kein Einhaken ins Spiel
/// (Anti-Cheat). Sichtbar bei „Randloses Fenster“, nicht bei exklusivem Vollbild.
fn overlay_abgleichen(app: &AppHandle, o: &einstellungen::Oberflaeche) {
    let vorhanden = app.get_webview_window("overlay");
    if !o.overlay {
        if let Some(w) = vorhanden {
            let _ = w.destroy();
        }
        return;
    }
    let w = match vorhanden {
        Some(w) => w,
        None => match WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("index.html".into()))
            .title("MaskCue Overlay")
            .transparent(true)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .focused(false)
            .shadow(false)
            .visible(false)
            .inner_size(OVERLAY_BREITE, OVERLAY_HOEHE)
            .build()
        {
            Ok(w) => w,
            Err(f) => {
                let _ = app.emit("hinweis", format!("Overlay geht nicht: {f}"));
                return;
            }
        },
    };
    // In die gewählte Ecke des Hauptbildschirms, innerhalb der Arbeitsfläche (ohne Taskleiste).
    if let Ok(Some(m)) = w.primary_monitor() {
        let flaeche = m.work_area();
        let s = m.scale_factor();
        let (b, h, rand) = ((OVERLAY_BREITE * s) as i32, (OVERLAY_HOEHE * s) as i32, (12.0 * s) as i32);
        let (x0, y0) = (flaeche.position.x, flaeche.position.y);
        let (x1, y1) = (x0 + flaeche.size.width as i32, y0 + flaeche.size.height as i32);
        let (x, y) = match o.overlay_ecke.as_str() {
            "oben-links" => (x0 + rand, y0 + rand),
            "unten-links" => (x0 + rand, y1 - h - rand),
            "unten-rechts" => (x1 - b - rand, y1 - h - rand),
            _ => (x1 - b - rand, y0 + rand),
        };
        let _ = w.set_position(PhysicalPosition::new(x, y));
    }
    let _ = w.set_ignore_cursor_events(true);
    match w.hwnd() {
        Ok(hwnd) => overlay_zeigen_ohne_fokus(hwnd.0 as isize),
        Err(_) => {
            let _ = w.show();
        }
    }
}

/// Fenster zeigen, ohne es zu aktivieren: Das Spiel behält Fokus und Tasten.
fn overlay_zeigen_ohne_fokus(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, SW_SHOWNOACTIVATE, SetWindowLongPtrW, ShowWindow, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };
    let h = HWND(hwnd as *mut core::ffi::c_void);
    unsafe {
        let stil = GetWindowLongPtrW(h, GWL_EXSTYLE);
        SetWindowLongPtrW(h, GWL_EXSTYLE, stil | WS_EX_NOACTIVATE.0 as isize | WS_EX_TOOLWINDOW.0 as isize);
        let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
    }
}

/// Großes Symbol (Taskleiste) aus der Programm-Datei setzen. Tauri setzt nur
/// das kleine; ohne großes nimmt Windows das Symbol aus seinem Zwischenspeicher,
/// und der zeigt nach einem Update noch das alte (nachgestellt 17.10.2026).
fn taskleisten_symbol(hwnd: isize) {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, ICON_BIG, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW, SM_CXICON, SM_CYICON, SendMessageW, WM_SETICON,
    };
    use windows::core::PCWSTR;
    unsafe {
        let Ok(modul) = GetModuleHandleW(None) else { return };
        // Symbol-Nummer, unter der tauri-build das Symbol in die .exe legt
        // (tauri_utils::platform::WINDOWS_APP_ICON_RESOURCE_ID).
        let id = PCWSTR(32512usize as *const u16);
        let Ok(symbol) = LoadImageW(Some(modul.into()), id, IMAGE_ICON, GetSystemMetrics(SM_CXICON), GetSystemMetrics(SM_CYICON), LR_DEFAULTCOLOR) else {
            return;
        };
        SendMessageW(HWND(hwnd as *mut core::ffi::c_void), WM_SETICON, Some(WPARAM(ICON_BIG as usize)), Some(LPARAM(symbol.0 as isize)));
    }
}

/// Async, weil `uebernehmen` das Overlay-Fenster anlegen kann: In einem
/// synchronen Befehl blockiert sich die App unter Windows dabei selbst
/// (nachgestellt: Befehle antworteten danach nicht mehr).
#[tauri::command]
async fn einstellungen_setzen(app: AppHandle, z: State<'_, AppZustand>, mut e: Einstellungen) -> Result<(), String> {
    e.sanieren();
    uebernehmen(&app, &z, e);
    Ok(())
}

/// Datei-Auswahl für Sounds. Gibt die neuen Einstellungen zurück.
#[tauri::command]
async fn sounds_waehlen(app: AppHandle, z: State<'_, AppZustand>) -> Result<Einstellungen, String> {
    let gewaehlt = app.dialog().file().set_title("Sounds hinzufügen").add_filter("Ton-Dateien", &["mp3", "wav", "ogg", "flac"]).blocking_pick_files();
    let mut e = z.e.lock().map_err(|f| f.to_string())?.clone();
    if let Some(dateien) = gewaehlt {
        let pfade: Vec<_> = dateien.into_iter().filter_map(|d| d.into_path().ok()).collect();
        e.sounds_hinzufuegen(&pfade);
        uebernehmen(&app, &z, e.clone());
    }
    Ok(e)
}

/// Eigenes Bild für eine Figur wählen. Gibt "datei:<name>" zurück, oder
/// `None`, wenn nichts gewählt wurde.
#[tauri::command]
async fn bild_hochladen(app: AppHandle) -> Result<Option<String>, String> {
    let gewaehlt = app.dialog().file().set_title("Bild für die Figur").add_filter("Bilder", &["png", "jpg", "jpeg", "webp", "gif"]).blocking_pick_file();
    let Some(datei) = gewaehlt else { return Ok(None) };
    let pfad = datei.into_path().map_err(|f| f.to_string())?;
    einstellungen::bild_uebernehmen(&pfad, &einstellungen::bilder_ordner()).map(Some)
}

/// Figur mit den gewählten Sounds als ZIP speichern. Gibt den Pfad zurück
/// oder `None`, wenn abgebrochen.
#[tauri::command]
async fn figur_exportieren(app: AppHandle, z: State<'_, AppZustand>, name: String, sounds: Vec<String>) -> Result<Option<String>, String> {
    let e = z.e.lock().map_err(|f| f.to_string())?.clone();
    let figur = e.figuren.iter().find(|f| f.name == name).ok_or_else(|| format!("Figur „{name}“ nicht gefunden"))?.clone();
    let gewaehlt: Vec<_> = e.sounds.iter().filter(|s| sounds.contains(&s.name)).cloned().collect();
    let datei_name: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
    let ziel = app.dialog().file().set_title("Figur exportieren").set_file_name(format!("{datei_name}.maskcue.zip")).add_filter("MaskCue-Figur", &["zip"]).blocking_save_file();
    let Some(ziel) = ziel else { return Ok(None) };
    let pfad = ziel.into_path().map_err(|f| f.to_string())?;
    app::paket::exportieren(&figur, &gewaehlt, &einstellungen::bilder_ordner(), &pfad)?;
    Ok(Some(pfad.to_string_lossy().into_owned()))
}

#[derive(Serialize)]
struct ImportErgebnis {
    einstellungen: Einstellungen,
    name: String,
    sounds: usize,
    uebersprungen: usize,
}

/// Figur aus einem ZIP importieren (mit Schutz, siehe app/paket.rs).
#[tauri::command]
async fn figur_importieren(app: AppHandle, z: State<'_, AppZustand>) -> Result<Option<ImportErgebnis>, String> {
    let gewaehlt = app.dialog().file().set_title("Figur importieren").add_filter("MaskCue-Figur", &["zip"]).blocking_pick_file();
    let Some(datei) = gewaehlt else { return Ok(None) };
    let pfad = datei.into_path().map_err(|f| f.to_string())?;
    let imp = app::paket::importieren(&pfad, &einstellungen::bilder_ordner(), &einstellungen::ordner().join("sounds"), app::paket::Grenzen::default())?;
    let (sounds, uebersprungen) = (imp.sounds.len(), imp.uebersprungen);
    let mut e = z.e.lock().map_err(|f| f.to_string())?.clone();
    let name = app::paket::einfuegen(&mut e, imp);
    uebernehmen(&app, &z, e.clone());
    Ok(Some(ImportErgebnis { einstellungen: e, name, sounds, uebersprungen }))
}

/// Alle Einstellungen als ZIP sichern (siehe app/sicherung.rs).
#[tauri::command]
async fn einstellungen_exportieren(app: AppHandle, z: State<'_, AppZustand>, mit_sounds: bool) -> Result<Option<String>, String> {
    let e = z.e.lock().map_err(|f| f.to_string())?.clone();
    let ziel = app
        .dialog()
        .file()
        .set_title("Einstellungen sichern")
        .set_file_name("MaskCue-Einstellungen.zip")
        .add_filter("MaskCue-Sicherung", &["zip"])
        .blocking_save_file();
    let Some(ziel) = ziel else { return Ok(None) };
    let pfad = ziel.into_path().map_err(|f| f.to_string())?;
    app::sicherung::exportieren(&e, &einstellungen::bilder_ordner(), mit_sounds, &pfad)?;
    Ok(Some(pfad.to_string_lossy().into_owned()))
}

#[derive(Serialize)]
struct SicherungErgebnis {
    einstellungen: Einstellungen,
    sounds: usize,
    bilder: usize,
    uebersprungen: usize,
    /// Kopie der alten Einstellungen.
    kopie: Option<String>,
}

/// Sicherung einspielen: ersetzt alle Einstellungen. Die alten bleiben als Kopie liegen.
#[tauri::command]
async fn einstellungen_importieren(app: AppHandle, z: State<'_, AppZustand>) -> Result<Option<SicherungErgebnis>, String> {
    let gewaehlt = app.dialog().file().set_title("Einstellungen einspielen").add_filter("MaskCue-Sicherung", &["zip"]).blocking_pick_file();
    let Some(datei) = gewaehlt else { return Ok(None) };
    let pfad = datei.into_path().map_err(|f| f.to_string())?;
    let imp = app::sicherung::importieren(&pfad, &einstellungen::bilder_ordner(), &einstellungen::ordner().join("sounds"), app::paket::Grenzen::default())?;
    let kopie = app::sicherung::alte_sichern(&einstellungen::datei())?;
    uebernehmen(&app, &z, imp.einstellungen.clone());
    Ok(Some(SicherungErgebnis {
        einstellungen: imp.einstellungen,
        sounds: imp.sounds,
        bilder: imp.bilder,
        uebersprungen: imp.uebersprungen,
        kopie: kopie.map(|k| k.to_string_lossy().into_owned()),
    }))
}

#[tauri::command]
async fn sound_umbenennen(app: AppHandle, z: State<'_, AppZustand>, alt: String, neu: String) -> Result<Einstellungen, String> {
    let mut e = z.e.lock().map_err(|f| f.to_string())?.clone();
    e.sound_umbenennen(&alt, &neu)?;
    uebernehmen(&app, &z, e.clone());
    Ok(e)
}

/// Warnungen zu den Hotkeys (doppelte Tasten, unbekannte Sounds und Figuren).
#[tauri::command]
fn hotkeys_pruefen(e: Einstellungen) -> Vec<String> {
    let mut w = e.hotkeys.pruefen();
    let alle = e.hotkeys.global.iter().chain(e.hotkeys.profile.iter().flat_map(|p| &p.belegungen));
    for b in alle {
        match &b.aktion {
            hotkeys::profil::Aktion::Sound(n) if !e.sounds.iter().any(|s| &s.name == n) => w.push(format!("{}: Sound „{n}“ gibt es nicht.", b.taste)),
            hotkeys::profil::Aktion::Figur(n) if !e.figuren.iter().any(|f| f.name.eq_ignore_ascii_case(n)) => w.push(format!("{}: Figur „{n}“ gibt es nicht.", b.taste)),
            _ => {}
        }
    }
    w
}

#[tauri::command]
fn ordner_oeffnen(app: AppHandle) -> Result<(), String> {
    let o = einstellungen::ordner();
    std::fs::create_dir_all(&o).map_err(|f| f.to_string())?;
    app.opener().open_path(o.to_string_lossy(), None::<&str>).map_err(|f| f.to_string())
}

#[tauri::command]
fn ausloesen(z: State<'_, AppZustand>, name: String) {
    z.motor.ausloesen(name);
}

#[tauri::command]
fn sound_pause(z: State<'_, AppZustand>, name: String) {
    z.motor.sound_pause(name);
}

#[tauri::command]
fn sound_stopp(z: State<'_, AppZustand>, name: String) {
    z.motor.sound_stopp(name);
}

#[tauri::command]
fn sound_springen(z: State<'_, AppZustand>, name: String, anteil: f32) {
    if anteil.is_finite() {
        z.motor.sound_springen(name, anteil);
    }
}

#[tauri::command]
fn alles_stoppen(z: State<'_, AppZustand>) {
    z.motor.alles_stopp();
}

#[tauri::command]
fn figur(z: State<'_, AppZustand>, name: Option<String>) {
    z.motor.figur(name);
}

#[tauri::command]
fn profil(z: State<'_, AppZustand>, name: Option<String>) {
    z.motor.profil(name);
}

#[tauri::command]
fn lernen(z: State<'_, AppZustand>, an: bool) {
    z.motor.lernen(an);
}

/// Name einer Taste, die die Oberfläche selbst gesehen hat (Lern-Modus bei
/// MaskCue im Vordergrund: Dann kommen Tastatur-Tasten nicht sicher beim
/// Hook an, das WebView fängt sie ab). `vk`: Windows-Tastencode, `mods`:
/// Strg = 1, Alt = 2, Umschalt = 4, Win = 8.
#[tauri::command]
fn taste_benennen(vk: u8, mods: u8) -> Result<String, String> {
    hotkeys::taste::name_von_vk(vk, mods)
}

/// Taste aus der Oberfläche, wenn MaskCue vorne ist: wirkt wie ein Hotkey.
#[tauri::command]
fn taste_druecken(z: State<'_, AppZustand>, vk: u8, mods: u8) -> Result<(), String> {
    let k: hotkeys::taste::Kombination = hotkeys::taste::name_von_vk(vk, mods)?.parse()?;
    z.motor.taste(k);
    Ok(())
}

/// Klick-Ton der Oberfläche ("klick", "an", "aus"). Spielt nur, wenn eingeschaltet.
#[tauri::command]
fn ui_ton(z: State<'_, AppZustand>, art: String) {
    if let Some(t) = app::ui_toene::Ton::aus_text(&art) {
        z.motor.ui_ton(t);
    }
}

/// MaskCue-Stummtaste: an, aus oder umschalten (`None`).
#[tauri::command]
fn stumm(z: State<'_, AppZustand>, an: Option<bool>) {
    z.motor.stumm(an);
}

/// Aufnahme an, aus oder umschalten (`None`).
#[tauri::command]
fn aufnahme(z: State<'_, AppZustand>, an: Option<bool>) {
    z.motor.aufnahme(an);
}

/// Hörprobe einer KI-Stimme mit dem Probesatz (auf dem Abhör-Gerät).
#[tauri::command]
fn ki_probe(z: State<'_, AppZustand>, wahl: audio::ki::KiWahl, halbtoene: f32, formant: f32) -> Result<(), String> {
    let wahl = wahl.sauber().ok_or("Ungültiges KI-Modell")?;
    z.motor.ki_probe(wahl, halbtoene, formant);
    Ok(())
}

/// Schafft dieser PC die KI-Stimme? Rechnet 3 s Testsignal (ca. 1–4 s).
#[tauri::command]
async fn ki_tempo(wahl: audio::ki::KiWahl) -> Result<audio::ki::KiTempo, String> {
    let wahl = wahl.sauber().ok_or("Ungültiges KI-Modell")?;
    let ordner = app::ki_modelle::ordner().join(&wahl.modell);
    tauri::async_runtime::spawn_blocking(move || audio::ki::tempo_messen(&ordner, &wahl)).await.map_err(|f| f.to_string())?
}

/// Eigene Tonhöhe aus einer Aufnahme messen (Hz). `None`: zu wenig Stimme.
#[tauri::command]
async fn ki_tonhoehe_messen(name: String) -> Result<Option<f32>, String> {
    let datei = app::aufnahmen::pfad(&app::aufnahmen::standard_ordner(), &name)?;
    let schaetzer = app::ki_modelle::tonhoehen_schaetzer(&app::ki_modelle::ordner()).ok_or("Kein KI-Modell gefunden")?;
    let d = app::sounds::laden(&datei)?;
    let mono: Vec<f32> = d.samples.chunks(d.kanaele.max(1)).map(|c| c.iter().sum::<f32>() / c.len() as f32).collect();
    audio::ki::tonhoehe_messen(&schaetzer, &mono)
}

/// KI-Modelle mit ihren Stimmen.
#[tauri::command]
fn ki_modelle() -> Vec<app::ki_modelle::KiModellInfo> {
    app::ki_modelle::liste(&app::ki_modelle::ordner())
}

/// KI-Stimmen zum Herunterladen, mit „schon da“.
#[tauri::command]
fn ki_angebot() -> Vec<app::ki_modelle::KiAngebot> {
    app::ki_modelle::angebot(&app::ki_modelle::ordner())
}

/// KI-Stimme laden. Nur nach Zustimmung zu den Regeln (`ki.regeln_ok`).
/// Fortschritt als Ereignis „ki-download“: `[ordner, anteil 0…1]`.
#[tauri::command]
async fn ki_herunterladen(app: AppHandle, z: State<'_, AppZustand>, ordner: String) -> Result<(), String> {
    if z.e.lock().map_err(|f| f.to_string())?.ki.regeln_ok.is_none() {
        return Err("Bitte zuerst den Regeln für KI-Stimmen zustimmen.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let basis = app::ki_modelle::ordner();
        std::fs::create_dir_all(&basis).map_err(|f| f.to_string())?;
        let mut zuletzt = 0.0f32;
        app::ki_modelle::herunterladen(&ordner, &basis, |geladen, gesamt| {
            let anteil = geladen as f32 / gesamt.max(1) as f32;
            if anteil - zuletzt >= 0.01 || geladen == gesamt {
                zuletzt = anteil;
                let _ = app.emit("ki-download", (&ordner, anteil));
            }
        })
    })
    .await
    .map_err(|f| f.to_string())?
}

#[tauri::command]
fn aufnahmen_liste() -> Vec<app::aufnahmen::AufnahmeInfo> {
    app::aufnahmen::liste(&app::aufnahmen::standard_ordner())
}

/// Aufnahme auf dem Abhör-Gerät spielen (`None` = stoppen).
#[tauri::command]
fn aufnahme_abhoeren(z: State<'_, AppZustand>, name: Option<String>) -> Result<(), String> {
    let datei = name.map(|n| app::aufnahmen::pfad(&app::aufnahmen::standard_ordner(), &n)).transpose()?;
    z.motor.abhoeren(datei);
    Ok(())
}

/// Umbenennen. Ein Sound im Soundboard, der auf die Datei zeigt, zeigt danach auf die neue.
#[tauri::command]
async fn aufnahme_umbenennen(app: AppHandle, z: State<'_, AppZustand>, alt: String, neu: String) -> Result<Einstellungen, String> {
    let (a, n) = app::aufnahmen::umbenennen(&app::aufnahmen::standard_ordner(), &alt, &neu)?;
    let mut e = z.e.lock().map_err(|f| f.to_string())?.clone();
    for s in e.sounds.iter_mut().filter(|s| s.datei == a) {
        s.datei = n.clone();
    }
    uebernehmen(&app, &z, e.clone());
    Ok(e)
}

/// Löschen. Ein Sound im Soundboard mit dieser Datei fliegt mit raus.
#[tauri::command]
async fn aufnahme_loeschen(app: AppHandle, z: State<'_, AppZustand>, name: String) -> Result<Einstellungen, String> {
    z.motor.abhoeren(None);
    let p = app::aufnahmen::loeschen(&app::aufnahmen::standard_ordner(), &name)?;
    let mut e = z.e.lock().map_err(|f| f.to_string())?.clone();
    e.sounds.retain(|s| s.datei != p);
    uebernehmen(&app, &z, e.clone());
    Ok(e)
}

/// Aufnahme als Sound ins Soundboard (dann per Hotkey ins Spiel). Gibt den Sound-Namen zurück.
#[tauri::command]
async fn aufnahme_ins_soundboard(app: AppHandle, z: State<'_, AppZustand>, name: String) -> Result<(Einstellungen, String), String> {
    let p = app::aufnahmen::pfad(&app::aufnahmen::standard_ordner(), &name)?;
    if !p.is_file() {
        return Err(format!("Aufnahme „{name}“ gibt es nicht."));
    }
    let mut e = z.e.lock().map_err(|f| f.to_string())?.clone();
    if let Some(s) = e.sounds.iter().find(|s| s.datei == p) {
        return Ok((e.clone(), s.name.clone()));
    }
    let neu = e.sounds_hinzufuegen(std::slice::from_ref(&p)).pop().unwrap_or(name);
    uebernehmen(&app, &z, e.clone());
    Ok((e, neu))
}

/// Umgebung umschalten wie mit dem Hotkey (`None` = jede Umgebung aus).
#[tauri::command]
fn umgebung(z: State<'_, AppZustand>, art: Option<audio::umgebung::Umgebung>) {
    z.motor.umgebung(art);
}

#[tauri::command]
fn neustart(z: State<'_, AppZustand>) {
    z.motor.neustart();
}

/// Startparameter, wenn Windows MaskCue beim Anmelden startet.
const AUTOSTART_ARG: &str = "--autostart";

/// Startet MaskCue mit Windows? (Eintrag in der Registry, nicht in den Einstellungen.)
#[tauri::command]
fn autostart_lesen(app: AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|f| f.to_string())
}

/// Mit Windows starten an oder aus. Gibt den neuen Stand zurück.
#[tauri::command]
fn autostart_setzen(app: AppHandle, an: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let a = app.autolaunch();
    if an { a.enable() } else { a.disable() }.map_err(|f| f.to_string())?;
    a.is_enabled().map_err(|f| f.to_string())
}

fn fenster_zeigen(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Menü des Taskleisten-Symbols: Profile von Hand, Notbremse, Öffnen, Beenden.
fn menue_bauen(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let profile: Vec<String> = app.try_state::<AppZustand>().map(|z| z.e.lock().map(|e| e.hotkeys.profile.iter().map(|p| p.name.clone()).collect()).unwrap_or_default()).unwrap_or_default();
    let m = Menu::new(app)?;
    m.append(&MenuItem::with_id(app, "oeffnen", "MaskCue öffnen", true, None::<&str>)?)?;
    m.append(&MenuItem::with_id(app, "stopp", "Alles stoppen", true, None::<&str>)?)?;
    m.append(&MenuItem::with_id(app, "stumm", "Stumm an/aus", true, None::<&str>)?)?;
    m.append(&PredefinedMenuItem::separator(app)?)?;
    m.append(&MenuItem::with_id(app, "profil:", "Profil: automatisch", true, None::<&str>)?)?;
    for p in profile.iter().map(String::as_str).chain([PROFIL_AUS]) {
        m.append(&MenuItem::with_id(app, format!("profil:{p}"), format!("Profil: {p}"), true, None::<&str>)?)?;
    }
    m.append(&PredefinedMenuItem::separator(app)?)?;
    m.append(&MenuItem::with_id(app, "beenden", "Beenden", true, None::<&str>)?)?;
    Ok(m)
}

fn tray_menue(app: &AppHandle) {
    if let (Some(t), Ok(m)) = (app.tray_by_id("maskcue"), menue_bauen(app)) {
        let _ = t.set_menu(Some(m));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        // Nur eine MaskCue: Eine zweite würde um Mikrofon und Hotkeys streiten.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| fenster_zeigen(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        // Mit Windows starten: Windows ruft MaskCue dann mit --autostart auf.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec![AUTOSTART_ARG])))
        .setup(|app| {
            // Denoise-Modelle: mitgelieferte Ressourcen der installierten App.
            // KI-Stimmen lädt der Spieler selbst (Seite Figuren, KI-Stimme).
            // Neue Installation: Start-Sounds kopieren und eintragen.
            if let Ok(r) = app.path().resource_dir() {
                let ordner = r.join("modelle");
                if audio::denoise::ist_modell_ordner(&ordner) {
                    audio::denoise::modell_ordner_setzen(ordner);
                }
                if let Err(f) = einstellungen::neu_einrichten(&einstellungen::datei(), &r.join("sounds"), &einstellungen::ordner().join("sounds").join("start")) {
                    eprintln!("Start-Sounds einrichten ging nicht: {f}");
                }
            }
            let handle = app.handle().clone();
            let (motor, e) = MotorGriff::start(einstellungen::datei(), move |m| match m {
                Meldung::Status(s) => {
                    if let Some(z) = handle.try_state::<AppZustand>()
                        && let Ok(mut alt) = z.tray_profil.lock()
                        && *alt != s.profil
                    {
                        alt.clone_from(&s.profil);
                        if let Some(t) = handle.tray_by_id("maskcue") {
                            let _ = t.set_tooltip(Some(format!("MaskCue – Profil {}", s.profil)));
                        }
                    }
                    let _ = handle.emit("status", s);
                }
                Meldung::Taste(t) => {
                    let _ = handle.emit("taste", t);
                }
                Meldung::Hinweis(h) => {
                    let _ = handle.emit("hinweis", h);
                }
                Meldung::AufnahmeFertig(r) => {
                    let _ = handle.emit("aufnahme-fertig", r);
                }
            });
            let oberflaeche = e.oberflaeche.clone();
            app.manage(AppZustand { motor, e: Mutex::new(e), tray_profil: Mutex::new(String::new()), update: Mutex::new(None) });
            overlay_abgleichen(app.handle(), &oberflaeche);
            if let Some(Ok(hwnd)) = app.get_webview_window("main").map(|w| w.hwnd()) {
                taskleisten_symbol(hwnd.0 as isize);
            }

            TrayIconBuilder::with_id("maskcue")
                .icon(app.default_window_icon().cloned().expect("Symbol"))
                .tooltip("MaskCue")
                .menu(&menue_bauen(app.handle())?)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, ereignis| {
                    let id = ereignis.id.as_ref();
                    let z = app.state::<AppZustand>();
                    match id {
                        "oeffnen" => fenster_zeigen(app),
                        "stopp" => z.motor.alles_stopp(),
                        "stumm" => z.motor.stumm(None),
                        "beenden" => app.exit(0),
                        _ => {
                            if let Some(p) = id.strip_prefix("profil:") {
                                z.motor.profil((!p.is_empty()).then(|| p.to_string()));
                            }
                        }
                    }
                })
                .on_tray_icon_event(|tray, ereignis| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ereignis {
                        fenster_zeigen(tray.app_handle());
                    }
                })
                .build(app)?;
            // Beim Autostart unsichtbar im Tray bleiben, sonst Fenster zeigen.
            if !std::env::args().any(|a| a == AUTOSTART_ARG) {
                fenster_zeigen(app.handle());
            }
            update_beim_start(app.handle().clone());
            Ok(())
        })
        // Fenster schließen = in die Taskleiste. Der Ton läuft weiter.
        .on_window_event(|w, ereignis| {
            if let WindowEvent::CloseRequested { api, .. } = ereignis
                && w.label() == "main"
            {
                api.prevent_close();
                let _ = w.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            start,
            autostart_lesen,
            autostart_setzen,
            update_pruefen,
            update_installieren,
            update_stand,
            geraete,
            einstellungen_setzen,
            sounds_waehlen,
            sound_umbenennen,
            bild_hochladen,
            figur_exportieren,
            figur_importieren,
            einstellungen_exportieren,
            einstellungen_importieren,
            hotkeys_pruefen,
            ordner_oeffnen,
            ausloesen,
            sound_pause,
            sound_stopp,
            sound_springen,
            alles_stoppen,
            figur,
            profil,
            lernen,
            taste_benennen,
            taste_druecken,
            ui_ton,
            stumm,
            umgebung,
            aufnahme,
            aufnahmen_liste,
            ki_modelle,
            ki_angebot,
            ki_herunterladen,
            ki_probe,
            ki_tempo,
            ki_tonhoehe_messen,
            aufnahme_abhoeren,
            aufnahme_umbenennen,
            aufnahme_loeschen,
            aufnahme_ins_soundboard,
            neustart
        ])
        .build(tauri::generate_context!())
        .expect("MaskCue startet nicht");
    app.run(|app, ereignis| {
        if let RunEvent::Exit = ereignis
            && let Some(z) = app.try_state::<AppZustand>()
        {
            // Speichern und Geräte freigeben, bevor das Programm endet.
            z.motor.beenden();
        }
    });
}
