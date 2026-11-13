// Bausteine nach Material 3 Expressive. Jeder Baustein gibt ein Element zurück.

import { api } from "../api";
import { t } from "../i18n";
import { symbol, type SymbolName } from "../icons";
import { h } from "./dom";

// ---------- Knöpfe ----------

export type KnopfArt = "gefuellt" | "spielen" | "tonal" | "umriss" | "text" | "fehler";

export function knopf(o: { text: string; symbol?: SymbolName; art?: KnopfArt; gross?: boolean; titel?: string; beiKlick: () => void }) {
  const b = h("button", { class: `knopf ebene ${o.art ?? "text"}${o.gross ? " gross" : ""}`, type: "button", title: o.titel, onclick: () => o.beiKlick() });
  if (o.symbol) b.append(symbol(o.symbol, true));
  b.append(o.text);
  return b;
}

export function symbolknopf(o: { symbol: SymbolName; titel: string; tonal?: boolean; gewaehlt?: boolean; beiKlick: () => void }) {
  return h(
    "button",
    { class: `symbolknopf ebene${o.tonal ? " tonal" : ""}${o.gewaehlt ? " gewaehlt" : ""}`, type: "button", title: o.titel, "aria-label": o.titel, onclick: () => o.beiKlick() },
    symbol(o.symbol, o.gewaehlt),
  );
}

// ---------- Schalter ----------

export function schalter(o: { text: string; hilfe?: string; wert: boolean; beiAenderung: (an: boolean) => void }) {
  const input = h("input", { type: "checkbox", role: "switch" });
  input.checked = o.wert;
  input.addEventListener("change", () => o.beiAenderung(input.checked));
  const text = h("span", { class: "schalter-text" }, h("span", { class: "text-l", text: o.text }));
  if (o.hilfe) text.append(h("span", { class: "text-m leise", style: "display:block", text: o.hilfe }));
  return h("label", { class: "schalter" }, text, input, h("span", { class: "schalter-spur" }, h("span", { class: "schalter-griff" }, symbol("haken"))));
}

// ---------- Regler ----------

export interface ReglerElement extends HTMLDivElement {
  setzen(wert: number): void;
}

