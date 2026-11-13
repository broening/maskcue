// Umgebungen: Stimme in einen Raum oder ein Gerät setzen (Halle, Funkgerät, …).
// Ein Klick schaltet ein und aus (wie der Hotkey). Stärke je Umgebung.

import { api } from "../api";
import { t, type Schluessel } from "../i18n";
import { symbol } from "../icons";
import { knopf, regler } from "../ui/bausteine";
import { h } from "../ui/dom";
import { UMGEBUNGEN, umgebungStaerke, type Umgebung } from "../typen";
import { aendern, aufStatus, z } from "../zustand";

export const umgebungName = (u: Umgebung) => t(`umg.${u}` as Schluessel);

/** Hotkey für Umgebung `u` (`null` = „Umgebung aus“) im Profil `profil`. */
export function umgebungTaste(profil: string | undefined, u: Umgebung | null): string | null {
  const belegungen = z.e.hotkeys.profile.find((p) => p.name === profil)?.belegungen ?? [];
  const b = belegungen.find((x) => (u === null ? x.aktion === "UmgebungAus" : typeof x.aktion === "object" && "Umgebung" in x.aktion && x.aktion.Umgebung === u));
  return b?.taste ?? null;
}

export function umgebungenSeite(): HTMLElement {
  const teile = new Map<Umgebung, { karte: HTMLElement; knopfPlatz: HTMLElement; taste: HTMLElement }>();
  const raster = h("div", { class: "umgebung-raster auftritt" });
  UMGEBUNGEN.forEach((u, i) => {
    const knopfPlatz = h("div", { class: "knopf-reihe" });
    const taste = h("span", { class: "chip klein" });
    const karte = h(
      "section",
      { class: "flaeche umgebung-karte", style: `--i:${i}` },
      h("div", { class: "flaeche-kopf" }, h("div", {}, symbol(u), h("h2", { class: "titel-m", text: umgebungName(u) })), taste),
      h("p", { class: "text-m leise", text: t(`umg.${u}.text` as Schluessel) }),
      regler({
        titel: t("umg.staerke"),
        min: 0,
        max: 100,
        schritt: 5,
        wert: Math.round(umgebungStaerke(z.e, u) * 100),
        format: (v) => `${v} %`,
        hilfe: i === 0 ? t("umg.staerkeText") : undefined,
        beiAenderung: (v) => aendern((e) => (e.umgebungen = { ...e.umgebungen, [u]: v / 100 })),
      }),
      knopfPlatz,
    );
    teile.set(u, { karte, knopfPlatz, taste });
    raster.append(karte);
  });

  let stand = "";
  const zeichnen = (aktiv: Umgebung | null, vonFigur: boolean, profil: string | undefined) => {
    const neu = `${aktiv}|${vonFigur}|${profil}`;
    if (neu === stand) return;
    stand = neu;
    for (const [u, { karte, knopfPlatz, taste }] of teile) {
      const an = u === aktiv;
      karte.classList.toggle("aktiv", an);
      knopfPlatz.replaceChildren(
        knopf({
          text: an ? (vonFigur ? `${t("umg.an")} · ${t("umg.vonFigur")}` : t("umg.an")) : t("umg.einschalten"),
          symbol: u,
          art: an ? "spielen" : "gefuellt",
          beiKlick: () => void api.umgebung(u),
        }),
      );
      const k = umgebungTaste(profil, u);
      taste.hidden = !k;
      taste.textContent = k ? `${t("umg.hotkey")} ${k}` : "";
    }
  };
  aufStatus((s) => zeichnen(s.umgebung, !!s.umgebung && !s.umgebung_hotkey, s.profil));
  zeichnen(z.status?.umgebung ?? null, !!z.status?.umgebung && !z.status?.umgebung_hotkey, z.status?.profil);

  return h(
    "div",
    { class: "seite" },
    h(
      "div",
      { class: "seiten-kopf" },
      h("div", {}, h("h1", { class: "titel-xl", text: t("umg.titel") }), h("p", { class: "text-l leise", text: t("umg.text") })),
      h("div", { class: "knopf-reihe" }, knopf({ text: t("umg.aus"), symbol: "stopp", art: "umriss", gross: true, beiKlick: () => void api.umgebung(null) })),
    ),
    raster,
  );
}
