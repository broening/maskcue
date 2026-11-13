// Verbindung zum Motor (Rust). Läuft die Seite ohne Tauri (Vorschau im
// Browser), antwortet ein Ersatz mit Beispiel-Daten. So lässt sich die
// Oberfläche auch ohne Ton-Motor prüfen.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { AufnahmeInfo, Einstellungen, Geraete, KiAngebot, KiModell, UpdateInfo, KiTempo, KiWahl, Start, Status, Umgebung } from "./typen";

export const mitTauri = "__TAURI_INTERNALS__" in window;

/** Läuft diese Seite im Overlay-Fenster? (Vorschau im Browser: ?overlay) */
export const istOverlay = mitTauri ? getCurrentWebviewWindow().label === "overlay" : location.search.includes("overlay");

type Hoerer<T> = (wert: T) => void;

export interface Api {
  start(): Promise<Start>;
  geraete(): Promise<Geraete>;
  einstellungenSetzen(e: Einstellungen): Promise<void>;
  soundsWaehlen(): Promise<Einstellungen>;
  soundUmbenennen(alt: string, neu: string): Promise<Einstellungen>;
  bildHochladen(): Promise<string | null>;
  figurExportieren(name: string, sounds: string[]): Promise<string | null>;
  figurImportieren(): Promise<{ einstellungen: Einstellungen; name: string; sounds: number; uebersprungen: number } | null>;
  /** Alle Einstellungen als ZIP sichern. Gibt den Pfad zurück (`null` = abgebrochen). */
  einstellungenExportieren(mitSounds: boolean): Promise<string | null>;
  /** Sicherung einspielen: ersetzt alle Einstellungen, die alten bleiben als Kopie. */
  einstellungenImportieren(): Promise<{ einstellungen: Einstellungen; sounds: number; bilder: number; uebersprungen: number; kopie: string | null } | null>;
  hotkeysPruefen(e: Einstellungen): Promise<string[]>;
  ausloesen(name: string): Promise<void>;
  /** Laufenden Sound anhalten oder weiterspielen. */
  soundPause(name: string): Promise<void>;
  /** Einen Sound stoppen (auch wenn „Nochmal drücken“ neu startet). */
  soundStopp(name: string): Promise<void>;
  /** Laufenden Sound an eine Stelle setzen (`anteil` 0…1 der Länge). */
  soundSpringen(name: string, anteil: number): Promise<void>;
  allesStoppen(): Promise<void>;
  figur(name: string | null): Promise<void>;
  profil(name: string | null): Promise<void>;
  lernen(an: boolean): Promise<void>;
  /** Name einer Taste aus Windows-Tastencode und Zusatz-Bits (Strg 1, Alt 2, Umschalt 4, Win 8). */
  tasteBenennen(vk: number, mods: number): Promise<string>;
  /** Taste, die die Oberfläche gesehen hat (MaskCue vorne): wirkt wie ein Hotkey. */
  tasteDruecken(vk: number, mods: number): Promise<void>;
  neustart(): Promise<void>;
  uiTon(art: "klick" | "an" | "aus"): Promise<void>;
  stumm(an: boolean | null): Promise<void>;
  /** Umgebung umschalten wie mit dem Hotkey (`null` = jede aus). */
  umgebung(art: Umgebung | null): Promise<void>;
  /** Aufnahme an, aus oder umschalten (`null`). */
  aufnahme(an: boolean | null): Promise<void>;
  aufnahmenListe(): Promise<AufnahmeInfo[]>;
  /** KI-Modelle mit ihren Stimmen (`%APPDATA%\MaskCue\ki-modelle`). */
  kiModelle(): Promise<KiModell[]>;
  /** Startet MaskCue mit Windows? */
  autostartLesen(): Promise<boolean>;
  /** Mit Windows starten an/aus. Gibt den neuen Stand zurück. */
  autostartSetzen(an: boolean): Promise<boolean>;
  /** Jetzt bei GitHub nach einer neuen Version suchen (`null` = aktuell). */
  updatePruefen(): Promise<UpdateInfo | null>;
  /** Gefundenes Update laden und installieren; MaskCue startet neu. */
  updateInstallieren(): Promise<void>;
  /** Schon gefundenes Update (falls die Oberfläche das Ereignis verpasst hat). */
  updateStand(): Promise<UpdateInfo | null>;
  aufUpdate(f: Hoerer<UpdateInfo>): void;
  aufUpdateLaedt(f: Hoerer<string>): void;
  aufUpdateFortschritt(f: Hoerer<number>): void;
  /** KI-Stimmen zum Herunterladen, mit „schon da“. */
  kiAngebot(): Promise<KiAngebot[]>;
  /** KI-Stimme laden (nur nach Zustimmung zu den Regeln). */
  kiHerunterladen(ordner: string): Promise<void>;
  /** Fortschritt beim Laden: Ordner und Anteil 0…1. */
  aufKiDownload(f: Hoerer<[string, number]>): void;
  /** Hörprobe mit dem Probesatz auf dem Abhör-Gerät. `halbtoene`: Tonhöhe der Figur (bei automatisch: fein). */
  kiProbe(wahl: KiWahl, halbtoene: number, formant: number): Promise<void>;
  /** Schafft dieser PC die KI-Stimme? Rechnet 3 s Testsignal. */
  kiTempo(wahl: KiWahl): Promise<KiTempo>;
  /** Eigene Tonhöhe (Hz) aus einer Aufnahme; `null` = zu wenig Stimme. */
  kiTonhoeheMessen(name: string): Promise<number | null>;
  /** Aufnahme auf dem Abhör-Gerät spielen (`null` = stoppen). */
  aufnahmeAbhoeren(name: string | null): Promise<void>;
  aufnahmeUmbenennen(alt: string, neu: string): Promise<Einstellungen>;
  aufnahmeLoeschen(name: string): Promise<Einstellungen>;
  /** Als Sound ins Soundboard. Gibt Einstellungen und den Namen des Sounds zurück. */
  aufnahmeInsSoundboard(name: string): Promise<[Einstellungen, string]>;
  aufAufnahmeFertig(f: Hoerer<{ Ok?: string; Err?: string }>): void;
  ordnerOeffnen(): Promise<void>;
  aufStatus(f: Hoerer<Status>): void;
  aufTaste(f: Hoerer<string>): void;
  aufHinweis(f: Hoerer<string>): void;
  aufEinstellungen(f: Hoerer<Einstellungen>): void;
}