export function regler(o: {
  titel: string;
  min: number;
  max: number;
  schritt: number;
  wert: number;
  format?: (v: number) => string;
  /** Aktive Spur von der Mitte aus (für ± Werte wie Bass, Tonhöhe). */
  mitte?: boolean;
  /** Wie `mitte`, aber mit eigenem Nullpunkt (z. B. 0 bei −15 … +30): Markierung,
   *  aktive Spur und Doppelklick gehen dorthin. */
  nullpunkt?: number;
  hilfe?: string;
  /** Zahl per Tastatur eintippen (Klick auf den Wert oder Ziffer tippen).
   *  Aus, wenn der angezeigte Wert nicht der Regler-Wert ist. */
  eingabe?: boolean;
  beiAenderung: (v: number) => void;
}): ReglerElement {
  const format = o.format ?? ((v: number) => String(v));
  const nullpunkt = o.nullpunkt ?? (o.mitte ? (o.min + o.max) / 2 : undefined);
  const pNull = nullpunkt === undefined ? 0 : (nullpunkt - o.min) / (o.max - o.min);
  const tippen = o.eingabe !== false;
  const wertText = tippen
    ? h("button", { class: "regler-wert etikett zahl tippbar", type: "button", title: t("allg.zahlTippen") })
    : h("span", { class: "regler-wert etikett zahl" });
  const eingabe = h("input", { class: "regler-eingabe etikett zahl", type: "text", inputmode: "decimal", "aria-label": `${o.titel}: ${t("allg.zahlTippen")}` }) as HTMLInputElement;
  eingabe.hidden = true;
  const aktiv = h("div", { class: "regler-aktiv" });
  const griff = h("div", { class: "regler-griff" });
  const blase = h("div", { class: "regler-blase etikett zahl" });
  const flaeche = h(
    "div",
    { class: "regler-flaeche", role: "slider", tabindex: 0, "aria-label": o.titel, "aria-valuemin": o.min, "aria-valuemax": o.max },
    h("div", { class: "regler-spur" }),
    nullpunkt !== undefined ? h("div", { class: "regler-mitte-punkt", style: `left:${pNull * 100}%` }) : null,
    aktiv,
    griff,
    blase,
  );
  const el = h("div", { class: `regler${nullpunkt !== undefined ? " mitte" : ""}` }, h("div", { class: "regler-kopf" }, h("span", { class: "text-l", text: o.titel }), wertText, eingabe), flaeche) as ReglerElement;
  if (o.hilfe) el.append(h("span", { class: "text-s leise", text: o.hilfe }));

  let wert = o.wert;
  const zeichnen = () => {
    const p = (wert - o.min) / (o.max - o.min);
    const prozent = `${p * 100}%`;
    griff.style.left = prozent;
    blase.style.left = prozent;
    if (nullpunkt !== undefined) {
      const a = Math.min(p, pNull);
      aktiv.style.left = `${a * 100}%`;
      aktiv.style.width = `${Math.abs(p - pNull) * 100}%`;
    } else {
      aktiv.style.left = "0";
      aktiv.style.width = prozent;
    }
    const text = format(wert);
    wertText.textContent = text;
    blase.textContent = text;
    flaeche.setAttribute("aria-valuenow", String(wert));
    flaeche.setAttribute("aria-valuetext", text);
  };
  const setzen = (v: number, melden: boolean) => {
    const gerundet = Math.round((Math.min(o.max, Math.max(o.min, v)) - o.min) / o.schritt) * o.schritt + o.min;
    const neu = Number(gerundet.toFixed(4));
    if (neu === wert) return;
    wert = neu;
    zeichnen();
    if (melden) o.beiAenderung(wert);
  };
  const ausX = (x: number) => {
    const r = flaeche.getBoundingClientRect();
    setzen(o.min + ((x - r.left) / r.width) * (o.max - o.min), true);
  };
  flaeche.addEventListener("pointerdown", (ev) => {
    flaeche.setPointerCapture(ev.pointerId);
    flaeche.classList.add("zieht");
    ausX(ev.clientX);
  });
  flaeche.addEventListener("pointermove", (ev) => {
    if (flaeche.hasPointerCapture(ev.pointerId)) ausX(ev.clientX);
  });
  const loslassen = () => flaeche.classList.remove("zieht");
  flaeche.addEventListener("pointerup", loslassen);
  flaeche.addEventListener("pointercancel", loslassen);
  flaeche.addEventListener("dblclick", () => nullpunkt !== undefined && setzen(nullpunkt, true));
  flaeche.addEventListener("keydown", (ev) => {
    const gross = Math.max(o.schritt, (o.max - o.min) / 10);
    const neu: Record<string, number> = {
      ArrowLeft: wert - o.schritt,
      ArrowDown: wert - o.schritt,
      ArrowRight: wert + o.schritt,
      ArrowUp: wert + o.schritt,
      PageDown: wert - gross,
      PageUp: wert + gross,
      Home: o.min,
      End: o.max,
    };
    if (ev.key in neu) {
      ev.preventDefault();
      setzen(neu[ev.key], true);
    }
  });
  // Zahl eintippen: Klick auf den Wert, oder Ziffer tippen, während der
  // Regler den Fokus hat. Enter oder Wegklicken übernimmt, Esc bricht ab.
  const bearbeiten = (start?: string) => {
    eingabe.value = start ?? String(wert).replace(".", t("allg.dezimal"));
    wertText.hidden = true;
    eingabe.hidden = false;
    eingabe.focus();
    if (start === undefined) eingabe.select();
  };
  const fertig = (uebernehmen: boolean) => {
    if (eingabe.hidden) return;
    const v = zahlLesen(eingabe.value);
    eingabe.hidden = true;
    wertText.hidden = false;
    if (uebernehmen && v !== null) setzen(v, true);
  };
  if (tippen) {
    wertText.addEventListener("click", () => bearbeiten());
    eingabe.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter" || ev.key === "Escape") {
        ev.preventDefault();
        fertig(ev.key === "Enter");
        flaeche.focus();
      }
    });
    eingabe.addEventListener("blur", () => fertig(true));
    flaeche.addEventListener("keydown", (ev) => {
      if (/^[0-9+\-,.]$/.test(ev.key) && !ev.ctrlKey && !ev.altKey && !ev.metaKey) {
        ev.preventDefault();
        bearbeiten(ev.key);
      }
    });
  }
  el.setzen = (v) => setzen(v, false);
  zeichnen();
  return el;
}

