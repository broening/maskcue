// Mikrofon: Geräte, KI-Entrauschen, Empfindlichkeit (Gate), Klang.

import { t } from "../i18n";
import { auswahl, flaeche, knopf, pegelAnzeige, regler, schalter, segmente } from "../ui/bausteine";
import { symbol } from "../icons";
import { bildWahl } from "../ui/bildwahl";
import { bildVon } from "../ui/formen";
import { h } from "../ui/dom";
import type { DenoiseModell, DenoiseWahl } from "../typen";
import { aendern, aufStatus, z } from "../zustand";

const db = (v: number) => `${v > 0 ? "+" : ""}${v} ${t("allg.db")}`;

/** Gerät wählen. Fehlt das gespeicherte Gerät, bleibt es in der Liste und ist markiert. */
function geraetWahl(titel: string, hilfe: string, liste: string[], wert: string, leerText: string | null, setzen: (w: string) => void) {
  const optionen = liste.map((n) => ({ wert: n, text: n }));
  // Gespeichert ist oft nur der Anfang ("MaskCue"). Dann den vollen Namen zeigen.
  const passend = liste.find((n) => n === wert) ?? liste.find((n) => n.toLowerCase().startsWith(`${wert.toLowerCase()} (`));
  let fehler: string | undefined;
  if (wert && !passend) {
    optionen.unshift({ wert, text: `${wert} – ${t("mik.fehlt")}` });
    fehler = `${wert}: ${t("mik.fehlt")}`;
  }
  if (leerText !== null) optionen.unshift({ wert: "", text: leerText });
  else if (!wert) optionen.unshift({ wert: "", text: t("mik.waehlen") });
  return auswahl({ titel, hilfe, fehler, optionen, wert: passend ?? wert, beiAenderung: setzen });
}

function denoiseBlock(titel: string, wahl: () => DenoiseWahl) {
  const d = wahl();
  return h(
    "div",
    { class: "block" },
    h("span", { class: "etikett", text: titel }),
    segmente<DenoiseModell>({
      titel,
      optionen: [
        { wert: "aus", text: t("mik.modell.aus") },
        { wert: "leicht", text: t("mik.modell.leicht") },
        { wert: "normal", text: t("mik.modell.normal") },
      ],
      wert: d.modell,
      beiAenderung: (m) => aendern(() => (wahl().modell = m)),
    }),
    regler({
      titel: t("mik.staerke"),
      min: 10,
      max: 100,
      schritt: 5,
      wert: d.daempfung_db,
      format: (v) => (v >= 100 ? "max." : `${v} ${t("allg.db")}`),
      beiAenderung: (v) => aendern(() => (wahl().daempfung_db = v)),
    }),
  );
}