const echt: Api = {
  start: () => invoke("start"),
  geraete: () => invoke("geraete"),
  einstellungenSetzen: (e) => invoke("einstellungen_setzen", { e }),
  soundsWaehlen: () => invoke("sounds_waehlen"),
  soundUmbenennen: (alt, neu) => invoke("sound_umbenennen", { alt, neu }),
  bildHochladen: () => invoke("bild_hochladen"),
  figurExportieren: (name, sounds) => invoke("figur_exportieren", { name, sounds }),
  figurImportieren: () => invoke("figur_importieren"),
  einstellungenExportieren: (mitSounds) => invoke("einstellungen_exportieren", { mitSounds }),
  einstellungenImportieren: () => invoke("einstellungen_importieren"),
  hotkeysPruefen: (e) => invoke("hotkeys_pruefen", { e }),
  ausloesen: (name) => invoke("ausloesen", { name }),
  soundPause: (name) => invoke("sound_pause", { name }),
  soundStopp: (name) => invoke("sound_stopp", { name }),
  soundSpringen: (name, anteil) => invoke("sound_springen", { name, anteil }),
  allesStoppen: () => invoke("alles_stoppen"),
  figur: (name) => invoke("figur", { name }),
  profil: (name) => invoke("profil", { name }),
  lernen: (an) => invoke("lernen", { an }),
  tasteBenennen: (vk, mods) => invoke("taste_benennen", { vk, mods }),
  tasteDruecken: (vk, mods) => invoke("taste_druecken", { vk, mods }),
  neustart: () => invoke("neustart"),
  uiTon: (art) => invoke("ui_ton", { art }),
  stumm: (an) => invoke("stumm", { an }),
  umgebung: (art) => invoke("umgebung", { art }),
  aufnahme: (an) => invoke("aufnahme", { an }),
  aufnahmenListe: () => invoke("aufnahmen_liste"),
  kiModelle: () => invoke("ki_modelle"),
  autostartLesen: () => invoke("autostart_lesen"),
  autostartSetzen: (an) => invoke("autostart_setzen", { an }),
  updatePruefen: () => invoke("update_pruefen"),
  updateInstallieren: () => invoke("update_installieren"),
  updateStand: () => invoke("update_stand"),
  aufUpdate: (f) => void listen<UpdateInfo>("update", (e) => f(e.payload)),
  aufUpdateLaedt: (f) => void listen<string>("update-laedt", (e) => f(e.payload)),
  aufUpdateFortschritt: (f) => void listen<number>("update-fortschritt", (e) => f(e.payload)),
  kiAngebot: () => invoke("ki_angebot"),
  kiHerunterladen: (ordner) => invoke("ki_herunterladen", { ordner }),
  aufKiDownload: (f) => void listen<[string, number]>("ki-download", (e) => f(e.payload)),
  kiProbe: (wahl, halbtoene, formant) => invoke("ki_probe", { wahl, halbtoene, formant }),
  kiTempo: (wahl) => invoke("ki_tempo", { wahl }),
  kiTonhoeheMessen: (name) => invoke("ki_tonhoehe_messen", { name }),
  aufnahmeAbhoeren: (name) => invoke("aufnahme_abhoeren", { name }),
  aufnahmeUmbenennen: (alt, neu) => invoke("aufnahme_umbenennen", { alt, neu }),
  aufnahmeLoeschen: (name) => invoke("aufnahme_loeschen", { name }),
  aufnahmeInsSoundboard: (name) => invoke("aufnahme_ins_soundboard", { name }),
  aufAufnahmeFertig: (f) => void listen<{ Ok?: string; Err?: string }>("aufnahme-fertig", (e) => f(e.payload)),
  ordnerOeffnen: () => invoke("ordner_oeffnen"),
  aufStatus: (f) => void listen<Status>("status", (e) => f(e.payload)),
  aufTaste: (f) => void listen<string>("taste", (e) => f(e.payload)),
  aufHinweis: (f) => void listen<string>("hinweis", (e) => f(e.payload)),
  aufEinstellungen: (f) => void listen<Einstellungen>("einstellungen", (e) => f(e.payload)),
};

