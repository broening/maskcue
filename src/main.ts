import { updateAnmelden } from "./update";
import "@fontsource-variable/fredoka";
import "@fontsource-variable/figtree";
import "./stil/tokens.css";
import "./stil/basis.css";
import "./stil/bausteine.css";
import "./stil/seiten.css";
import "./stil/overlay.css";

import { api, istOverlay, mitTauri } from "./api";
import { overlayStarten } from "./overlay";
import { bilderOrdnerSetzen } from "./avatare";
import { spracheSetzen, t, type Schluessel } from "./i18n";
import { symbol, type SymbolName } from "./icons";
import logo from "./assets/logo.svg?url";
import { einstellungenSeite } from "./seiten/einstellungen";
import { figurenSeite } from "./seiten/figuren";
import { hotkeysSeite } from "./seiten/hotkeys";
import { mikrofonSeite } from "./seiten/mikrofon";
import { soundsSeite } from "./seiten/sounds";
import { uebersichtSeite } from "./seiten/uebersicht";
import { umgebungenSeite, umgebungName } from "./seiten/umgebungen";
import { aufnahmeFertigAnmelden, aufnahmenSeite, dauer } from "./seiten/aufnahmen";
import { melden, tasteEmpfangen } from "./ui/bausteine";
import { h } from "./ui/dom";
import { aufStatus, seitenHoererLeeren, statusVerteilen, z } from "./zustand";
import { PROFIL_AUS, type Status } from "./typen";

type SeitenName = "uebersicht" | "mikrofon" | "figuren" | "umgebungen" | "sounds" | "aufnahmen" | "hotkeys" | "einstellungen";

const SEITEN: { name: SeitenName; symbol: SymbolName; bau: () => HTMLElement }[] = [
  { name: "uebersicht", symbol: "uebersicht", bau: uebersichtSeite },
  { name: "mikrofon", symbol: "mikrofon", bau: mikrofonSeite },
  { name: "figuren", symbol: "figur", bau: figurenSeite },
  { name: "umgebungen", symbol: "umgebung", bau: umgebungenSeite },
  { name: "sounds", symbol: "sounds", bau: soundsSeite },
  { name: "aufnahmen", symbol: "aufnahmen", bau: aufnahmenSeite },
  { name: "hotkeys", symbol: "tastatur", bau: hotkeysSeite },
  { name: "einstellungen", symbol: "einstellungen", bau: einstellungenSeite },
];

let aktuelleSeite: SeitenName = "uebersicht";
const inhalt = h("main", { class: "inhalt" });
const leiste = h("nav", { class: "leiste", "aria-label": "MaskCue" });
const statuszeile = h("header", { class: "statuszeile" });

function leisteZeichnen() {
  const notbremse = h(
    "button",
    { class: "notbremse ebene", type: "button", title: `${t("notbremse")} (Pause)`, onclick: () => void api.allesStoppen() },
    symbol("stopp_hand", true),
  );
  leiste.replaceChildren(
    h("img", { class: "marke", src: logo, alt: "", "aria-hidden": "true" }),
    h("div", { class: "notbremse-block" }, notbremse, h("span", { class: "etikett", text: t("notbremse") })),
    h(
      "button",
      { class: `stumm-knopf${z.status?.stumm ? " an" : ""}`, type: "button", title: t("akt.stumm"), "aria-pressed": String(!!z.status?.stumm), onclick: () => void api.stumm(null) },
      symbol(z.status?.stumm ? "stumm" : "mikrofon", true),
      h("span", { class: "etikett", text: t("stumm") }),
    ),
    h(
      "div",
      { class: "ziele" },
      ...SEITEN.map((s) => {
        const an = s.name === aktuelleSeite;
        return h(
          "button",
          { class: `ziel${an ? " aktiv" : ""}`, type: "button", "aria-current": an ? "page" : undefined, onclick: () => zeigen(s.name) },
          h("span", { class: "ziel-pille ebene" }, symbol(s.symbol, an)),
          h("span", { class: "etikett", text: t(`nav.${s.name}` as Schluessel) }),
        );
      }),
    ),
  );
}