/** Zahl aus getipptem Text: Komma oder Punkt, Minus auch als „−“, Einheit
 *  dahinter (z. B. „-4,5 HT“) wird ignoriert. `null`, wenn keine Zahl drin ist. */
export function zahlLesen(text: string): number | null {
  const m = text.replace(/−/g, "-").replace(",", ".").match(/[-+]?(\d+\.?\d*|\.\d+)/);
  if (!m) return null;
  const v = Number(m[0]);
  return Number.isFinite(v) ? v : null;
}

// ---------- Verbundene Knopf-Gruppe ----------

/** `haken: false`: die Auswahl zeigt keinen Haken (z. B. Reiter). */
export function segmente<T extends string>(o: { optionen: { wert: T; text: string; symbol?: SymbolName }[]; wert: T; titel?: string; haken?: boolean; beiAenderung: (w: T) => void }) {
  const gruppe = h("div", { class: "segmente", role: "radiogroup", "aria-label": o.titel });
  const zeichnen = (aktuell: T) => {
    gruppe.replaceChildren(
      ...o.optionen.map((op) => {
        const an = op.wert === aktuell;
        const b = h("button", { class: `segment ebene${an ? " gewaehlt" : ""}`, type: "button", role: "radio", "aria-checked": String(an) });
        if (an && o.haken !== false) b.append(symbol("haken"));
        else if (op.symbol) b.append(symbol(op.symbol, an));
        b.append(op.text);
        b.addEventListener("click", () => {
          if (op.wert === aktuell) return;
          zeichnen(op.wert);
          o.beiAenderung(op.wert);
        });
        return b;
      }),
    );
  };
  zeichnen(o.wert);
  return gruppe;
}

// ---------- Felder ----------

export function auswahl(o: {
  titel: string;
  optionen: { wert: string; text: string }[];
  wert: string;
  hilfe?: string;
  fehler?: string;
  beiAenderung: (w: string) => void;
}) {
  const sel = h("select", { "aria-label": o.titel });
  for (const op of o.optionen) {
    const el = h("option", { value: op.wert, text: op.text });
    el.selected = op.wert === o.wert;
    sel.append(el);
  }
  sel.addEventListener("change", () => o.beiAenderung(sel.value));
  return h(
    "label",
    { class: "feld" },
    h("span", { class: "feld-beschriftung etikett", text: o.titel }),
    h("span", { class: `eingabe${o.fehler ? " fehler" : ""}` }, sel, symbol("pfeil_runter")),
    o.fehler ? h("span", { class: "feld-hilfe fehler text-s", text: o.fehler }) : o.hilfe ? h("span", { class: "feld-hilfe text-s", text: o.hilfe }) : null,
  );
}

export function texteingabe(o: { titel: string; wert: string; platzhalter?: string; hilfe?: string; beiAenderung: (w: string) => void }) {
  const inp = h("input", { type: "text", value: o.wert, placeholder: o.platzhalter, "aria-label": o.titel, spellcheck: "false" });
  inp.addEventListener("change", () => o.beiAenderung(inp.value));
  inp.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter") inp.blur();
  });
  return h(
    "label",
    { class: "feld" },
    h("span", { class: "feld-beschriftung etikett", text: o.titel }),
    h("span", { class: "eingabe" }, inp),
    o.hilfe ? h("span", { class: "feld-hilfe text-s", text: o.hilfe }) : null,
  );
}

// ---------- Flächen ----------