// ---- Ersatz für die Vorschau im Browser ----

function beispiel(): Einstellungen {
  const sounds = ["Wind", "Kirchenglocken", "Geister rufen", "Engel Segen", "Erdbebeben", "Feuersiegel", "Knochen brechen", "Mönchgesang", "Zombie isst", "Grim Hollow – New World"];
  return {
    version: 1,
    geraete: { eingang: "Tischmikrofon (2- USB Audio Device)", spiel: "MaskCue", chat: "MaskCue Chat" },
    mikro: {
      hochpass_hz: 80,
      denoise_spiel: { modell: "leicht", daempfung_db: 100 },
      denoise_chat: { modell: "normal", daempfung_db: 100 },
      gate_spiel: { an: true, schwelle_db: -45 },
      gate_chat: { an: false, schwelle_db: -45 },
      kompressor: { an: true, schwelle_db: -20, verhaeltnis: 3, verstaerkung_db: 3 },
      bass_db: 0,
      hoehen_db: 0,
    },
    figuren: [
      { name: "Opa", halbtoene: -4, formant: -3, bild: "avatar:opa" },
      { name: "Dämon", halbtoene: -7, formant: -4, bild: "avatar:daemon" },
      { name: "Kobold", halbtoene: 5, formant: 4, bild: "avatar:kobold" },
      { name: "Papagei", halbtoene: 7, formant: 3, bild: "avatar:papagei" },
      { name: "Bär", halbtoene: -5, formant: -4, bild: "avatar:baer" },
      { name: "Geist", halbtoene: 2, formant: 1, bild: "avatar:geist", umgebung: "Halle" },
      { name: "Prinzessin", halbtoene: 0, formant: 0, bild: "avatar:prinzessin", verfahren: "ohne", ki_an: true, ki: { modell: "thorsten", stimmen: [{ nr: 0, gewicht: 1 }], laut_anteil: 0.25, tonhoehe_auto: true } },
    ],
    sounds: sounds.map((name, i) => ({
      name,
      datei: `C:/Users/Nutzer/Documents/ClownfishCustomSounds/${name}.wav`,
      lautstaerke: 1,
      schleife: i === 9,
      musik: i === 9,
      nochmal: "stopp",
      spiel: true,
      chat: i === 1,
    })),
    mischer: { ducking_db: -12 },
    umgebungen: {},
    hotkeys: {
      global: [{ taste: "Pause", aktion: "AllesStopp" }],
      profile: [
        {
          name: "FiveM",
          programme: ["FiveM*.exe"],
          belegungen: [
            { taste: "Num1", aktion: { Sound: "Wind" } },
            { taste: "Num2", aktion: { Sound: "Kirchenglocken" } },
            { taste: "Num7", aktion: { Figur: "Opa" } },
            { taste: "Num0", aktion: "FigurAus" },
            { taste: "Num8", aktion: { Umgebung: "Funk" } },
          ],
        },
        { name: "RedM", programme: ["RedM*.exe"], belegungen: [] },
      ],
    },
    oberflaeche: { sprache: "de", ui_toene: false, ui_toene_geraet: "", overlay: false, overlay_ecke: "oben-rechts", updates: "auto" },
    aufnahme: { abhoeren_geraet: "" },
  };
}