function statuszeileZeichnen(s: Status | null) {
  if (!s) return;
  const kinder: Node[] = [];
  kinder.push(
    h(
      "span",
      { class: `punkt${s.laeuft ? " an" : ""}`, title: s.laeuft ? t("status.laeuft") : t("status.aus") },
    ),
  );
  const profilAus = s.profil === PROFIL_AUS;
  kinder.push(
    h(
      "span",
      { class: `chip klein${profilAus ? "" : " gewaehlt"}`, title: `${t("status.programm")}: ${s.programm}` },
      symbol("spiel", !profilAus),
      `${t("status.profil")} ${s.profil}`,
      h("span", { class: "leise", text: ` · ${s.profil_von_hand ? t("status.vonHand") : t("status.automatisch")}` }),
    ),
  );
  kinder.push(h("span", { class: `chip klein${s.figur ? " gewaehlt" : ""}` }, symbol("figur", !!s.figur), s.figur ?? t("ueb.normal")));
  if (s.umgebung) kinder.push(h("span", { class: "chip klein gewaehlt" }, symbol(s.umgebung, true), umgebungName(s.umgebung)));
  s.ausgaenge.forEach((a, i) => {
    kinder.push(
      h(
        "span",
        { class: `chip klein zahl${a.aussetzer > 0 ? " warnung" : ""}`, title: `${a.name} · ${t("status.aussetzer")}: ${a.aussetzer}` },
        symbol(i === 0 ? "spiel" : "chat"),
        a.latenz_ms === null ? "–" : `${Math.round(a.latenz_ms)} ${t("allg.ms")}`,
      ),
    );
  });
  if (s.aufnahme_s !== null) kinder.push(h("span", { class: "chip klein warnung" }, symbol("aufnahme", true), `${t("auf.laeuft")} ${dauer(s.aufnahme_s)}`));
  if (s.stumm) kinder.push(h("span", { class: "chip klein warnung" }, symbol("stumm", true), t("stumm.an")));
  if (s.admin_problem) kinder.push(h("span", { class: "chip klein warnung", title: `${s.programm} ${t("status.admin")}` }, symbol("warnung", true), s.programm));
  statuszeile.replaceChildren(...kinder);
}

function zeigen(name: SeitenName) {
  aktuelleSeite = name;
  seitenHoererLeeren();
  leisteZeichnen();
  const s = SEITEN.find((x) => x.name === name) ?? SEITEN[0];
  inhalt.replaceChildren(statuszeile, s.bau());
  inhalt.scrollTop = 0;
}

// Plopp: Jeder Knopf federt nach dem Drücken kurz nach (Spiel-Gefühl). Über die
// Web-Animations-API, damit CSS-Animationen (Auftritt, Pochen) nicht neu starten.
const PLOPP = ".stumm-knopf .sym, .knopf, .symbolknopf, .segment, .kachel, .pad-knopf, .notbremse, button.chip, .ziel-pille, .figur-bild";
const ruhig = matchMedia("(prefers-reduced-motion: reduce)");
document.addEventListener("pointerdown", (ev) => {
  const el = (ev.target as Element).closest(PLOPP);
  if (!el || ruhig.matches) return;
  el.animate([{ scale: 1 }, { scale: 0.92, offset: 0.3 }, { scale: 1.05, offset: 0.65 }, { scale: 1 }], { duration: 420, easing: "ease-out" });
  if (el.classList.contains("notbremse")) {
    el.querySelector(".sym")?.animate([{ rotate: "0deg" }, { rotate: "-14deg" }, { rotate: "12deg" }, { rotate: "-6deg" }, { rotate: "0deg" }], { duration: 420 });
  }
});

document.addEventListener("animationend", (ev) => {
  if (ev.animationName === "aufsteigen") (ev.target as Element).classList.add("da");
});