export function flaeche(titel: string, sym: SymbolName | null, ...inhalt: (Node | null | false)[]) {
  const kopf = h("div", { class: "flaeche-kopf" }, h("div", {}, sym ? symbol(sym) : null, h("h2", { class: "titel-m", text: titel })));
  const f = h("section", { class: "flaeche" }, kopf);
  for (const i of inhalt) if (i) f.append(i);
  return f;
}

// ---------- Meldungen ----------

const meldungen = h("div", { class: "meldungen", role: "status", "aria-live": "polite" });
document.body.append(meldungen);

/** Die letzten Meldungen mit Uhrzeit (für den Fehlerbericht). */
const verlauf: { zeit: string; text: string }[] = [];

export function letzteMeldungen(): { zeit: string; text: string }[] {
  return [...verlauf];
}

export function melden(text: string, ms = 5000) {
  verlauf.push({ zeit: new Date().toLocaleTimeString("de-DE"), text });
  if (verlauf.length > 20) verlauf.shift();
  const m = h("div", { class: "meldung text-m" }, symbol("info"), h("span", { text }));
  meldungen.append(m);
  setTimeout(() => m.remove(), ms);
  while (meldungen.children.length > 3) meldungen.firstElementChild?.remove();
}

// ---------- Hotkey-Feld ----------

let lauschend: { feld: HTMLButtonElement; fertig: (taste: string | null) => void } | null = null;
let lernZeit: ReturnType<typeof setTimeout> | null = null;

function lernenBeenden(taste: string | null) {
  if (!lauschend) return;
  const l = lauschend;
  lauschend = null;
  if (lernZeit) clearTimeout(lernZeit);
  removeEventListener("keydown", tasteImFenster, true);
  removeEventListener("mousedown", mausImFenster, true);
  void api.lernen(false);
  l.fertig(taste);
}

// Ist MaskCue selbst vorne, kommen Tastatur-Tasten nicht sicher beim Hook im
// Motor an (das WebView fängt sie vorher ab). Darum hört das Feld beim
// Lernen auch selbst zu. Wer zuerst meldet, gewinnt.
const ZUSATZ_CODES = new Set([16, 17, 18, 91, 92]);

function tasteImFenster(ev: KeyboardEvent) {
  if (!lauschend) return;
  // Nichts davon soll die Seite erreichen (Leertaste klickt sonst das Feld, F5 lädt neu).
  ev.preventDefault();
  ev.stopPropagation();
  if (ev.repeat || ZUSATZ_CODES.has(ev.keyCode)) return;
  if (ev.key === "Escape" && !ev.ctrlKey && !ev.altKey && !ev.shiftKey) return lernenBeenden(null);
  // AltGr meldet Strg+Alt: so bleibt es auch im Hook.
  const mods = (ev.ctrlKey ? 1 : 0) | (ev.altKey ? 2 : 0) | (ev.shiftKey ? 4 : 0) | (ev.metaKey ? 8 : 0);
  void api.tasteBenennen(ev.keyCode, mods).then(
    (name) => lernenBeenden(name),
    () => {},
  );
}

function mausImFenster(ev: MouseEvent) {
  if (!lauschend) return;
  const name = ev.button === 1 ? "Maus3" : ev.button === 3 ? "Maus4" : ev.button === 4 ? "Maus5" : null;
  if (!name) return;
  ev.preventDefault();
  ev.stopPropagation();
  const mods = [ev.ctrlKey ? "Strg" : "", ev.altKey ? "Alt" : "", ev.shiftKey ? "Umschalt" : "", ev.metaKey ? "Win" : ""].filter(Boolean);
  lernenBeenden([...mods, name].join("+"));
}

/** Vom Motor gemeldete Taste (Lern-Modus). */
export function tasteEmpfangen(taste: string) {
  lernenBeenden(taste);
}

function tastenKappen(taste: string) {
  const s = h("span", { class: "zahl" });
  taste.split(/\+(?=.)/).forEach((teil, i) => {
    if (i > 0) s.append(" + ");
    s.append(h("span", { class: "taste-kappe", text: teil }));
  });
  return s;
}