function ersatz(): Api {
  let e = beispiel();
  let figur: string | null = istOverlay ? "Dämon" : null;
  const kiDa = new Set<string>();
  let autostart = false;
  const updateHoerer = { laedt: [] as ((v: string) => void)[], fortschritt: [] as ((a: number) => void)[] };
  const kiHoerer = new Set<(w: [string, number]) => void>();
  const laufen = new Set<string>(istOverlay ? ["Kirchenglocken", "Grim Hollow – New World"] : []);
  // Laufzeit der Vorschau-Sounds: Stelle (s) und seit wann sie läuft (null = Pause).
  const stand = new Map<string, { pos: number; seit: number | null }>();
  const laenge = (name: string) => 12 + ([...name].reduce((x, c) => x + c.charCodeAt(0), 0) % 50);
  const stelle = (name: string) => {
    const st = stand.get(name);
    return st ? st.pos + (st.seit === null ? 0 : (Date.now() - st.seit) / 1000) : 0;
  };
  const statusHoerer: Hoerer<Status>[] = [];
  const tastenHoerer: Hoerer<string>[] = [];
  let lernen = false;
  let stumm = false;
  // Umgebung per Hotkey wie im Motor: undefined = die der Figur, null = aus.
  let umgebungHotkey: Umgebung | null | undefined = istOverlay ? "Funk" : undefined;
  const figurUmgebung = () => e.figuren.find((f) => f.name === figur)?.umgebung ?? null;
  const umgebungJetzt = () => (umgebungHotkey === undefined ? figurUmgebung() : umgebungHotkey);
  // Aufnahmen der Vorschau (nur im Speicher).
  let aufnahmeStart: number | null = null;
  let abhoerenBis = 0;
  const aufnahmen: AufnahmeInfo[] = [
    { name: "Funkspruch Zentrale", datei: "C:/Users/Nutzer/AppData/Roaming/MaskCue/aufnahmen/Funkspruch Zentrale.wav", sekunden: 6.4, zeit: 1792000000 },
    { name: "Aufnahme 1", datei: "C:/Users/Nutzer/AppData/Roaming/MaskCue/aufnahmen/Aufnahme 1.wav", sekunden: 12.9, zeit: 1791990000 },
  ];
  const fertigHoerer: Hoerer<{ Ok?: string; Err?: string }>[] = [];
  setInterval(() => {
    for (const name of laufen) {
      if (!stand.has(name)) stand.set(name, { pos: 0, seit: Date.now() });
      const schleife = e.sounds.find((x) => x.name === name)?.schleife;
      if (stelle(name) >= laenge(name)) {
        if (schleife) stand.set(name, { pos: 0, seit: Date.now() });
        else {
          laufen.delete(name);
          stand.delete(name);
        }
      }
    }
    const s: Status = {
      laeuft: true,
      fehler: null,
      profil: "FiveM",
      profil_von_hand: false,
      programm: "FiveM_b3095_GTAProcess.exe",
      admin_problem: false,
      figur,
      ausgaenge: [
        { name: "MaskCue (Elgato Virtual Audio)", latenz_ms: (figur ? 81 : 51) + Math.random() * 2, aussetzer: 0 },
        { name: "MaskCue Chat (Elgato Virtual Audio)", latenz_ms: 69 + Math.random() * 2, aussetzer: 0 },
      ],
      pegel_db: -70 + Math.random() * 45 * (Math.sin(Date.now() / 900) > 0 ? 1 : 0.15),
      sounds_laufen: [...laufen],
      sounds_stand: [...laufen].map((name) => ({ name, pos_s: stelle(name), laenge_s: laenge(name), pausiert: stand.get(name)?.seit === null })),
      sounds_fehler: [],
      sounds_laden: 0,
      stumm,
      spricht: !stumm && Math.sin(Date.now() / 900) > 0,
      umgebung: umgebungJetzt(),
      umgebung_hotkey: umgebungHotkey !== undefined && umgebungHotkey !== null,
      aufnahme_s: aufnahmeStart === null ? null : (Date.now() - aufnahmeStart) / 1000,
      abhoeren: Date.now() < abhoerenBis,
    };
    statusHoerer.forEach((f) => f(s));
  }, 100);
  addEventListener("keydown", (k) => {
    if (lernen) tastenHoerer.forEach((f) => f(k.key.length === 1 ? k.key.toUpperCase() : k.key));
  });
  return {
    start: async () => ({ einstellungen: structuredClone(e), geraete: await beispielGeraete(), version: "0.1.5 (Vorschau)", bilder_ordner: "" }),
    geraete: () => beispielGeraete(),
    einstellungenSetzen: async (neu) => void (e = structuredClone(neu)),
    soundsWaehlen: async () => structuredClone(e),
    soundUmbenennen: async (alt, neu) => {
      const s = e.sounds.find((x) => x.name === alt);
      if (s) s.name = neu;
      return structuredClone(e);
    },
    hotkeysPruefen: async () => [],
    bildHochladen: async () => "datei:vorschau.png",
    figurExportieren: async () => null,
    figurImportieren: async () => null,
    kiProbe: async () => {},
    // Vorschau: mit ?langsam oder ?knapp spielt sie einen langsamen PC.
    kiTempo: async () => {
      await new Promise((r) => setTimeout(r, 800));
      return location.search.includes("langsam") ? { mittel: 0.9, spitze: 1.4 } : location.search.includes("knapp") ? { mittel: 0.55, spitze: 0.85 } : { mittel: 0.2, spitze: 0.25 };
    },
    autostartLesen: async () => autostart,
    autostartSetzen: async (an) => (autostart = an),
    // Vorschau: mit ?update tut sie so, als gäbe es Version 0.9.0.
    updatePruefen: async () => (location.search.includes("update") ? { version: "0.9.0", notiz: "" } : null),
    updateInstallieren: async () => {
      for (const h of updateHoerer.laedt) h("0.9.0");
      for (let a = 0.1; a <= 1.001; a += 0.15) {
        await new Promise((r) => setTimeout(r, 200));
        for (const h of updateHoerer.fortschritt) h(a);
      }
    },
    updateStand: async () => (location.search.includes("update") ? { version: "0.9.0", notiz: "" } : null),
    aufUpdate: () => {},
    aufUpdateLaedt: (f) => void updateHoerer.laedt.push(f),
    aufUpdateFortschritt: (f) => void updateHoerer.fortschritt.push(f),
    kiTonhoeheMessen: async () => 118,
    // Vorschau: Stimmen sind erst nach dem (gespielten) Download da.
    kiModelle: async () =>
      [
        { ordner: "thorsten", name: "Thorsten", stimmen: [{ nr: 0, name: "de_thorsten", sprache: "de", geschlecht: "m" as const, hz: 123 }] },
      ].filter((m) => kiDa.has(m.ordner)),
    kiAngebot: async () => [
      { ordner: "thorsten", name: "Thorsten", bytes: 17_663_301, installiert: kiDa.has("thorsten") },
    ],
    kiHerunterladen: async (ordner) => {
      if (!e.ki?.regeln_ok) throw "Bitte zuerst den Regeln für KI-Stimmen zustimmen.";
      for (let a = 0.1; a <= 1.001; a += 0.1) {
        await new Promise((r) => setTimeout(r, 120));
        kiHoerer.forEach((f) => f([ordner, a]));
      }
      kiDa.add(ordner);
    },
    aufKiDownload: (f) => void kiHoerer.add(f),
    einstellungenExportieren: async () => null,
    einstellungenImportieren: async () => null,
    ausloesen: async (name) => {
      stand.delete(name);
      if (laufen.has(name)) laufen.delete(name);
      else laufen.add(name);
    },
    soundPause: async (name) => {
      const st = stand.get(name);
      if (st) stand.set(name, st.seit === null ? { pos: st.pos, seit: Date.now() } : { pos: stelle(name), seit: null });
    },
    soundStopp: async (name) => {
      laufen.delete(name);
      stand.delete(name);
    },
    soundSpringen: async (name, anteil) => {
      const st = stand.get(name);
      if (st) stand.set(name, { pos: anteil * laenge(name), seit: st.seit === null ? null : Date.now() });
    },
    allesStoppen: async () => {
      laufen.clear();
      stand.clear();
    },
    figur: async (name) => {
      if (name !== figur && umgebungHotkey === null) umgebungHotkey = undefined;
      figur = name;
    },
    profil: async () => {},
    lernen: async (an) => void (lernen = an),
    tasteDruecken: async () => {},
    tasteBenennen: async (vk, mods) => {
      const name = vk >= 65 && vk <= 90 ? String.fromCharCode(vk) : vk >= 96 && vk <= 105 ? `Num${vk - 96}` : vk >= 112 && vk <= 135 ? `F${vk - 111}` : `VK${vk}`;
      return [mods & 1 ? "Strg" : "", mods & 2 ? "Alt" : "", mods & 4 ? "Umschalt" : "", mods & 8 ? "Win" : "", name].filter(Boolean).join("+");
    },
    neustart: async () => {},
    uiTon: async () => {},
    stumm: async (an) => void (stumm = an ?? !stumm),
    aufnahme: async (an) => {
      const neu = an ?? aufnahmeStart === null;
      if (neu && aufnahmeStart === null) aufnahmeStart = Date.now();
      if (!neu && aufnahmeStart !== null) {
        const name = `Aufnahme ${aufnahmen.length + 1}`;
        aufnahmen.unshift({ name, datei: `C:/x/${name}.wav`, sekunden: (Date.now() - aufnahmeStart) / 1000, zeit: Date.now() / 1000 });
        aufnahmeStart = null;
        fertigHoerer.forEach((f) => f({ Ok: name }));
      }
    },
    aufnahmenListe: async () => structuredClone(aufnahmen),
    aufnahmeAbhoeren: async (name) => void (abhoerenBis = name ? Date.now() + 3000 : 0),
    aufnahmeUmbenennen: async (alt, neu) => {
      const a = aufnahmen.find((x) => x.name === alt);
      if (a) a.name = neu;
      return structuredClone(e);
    },
    aufnahmeLoeschen: async (name) => {
      aufnahmen.splice(aufnahmen.findIndex((x) => x.name === name), 1);
      return structuredClone(e);
    },
    aufnahmeInsSoundboard: async (name) => {
      const a = aufnahmen.find((x) => x.name === name);
      if (a) e.sounds.push({ name, datei: a.datei, lautstaerke: 1, schleife: false, musik: false, nochmal: "stopp", spiel: true, chat: false });
      return [structuredClone(e), name];
    },
    aufAufnahmeFertig: (f) => void fertigHoerer.push(f),
    umgebung: async (art) => {
      // Gleiche Regel wie `umgebung_regel` in motor.rs.
      const fu = figurUmgebung();
      const neu = art !== null && umgebungHotkey === art ? fu : art !== null && umgebungJetzt() === art ? null : art;
      umgebungHotkey = neu === fu ? undefined : neu;
    },
    ordnerOeffnen: async () => {},
    aufStatus: (f) => void statusHoerer.push(f),
    aufTaste: (f) => void tastenHoerer.push(f),
    aufHinweis: () => {},
    aufEinstellungen: () => {},
  };
}

async function beispielGeraete(): Promise<Geraete> {
  return {
    eingaenge: ["Tischmikrofon (2- USB Audio Device)", "Mikrofon (Elgato Wave:3)", "Headset-Mikrofon (Arctis 7)"],
    ausgaenge: ["MaskCue (Elgato Virtual Audio)", "MaskCue Chat (Elgato Virtual Audio)", "Game (Elgato Virtual Audio)", "Kopfhörer (Arctis 7)"],
  };
}

export const api: Api = mitTauri ? echt : ersatz();
