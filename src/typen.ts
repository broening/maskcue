// Gleiche Form wie die Rust-Seite (src-tauri/src/app/einstellungen.rs, motor.rs).

export type DenoiseModell = "aus" | "leicht" | "normal";

export interface DenoiseWahl {
  modell: DenoiseModell;
  daempfung_db: number;
}

export interface GateWahl {
  an: boolean;
  schwelle_db: number;
}

export interface Mikro {
  hochpass_hz: number;
  denoise_spiel: DenoiseWahl;
  denoise_chat: DenoiseWahl;
  gate_spiel: GateWahl;
  gate_chat: GateWahl;
  kompressor: { an: boolean; schwelle_db: number; verhaeltnis: number; verstaerkung_db: number };
  bass_db: number;
  hoehen_db: number;
}

/** Ausschnitt eines eigenen Bildes: Mitte in Prozent (0…100), Zoom 1…4. */
export interface Ausschnitt {
  x: number;
  y: number;
  zoom: number;
}

/** Stimm-Umgebung (src-tauri/src/audio/umgebung.rs). */
export type Umgebung = "Halle" | "Hoehle" | "Funk" | "Telepathie" | "Roboter" | "Maske" | "Raumhelm" | "Weltall" | "Megafon";

export const UMGEBUNGEN: Umgebung[] = ["Halle", "Hoehle", "Funk", "Telepathie", "Roboter", "Maske", "Raumhelm", "Weltall", "Megafon"];

/** Stärke ab Werk (wie `Umgebung::staerke_standard`). */
export function umgebungStaerke(e: Einstellungen, u: Umgebung): number {
  // Gleich wie `staerke_standard` in umgebung.rs.
  return e.umgebungen?.[u] ?? (u === "Halle" || u === "Hoehle" || u === "Weltall" ? 0.7 : 1);
}

export interface Figur {
  name: string;
  halbtoene: number;
  formant: number;
  /** "avatar:<id>", "farbe:#rrggbb" oder "datei:<name>"; fehlt = Szene bzw. Avatar mit gleichem Namen. */
  bild?: string;
  /** Ausschnitt des eigenen Bildes. */
  bild_ausschnitt?: Ausschnitt;
  /** Feste Umgebung der Figur; ein Umgebungs-Hotkey geht vor. */
  umgebung?: Umgebung;
  /** Eigene Stärke dieser Umgebung nur für diese Figur (0…1); fehlt = wie auf der Seite „Umgebungen“. */
  umgebung_staerke?: number;
  /** Verfahren für Tonhöhe; fehlt = "psola". "wsola": leicht, ohne Formant (Helium-Klang). */
  verfahren?: "psola" | "signalsmith" | "wsola" | "ohne";
  /** KI-Stimme (Beatrice): gewählte Stimme. Gilt, wenn `ki_an`. */
  ki?: KiWahl;
  /** KI-Stimme vor dem Verfahren an. */
  ki_an?: boolean;
  /** Hell (+) oder dunkel (−), −6 … +6 dB, nach der Stimme. */
  hell?: number;
}

/** KI-Stimme, für alle Figuren gleich. */
export interface KiAllgemein {
  /** Aufnahme mit der normalen Stimme für Hörproben. */
  probesatz?: string;
  /** Eigene mittlere Tonhöhe (Hz), gemessen am Probesatz. */
  eigene_hz?: number;
  /** Lieblings-Stimmen je Modell (Ordner → Nummern). */
  favoriten?: Record<string, number[]>;
  /** Datum der Zustimmung zu den Regeln für KI-Stimmen. Erst dann gibt es Downloads. */
  regeln_ok?: string;
}

/** Eine Stimme in einer KI-Mischung. */
export interface KiAnteil {
  nr: number;
  /** Gewicht 0…1 (wird auf Summe 1 gebracht). */
  gewicht: number;
}

export interface KiWahl {
  /** Ordner-Name des Modells. */
  modell: string;
  stimmen: KiAnteil[];
  /** 1 = Aussprache ganz von der Ziel-Stimme, 0 = eigene Aussprache. */
  laut_anteil: number;
  /** Tonhöhe automatisch zur Ziel-Stimme; die Tonhöhe der Figur ist dann Feinabstimmung. */
  tonhoehe_auto?: boolean;
}

/** Geschwindigkeitstest der KI-Stimme: Anteil der Echtzeit (0,3 = 30 %). */
export interface KiTempo {
  mittel: number;
  /** Langsamste 5 % der Blöcke. Über 1 knackt es. */
  spitze: number;
}