/** Nachthimmel: feste Sterne (gleiche Lage bei jedem Start) und zwei Sternschnuppen. */
function himmel() {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("aria-hidden", "true");
  let x = 7;
  const zufall = () => (x = (x * 9301 + 49297) % 233280) / 233280;
  for (let i = 0; i < 70; i++) {
    const c = document.createElementNS(ns, "circle");
    c.setAttribute("cx", `${(zufall() * 100).toFixed(2)}%`);
    c.setAttribute("cy", `${(zufall() * 70).toFixed(2)}%`);
    c.setAttribute("r", (0.5 + zufall() * 1.1).toFixed(2));
    c.setAttribute("class", "stern");
    c.style.animationDelay = `${(-zufall() * 4).toFixed(2)}s`;
    svg.append(c);
  }
  return h(
    "div",
    { class: "himmel" },
    svg,
    h("span", { class: "schnuppe", style: "top:12%;left:58%" }),
    h("span", { class: "schnuppe", style: "top:30%;left:22%;animation-delay:-4.5s;animation-duration:13s" }),
  );
}

// Klick-Töne (Standard aus). Schalter klingen nach an/aus, alles andere nach Klick.
document.addEventListener("change", (ev) => {
  const el = ev.target as HTMLInputElement;
  if (z.e?.oberflaeche.ui_toene && el.matches(".schalter input")) void api.uiTon(el.checked ? "an" : "aus");
});
document.addEventListener("click", (ev) => {
  const el = ev.target as Element;
  if (z.e?.oberflaeche.ui_toene && el.closest("button")) void api.uiTon("klick");
});

// Seitenwechsel aus einer Seite heraus (z. B. Fehler-Hinweis → Mikrofon).
addEventListener("glottix-seite", (ev) => zeigen((ev as CustomEvent<SeitenName>).detail));

// Neu zeichnen, z. B. nach Sprachwechsel (Ereignis aus einer Seite).
addEventListener("glottix-neu", () => {
  spracheSetzen(z.e.oberflaeche.sprache);
  zeigen(aktuelleSeite);
  statuszeileZeichnen(z.status);
});

async function los() {
  const start = await api.start();
  z.e = start.einstellungen;
  z.geraete = start.geraete;
  z.version = start.version;
  bilderOrdnerSetzen(start.bilder_ordner);
  spracheSetzen(z.e.oberflaeche.sprache);
  document.body.append(h("div", { class: "app" }, himmel(), leiste, inhalt), updateAnmelden());
  api.aufStatus(statusVerteilen);
  api.aufTaste(tasteEmpfangen);
  // Ist MaskCue vorne, sieht der Hotkey-Wächter die Tastatur nicht sicher.
  // Darum gibt die Oberfläche Tasten weiter (nicht beim Tippen in Feldern).
  addEventListener("keydown", (ev) => {
    if (ev.repeat || [16, 17, 18, 91, 92].includes(ev.keyCode)) return;
    const ziel = ev.target as HTMLElement | null;
    if (ziel && (ziel.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(ziel.tagName))) return;
    const mods = (ev.ctrlKey ? 1 : 0) | (ev.altKey ? 2 : 0) | (ev.shiftKey ? 4 : 0) | (ev.metaKey ? 8 : 0);
    void api.tasteDruecken(ev.keyCode, mods).catch(() => {});
  });
  aufnahmeFertigAnmelden();
  api.aufHinweis((text) => melden(text));
  aufStatus(statuszeileZeichnen, true);
  // Stumm-Knopf in der Leiste folgt dem Status (auch bei Hotkey und Taskleiste).
  let warStumm = false;
  aufStatus((s) => {
    if (s.stumm !== warStumm) {
      warStumm = s.stumm;
      leisteZeichnen();
    }
  }, true);
  zeigen("uebersicht");
  if (!mitTauri) melden(t("ein.vorschau"));
}

void (istOverlay ? overlayStarten() : los());