export function hotkeyFeld(o: { wert: string; beiAenderung: (taste: string) => void }) {
  const feld = h("button", { class: "hotkey", type: "button", title: t("hk.feld") });
  let wert = o.wert;
  const zeichnen = () => {
    feld.classList.toggle("lauscht", lauschend?.feld === feld);
    feld.classList.toggle("leer", !wert);
    if (lauschend?.feld === feld) feld.replaceChildren(symbol("tastatur", true), t("hk.drueck"));
    else feld.replaceChildren(symbol("tastatur"), wert ? tastenKappen(wert) : t("hk.feld"));
  };
  feld.addEventListener("click", () => {
    if (lauschend?.feld === feld) return lernenBeenden(null);
    lernenBeenden(null);
    lauschend = {
      feld,
      fertig: (taste) => {
        if (taste) {
          wert = taste;
          o.beiAenderung(taste);
        }
        zeichnen();
      },
    };
    void api.lernen(true);
    addEventListener("keydown", tasteImFenster, true);
    addEventListener("mousedown", mausImFenster, true);
    lernZeit = setTimeout(() => lernenBeenden(null), 10_000);
    zeichnen();
  });
  zeichnen();
  return feld;
}

// ---------- Pegel mit Empfindlichkeits-Linie ----------

export interface PegelElement extends HTMLDivElement {
  pegel(db: number | null): void;
}

const PEGEL_MIN = -80;
const PEGEL_MAX = 0;

export function pegelAnzeige(o: { schwelle: number | null; titel: string; beiAenderung?: (db: number) => void }): PegelElement {
  const fuellung = h("div", { class: "pegel-fuellung" });
  const linie = o.schwelle === null ? null : h("div", { class: "pegel-linie" });
  const bahn = h("div", { class: "pegel", role: o.beiAenderung ? "slider" : "meter", tabindex: o.beiAenderung ? 0 : -1, "aria-label": o.titel }, h("div", { class: "pegel-spur" }, fuellung), linie);
  const el = h(
    "div",
    { class: "pegel-block" },
    bahn,
    h("div", { class: "pegel-skala text-s zahl" }, ...[-80, -60, -40, -20, 0].map((d) => h("span", { text: `${d}` }))),
  ) as PegelElement;
  let schwelle = o.schwelle;
  const anteil = (db: number) => (Math.min(PEGEL_MAX, Math.max(PEGEL_MIN, db)) - PEGEL_MIN) / (PEGEL_MAX - PEGEL_MIN);
  const linieZeichnen = () => {
    if (linie && schwelle !== null) {
      linie.style.left = `${anteil(schwelle) * 100}%`;
      bahn.setAttribute("aria-valuenow", String(schwelle));
      bahn.setAttribute("aria-valuetext", `${schwelle} dB`);
    }
  };
  const setzen = (db: number) => {
    schwelle = Math.round(Math.min(-20, Math.max(-75, db)));
    linieZeichnen();
    o.beiAenderung?.(schwelle);
  };
  if (o.beiAenderung) {
    const ausX = (x: number) => {
      const r = bahn.getBoundingClientRect();
      setzen(PEGEL_MIN + ((x - r.left) / r.width) * (PEGEL_MAX - PEGEL_MIN));
    };
    bahn.addEventListener("pointerdown", (ev) => {
      bahn.setPointerCapture(ev.pointerId);
      ausX(ev.clientX);
    });
    bahn.addEventListener("pointermove", (ev) => bahn.hasPointerCapture(ev.pointerId) && ausX(ev.clientX));
    bahn.addEventListener("keydown", (ev) => {
      if (schwelle === null) return;
      if (ev.key === "ArrowLeft" || ev.key === "ArrowDown") setzen(schwelle - 1);
      else if (ev.key === "ArrowRight" || ev.key === "ArrowUp") setzen(schwelle + 1);
      else return;
      ev.preventDefault();
    });
  }
  el.pegel = (db) => {
    const p = db === null ? 0 : anteil(db);
    fuellung.style.setProperty("--p", String(p));
    fuellung.classList.toggle("offen", db !== null && (schwelle === null || db >= schwelle));
  };
  linieZeichnen();
  return el;
}