export interface KiStimmeInfo {
  nr: number;
  name: string;
  /** "de", "es", "en" … */
  sprache?: string;
  geschlecht?: "m" | "w";
  /** Rang 0…1: 1 = groß/tief. */
  groesse?: number;
  /** Rang 0…1: 1 = hell. */
  hell?: number;
  /** Rang 0…1: 1 = hauchig. */
  hauch?: number;
  /** Mittlere Tonhöhe (Hz). */
  hz?: number;
  /** Dialekt ("bayerisch" …), siehe `ki.d.*`. */
  dialekt?: string;
  /** Verständlichkeit 0…100. */
  verstaendlich?: number;
}

/** Neue Version aus den GitHub-Releases. */
export interface UpdateInfo {
  version: string;
  notiz: string;
}

/** KI-Stimme zum Herunterladen. */
export interface KiAngebot {
  ordner: string;
  name: string;
  bytes: number;
  installiert: boolean;
}

export interface KiModell {
  ordner: string;
  name: string;
  stimmen: KiStimmeInfo[];
}

export interface Sound {
  name: string;
  datei: string;
  lautstaerke: number;
  schleife: boolean;
  musik: boolean;
  nochmal: "stopp" | "neu";
  spiel: boolean;
  chat: boolean;
  /** Reiter auf der Sounds-Seite; fehlt = nur unter „Alle“. */
  kategorie?: string;
  /** Eigenes Bild der Kachel (wie bei Figuren); fehlt = Farbe aus dem Namen. */
  bild?: string;
  bild_ausschnitt?: Ausschnitt;
}

/** Stand eines laufenden Sounds (Laufzeit-Leiste). */
export interface SoundStand {
  name: string;
  pos_s: number;
  laenge_s: number;
  pausiert: boolean;
}

/** Wie serde ein Rust-Enum schreibt: ohne Wert als Text, mit Wert als Objekt. */
export type Aktion =
  | "AllesStopp"
  | "FigurAus"
  | "ProfilAutomatisch"
  | "StummUmschalten"
  | "UmgebungAus"
  | "AufnahmeUmschalten"
  | { Umgebung: Umgebung }
  | { Sound: string }
  | { Figur: string }
  | { Profil: string };

export interface Belegung {
  taste: string;
  aktion: Aktion;
}

export interface Profil {
  name: string;
  programme: string[];
  belegungen: Belegung[];
}

export interface Einstellungen {
  version: number;
  geraete: { eingang: string; spiel: string; chat: string };
  mikro: Mikro;
  figuren: Figur[];
  /** Bild der normalen Stimme; fehlt = neutrales Mikrofon. */
  normal_bild?: string;
  normal_ausschnitt?: Ausschnitt;
  sounds: Sound[];
  /** Eigene Reiter der Sounds-Seite. */
  sound_kategorien?: string[];
  mischer: { ducking_db: number };
  /** Stärke je Umgebung (0…1); fehlt = Standard. */
  umgebungen: Partial<Record<Umgebung, number>>;
  /** `ohne_spiel`: Profil für Programme ohne eigenes Profil (z. B. Desktop); fehlt = „Aus“. */
  hotkeys: { global: Belegung[]; profile: Profil[]; ohne_spiel?: string };
  oberflaeche: { sprache: string; ui_toene: boolean; ui_toene_geraet: string; overlay: boolean; overlay_ecke: string; updates?: "auto" | "hinweis" | "aus" };
  /** Aufnahme-Bereich: Gerät zum Anhören (leer = Gerät der Klick-Töne). */
  aufnahme: { abhoeren_geraet: string };
  ki?: KiAllgemein;
  stumm?: boolean;
}

export interface Geraete {
  eingaenge: string[];
  ausgaenge: string[];
}

export interface Start {
  einstellungen: Einstellungen;
  geraete: Geraete;
  version: string;
  bilder_ordner: string;
}

export interface AusgangStatus {
  name: string;
  latenz_ms: number | null;
  aussetzer: number;
}

export interface Status {
  laeuft: boolean;
  fehler: string | null;
  profil: string;
  profil_von_hand: boolean;
  programm: string;
  admin_problem: boolean;
  figur: string | null;
  ausgaenge: AusgangStatus[];
  pegel_db: number | null;
  sounds_laufen: string[];
  sounds_stand: SoundStand[];
  sounds_fehler: [string, string][];
  sounds_laden: number;
  stumm: boolean;
  spricht: boolean;
  /** Aktive Umgebung (von der Figur oder per Hotkey). */
  umgebung: Umgebung | null;
  /** Die Umgebung kommt vom Hotkey (nicht von der Figur). */
  umgebung_hotkey: boolean;
  /** Läuft eine Aufnahme: ihre Länge in Sekunden. */
  aufnahme_s: number | null;
  /** Eine Aufnahme wird gerade angehört. */
  abhoeren: boolean;
}

export interface AufnahmeInfo {
  name: string;
  datei: string;
  sekunden: number;
  /** Sekunden seit 1970. */
  zeit: number;
}

export const PROFIL_AUS = "Aus";