export function mikrofonSeite(): HTMLElement {
  const m = () => z.e.mikro;
  const g = z.geraete;

  const geraete = flaeche(
    t("mik.geraete"),
    "mikrofon",
    geraetWahl(t("mik.eingang"), "", g.eingaenge, z.e.geraete.eingang, null, (w) => aendern((e) => (e.geraete.eingang = w))),
    geraetWahl(t("mik.spiel"), t("mik.spielText"), g.ausgaenge, z.e.geraete.spiel, null, (w) => aendern((e) => (e.geraete.spiel = w))),
    geraetWahl(t("mik.chat"), t("mik.chatText"), g.ausgaenge, z.e.geraete.chat, t("mik.keinChat"), (w) => aendern((e) => (e.geraete.chat = w))),
  );

  const denoise = flaeche(
    t("mik.denoise"),
    "eq",
    h("p", { class: "text-m leise", text: t("mik.modellText") }),
    denoiseBlock(t("ausgang.spiel"), () => m().denoise_spiel),
    h("div", { class: "trenner" }),
    denoiseBlock(t("ausgang.chat"), () => m().denoise_chat),
  );

  const pegel = pegelAnzeige({
    schwelle: m().gate_spiel.schwelle_db,
    titel: t("mik.empfindlichkeit"),
    beiAenderung: (v) =>
      aendern(() => {
        m().gate_spiel.schwelle_db = v;
        m().gate_chat.schwelle_db = v;
      }),
  });
  aufStatus((s) => pegel.pegel(s.pegel_db));
  const empfindlichkeit = flaeche(
    t("mik.empfindlichkeit"),
    "hoeren",
    h("p", { class: "text-m leise", text: t("mik.empfindlichkeitText") }),
    pegel,
    schalter({ text: t("mik.gateSpiel"), wert: m().gate_spiel.an, beiAenderung: (an) => aendern(() => (m().gate_spiel.an = an)) }),
    schalter({ text: t("mik.gateChat"), hilfe: t("mik.gateChatText"), wert: m().gate_chat.an, beiAenderung: (an) => aendern(() => (m().gate_chat.an = an)) }),
  );

  const klang = flaeche(
    t("mik.klang"),
    "regler",
    regler({ titel: t("mik.boost"), min: 0, max: 18, schritt: 1, wert: m().kompressor.verstaerkung_db, format: db, beiAenderung: (v) => aendern(() => (m().kompressor.verstaerkung_db = v)) }),
    regler({ titel: t("mik.bass"), min: -12, max: 12, schritt: 1, mitte: true, wert: m().bass_db, format: db, beiAenderung: (v) => aendern(() => (m().bass_db = v)) }),
    regler({ titel: t("mik.hoehen"), min: -12, max: 12, schritt: 1, mitte: true, wert: m().hoehen_db, format: db, beiAenderung: (v) => aendern(() => (m().hoehen_db = v)) }),
    regler({
      titel: t("mik.hochpass"),
      min: 0,
      max: 200,
      schritt: 10,
      wert: m().hochpass_hz,
      format: (v) => (v === 0 ? t("allg.aus") : `${v} ${t("allg.hz")}`),
      beiAenderung: (v) => aendern(() => (m().hochpass_hz = v)),
    }),
    schalter({ text: t("mik.kompressor"), hilfe: t("mik.kompressorText"), wert: m().kompressor.an, beiAenderung: (an) => aendern(() => (m().kompressor.an = an)) }),
  );

  // Bild der normalen Stimme (Übersicht): wie bei den Figuren wählbar.
  const vorschau = h("button", { class: "figur-bild", type: "button", title: t("fig.bild") });
  const waehlen = () =>
    bildWahl(t("ueb.normal"), z.e.normal_bild, z.e.normal_ausschnitt, (bild, ausschnitt) => {
      aendern((e) => {
        e.normal_bild = bild;
        if (ausschnitt) e.normal_ausschnitt = ausschnitt;
        else delete e.normal_ausschnitt;
      });
      bildZeichnen();
    });
  const bildZeichnen = () => vorschau.replaceChildren(bildVon(0, z.e.normal_bild, t("ueb.normal"), z.e.normal_ausschnitt), h("span", { class: "figur-bild-stift" }, symbol("bearbeiten", true)));
  vorschau.addEventListener("click", waehlen);
  bildZeichnen();
  const bild = flaeche(
    t("mik.bild"),
    "stimme",
    h("div", { class: "bild-zeile" }, vorschau, h("div", { class: "block" }, h("p", { class: "text-m leise", text: t("mik.bildText") }), knopf({ text: t("fig.bild"), symbol: "bearbeiten", art: "tonal", beiKlick: waehlen }))),
  );

  return h(
    "div",
    { class: "seite" },
    h("div", { class: "seiten-kopf" }, h("h1", { class: "titel-xl", text: t("nav.mikrofon") })),
    h("div", { class: "raster-2" }, h("div", { class: "spalte" }, geraete, bild, empfindlichkeit), h("div", { class: "spalte" }, denoise, klang)),
  );
}
