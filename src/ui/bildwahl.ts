// Bild wählen: fertige Avatare, eigenes Bild (mit Ausschnitt), eine Farbe
// oder die gemalte Szene. Für Figuren und für die normale Stimme.

import { api } from "../api";
import { avatare, bildUrl } from "../avatare";
import { t } from "../i18n";
import type { Ausschnitt } from "../typen";
import { knopf, melden, regler, symbolknopf } from "./bausteine";
import { h } from "./dom";
import { ausschnittStil } from "./formen";

/** Farben für „Ohne Bild“ (aus der Palette der App). */
export const BILD_FARBEN = ["#5b5ff5", "#8b5cf6", "#ec4899", "#ef4444", "#f97316", "#facc15", "#84cc16", "#22c55e", "#22d3ee", "#64748b"];

const MITTE: Ausschnitt = { x: 50, y: 50, zoom: 1 };

/** Seitenblatt zum Wählen des Bildes. `waehlen` bekommt das Bild und bei eigenen Bildern den Ausschnitt. */
export function bildWahl(figur: string, aktuell: string | undefined, ausschnitt: Ausschnitt | undefined, waehlen: (bild: string | undefined, ausschnitt?: Ausschnitt) => void) {
  const schliessen = () => {
    schleier.remove();
    blatt.remove();
    removeEventListener("keydown", taste);
  };
  const taste = (ev: KeyboardEvent) => ev.key === "Escape" && schliessen();
  addEventListener("keydown", taste);
  const nehmen = (bild: string | undefined, a?: Ausschnitt) => {
    waehlen(bild, a);
    schliessen();
  };
  const schleier = h("div", { class: "schleier", onclick: schliessen });
  const blatt = h("aside", { class: "seitenblatt breit", role: "dialog", "aria-label": `${t("fig.bild")}: ${figur}` });

  /** Ausschnitt eines eigenen Bildes: ziehen = verschieben, Regler = vergrößern. */
  const ausschnittZeigen = (bild: string, start: Ausschnitt) => {
    const url = bildUrl(bild);
    if (!url) return nehmen(bild);
    const a = { ...start };
    const img = h("img", { class: "foto ausschnitt-bild", src: url, alt: "", draggable: "false" });
    const zeichnen = () => img.setAttribute("style", ausschnittStil(a));
    zeichnen();
    const rahmen = h("div", { class: "ausschnitt-rahmen", title: t("fig.ausschnittZiehen") }, img);
    let ziehen: { px: number; py: number } | null = null;
    rahmen.addEventListener("pointerdown", (ev) => {
      ziehen = { px: ev.clientX, py: ev.clientY };
      rahmen.setPointerCapture(ev.pointerId);
    });
    rahmen.addEventListener("pointermove", (ev) => {
      if (!ziehen) return;
      const groesse = rahmen.clientWidth || 1;
      // Bild folgt der Maus: nach rechts ziehen zeigt mehr vom linken Rand.
      a.x = Math.min(100, Math.max(0, a.x - ((ev.clientX - ziehen.px) / groesse) * 100 / a.zoom));
      a.y = Math.min(100, Math.max(0, a.y - ((ev.clientY - ziehen.py) / groesse) * 100 / a.zoom));
      ziehen = { px: ev.clientX, py: ev.clientY };
      zeichnen();
    });
    const loslassen = () => void (ziehen = null);
    rahmen.addEventListener("pointerup", loslassen);
    rahmen.addEventListener("pointercancel", loslassen);
    blatt.replaceChildren(
      h("div", { class: "flaeche-kopf" }, h("h2", { class: "titel-m", text: t("fig.ausschnitt") }), symbolknopf({ symbol: "schliessen", titel: t("snd.schliessen"), beiKlick: schliessen })),
      h("p", { class: "text-m leise", text: t("fig.ausschnittText") }),
      rahmen,
      regler({
        titel: t("fig.zoom"),
        min: 100,
        max: 400,
        schritt: 5,
        wert: Math.round(a.zoom * 100),
        format: (v) => `${v} %`,
        beiAenderung: (v) => {
          a.zoom = v / 100;
          zeichnen();
        },
      }),
      h(
        "div",
        { class: "knopf-reihe" },
        knopf({ text: t("fig.uebernehmen"), symbol: "haken", art: "gefuellt", beiKlick: () => nehmen(bild, { x: Math.round(a.x * 10) / 10, y: Math.round(a.y * 10) / 10, zoom: a.zoom }) }),
        knopf({ text: t("fig.mitte"), symbol: "sync", art: "umriss", beiKlick: () => {
          Object.assign(a, MITTE);
          ausschnittZeigen(bild, a);
        } }),
      ),
    );
  };

  const raster = h(
    "div",
    { class: "avatar-raster" },
    ...avatare().map((av) => {
      const wert = `avatar:${av.id}`;
      return h(
        "button",
        { class: `avatar-wahl${aktuell === wert ? " gewaehlt" : ""}`, type: "button", title: av.name, onclick: () => nehmen(wert) },
        h("img", { src: av.url, alt: "", loading: "lazy", draggable: "false" }),
        h("span", { class: "etikett", text: av.name }),
      );
    }),
  );
  const farben = h(
    "div",
    { class: "farb-reihe", role: "radiogroup", "aria-label": t("fig.farbe") },
    ...BILD_FARBEN.map((f) => {
      const wert = `farbe:${f}`;
      return h("button", {
        class: `farb-wahl${aktuell === wert ? " gewaehlt" : ""}`,
        type: "button",
        title: f,
        "aria-label": f,
        style: `--farbe:${f}`,
        onclick: () => nehmen(wert),
      });
    }),
  );
  const eigenes = aktuell?.startsWith("datei:") ? aktuell : null;
  blatt.replaceChildren(
    h("div", { class: "flaeche-kopf" }, h("h2", { class: "titel-m", text: t("fig.bild") }), symbolknopf({ symbol: "schliessen", titel: t("snd.schliessen"), beiKlick: schliessen })),
    h("p", { class: "text-m leise", text: t("fig.bildText") }),
    h(
      "div",
      { class: "knopf-reihe" },
      knopf({
        text: t("fig.hochladen"),
        symbol: "ordner",
        art: "gefuellt",
        beiKlick: async () => {
          try {
            const bild = await api.bildHochladen();
            if (bild) ausschnittZeigen(bild, { ...MITTE });
          } catch (f) {
            melden(String(f));
          }
        },
      }),
      eigenes ? knopf({ text: t("fig.ausschnitt"), symbol: "bearbeiten", art: "tonal", beiKlick: () => ausschnittZeigen(eigenes, ausschnitt ?? { ...MITTE }) }) : null,
      knopf({ text: t("fig.ohneBild"), art: "umriss", beiKlick: () => nehmen(undefined) }),
    ),
    h("span", { class: "etikett leise", text: t("fig.farbe") }),
    farben,
    raster,
  );
  document.body.append(schleier, blatt);
}
