// Lizenzen der fremden Bausteine (Seitenblatt aus „Einstellungen, Version“).
// Die Liste baut scripts/lizenzen.mjs vor jedem Build. Sie lädt erst beim Öffnen.

import { t } from "../i18n";
import { melden, symbolknopf } from "../ui/bausteine";
import { h } from "../ui/dom";

type Paket = { name: string; version: string; lizenz: string; lizenzUrl?: string; urheber: string; quelle: string; art: string; texte: number[] };

export async function lizenzenZeigen() {
  let daten: { pakete: Paket[]; texte: string[] };
  try {
    daten = (await import("../lizenzen.json")).default;
  } catch (f) {
    melden(String(f));
    return;
  }
  const schliessen = () => {
    schleier.remove();
    blatt.remove();
    removeEventListener("keydown", taste);
  };
  const taste = (ev: KeyboardEvent) => ev.key === "Escape" && schliessen();
  addEventListener("keydown", taste);
  const schleier = h("div", { class: "schleier", onclick: schliessen });

  const zeile = (p: Paket) => {
    const kopf = h(
      "summary",
      {},
      h("span", { class: "lizenz-name", text: p.version ? `${p.name} ${p.version}` : p.name }),
      h("span", { class: "text-s leise", text: p.lizenz }),
    );
    const eintrag = h("details", { class: "lizenz-eintrag" }, kopf);
    // Texte erst beim Aufklappen einsetzen: über 400 Einträge, viele lange Texte.
    eintrag.addEventListener(
      "toggle",
      () => {
        eintrag.append(
          p.urheber ? h("p", { class: "text-s leise", text: p.urheber }) : "",
          h("p", { class: "text-s leise", text: p.quelle }),
          ...(p.lizenzUrl ? [h("p", { class: "text-s leise", text: p.lizenzUrl })] : []),
          ...p.texte.map((i) => h("pre", { class: "lizenz-text", text: daten.texte[i] })),
        );
      },
      { once: true },
    );
    return eintrag;
  };

  const blatt = h(
    "aside",
    { class: "seitenblatt breit", role: "dialog", "aria-label": t("liz.titel") },
    h("div", { class: "flaeche-kopf" }, h("h2", { class: "titel-m", text: t("liz.titel") }), symbolknopf({ symbol: "schliessen", titel: t("snd.schliessen"), beiKlick: schliessen })),
    h("p", { class: "text-m leise", text: t("liz.text").replace("{n}", String(daten.pakete.length)) }),
    h("div", { class: "lizenz-liste" }, ...daten.pakete.map(zeile)),
  );
  document.body.append(schleier, blatt);
}
