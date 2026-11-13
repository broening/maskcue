// Übersicht: Stimme wählen, Mikrofon-Pegel, Ausgänge, laufende Sounds, Profil.

import { api } from "../api";
import { t } from "../i18n";
import { symbol } from "../icons";
import { flaeche, knopf, pegelAnzeige } from "../ui/bausteine";
import { h } from "../ui/dom";
import { bildVon, figurKachel, type KachelElement } from "../ui/formen";
import { PROFIL_AUS, UMGEBUNGEN } from "../typen";
import { umgebungName, umgebungTaste } from "./umgebungen";
import { vorzeichenFein } from "./figuren";
import { aufStatus, z } from "../zustand";

function vorzeichen(v: number) {
  return v > 0 ? `+${v}` : `${v}`;
}

export function uebersichtSeite(): HTMLElement {
  // --- Fehler (z. B. Gerät fehlt) ---
  const fehlerText = h("span", { class: "text-l" });
  const fehler = h(
    "div",
    { class: "banner", hidden: true },
    symbol("warnung", true),
    fehlerText,
    knopf({ text: t("nav.mikrofon"), symbol: "mikrofon", art: "tonal", beiKlick: () => dispatchEvent(new CustomEvent("glottix-seite", { detail: "mikrofon" })) }),
    knopf({ text: t("status.neustart"), symbol: "neustart", art: "tonal", beiKlick: () => void api.neustart() }),
  );

  // --- Stimme ---
  const kacheln = new Map<string | null, KachelElement>();
  const normal = figurKachel({ platz: 0, name: t("ueb.normal"), unterzeile: t("ueb.normalText"), bild: bildVon(0, z.e.normal_bild, t("ueb.normal"), z.e.normal_ausschnitt), aktiv: true, beiKlick: () => void api.figur(null) });
  kacheln.set(null, normal);
  z.e.figuren.forEach((f, i) => {
    const k = figurKachel({
      platz: i + 1,
      name: f.name,
      // Tonhöhe wie auf der Seite Figuren (2 Nachkommastellen), Formant ganzzahlig.
      unterzeile: `${vorzeichenFein(f.halbtoene)} · ${vorzeichen(f.formant)}`,
      bild: bildVon(i + 1, f.bild, f.name, f.bild_ausschnitt),
      aktiv: false,
      beiKlick: () => void api.figur(f.name),
    });
    kacheln.set(f.name, k);
  });
  const kachelListe = [...kacheln.values()];
  kachelListe.forEach((k, i) => k.style.setProperty("--i", String(i)));
  const stimme = h("section", { class: "stimme" }, h("h1", { class: "titel-xl", text: t("ueb.stimme") }), h("div", { class: "kacheln auftritt" }, ...kachelListe));

  // --- Umgebung (Klick = an/aus wie der Hotkey) ---
  const umgebungChips = h("div", { class: "chips" });
  const umgebung = h("section", { class: "umgebung-wahl" }, h("h2", { class: "titel-m", text: t("umg.ueberschrift") }), umgebungChips);
  let umgebungStand = "";

  // --- Mikrofon ---
  const pegel = pegelAnzeige({ schwelle: z.e.mikro.gate_spiel.an ? z.e.mikro.gate_spiel.schwelle_db : null, titel: t("ueb.pegel") });
  const mikro = flaeche(t("ueb.pegel"), "mikrofon", h("p", { class: "text-m leise", text: z.e.geraete.eingang || t("mik.waehlen") }), pegel);

  // --- Ausgänge ---
  const ausgaengeListe = h("div", { class: "ausgaenge" });
  const ausgaenge = flaeche(t("ueb.ausgaenge"), "laut", ausgaengeListe);

  // --- Läuft gerade ---
  const laufenListe = h("div", { class: "chips" });
  const laufen = flaeche(t("ueb.laufen"), "musik", laufenListe);

  // --- Profil ---
  const profilNamen = [...z.e.hotkeys.profile.map((p) => p.name), PROFIL_AUS];
  const profilText = h("p", { class: "text-m leise" });
  const profilWahl = h("div", { class: "chips" });
  let profilZustand = "";
  const profil = flaeche(t("status.profil"), "spiel", profilText, profilWahl);

  // Hotkey-Abzeichen der Figuren: aus dem gerade aktiven Profil.
  let tastenProfil: string | null = null;
  const tastenZeigen = (profil: string) => {
    if (profil === tastenProfil) return;
    tastenProfil = profil;
    const belegungen = z.e.hotkeys.profile.find((p) => p.name === profil)?.belegungen ?? [];
    for (const [name, k] of kacheln) {
      const b = belegungen.find((x) => (name === null ? x.aktion === "FigurAus" : typeof x.aktion === "object" && "Figur" in x.aktion && x.aktion.Figur === name));
      k.taste(b ? b.taste : null);
    }
  };

  aufStatus((s) => {
    tastenZeigen(s.profil);
    fehler.hidden = !s.fehler;
    fehlerText.textContent = s.fehler ?? "";
    for (const [name, k] of kacheln) k.aktiv(name === s.figur);

    const umgebungNeu = `${s.umgebung}|${s.umgebung_hotkey}|${s.profil}`;
    if (umgebungNeu !== umgebungStand) {
      umgebungStand = umgebungNeu;
      const chip = (an: boolean, text: string, sym: Node, taste: string | null, beiKlick: () => void) =>
        h("button", { class: `chip${an ? " gewaehlt" : ""}`, type: "button", "aria-pressed": String(an), title: taste ? `${text} (${taste})` : text, onclick: beiKlick }, sym, text, taste ? h("span", { class: "leise", text: ` ${taste}` }) : null);
      umgebungChips.replaceChildren(
        chip(s.umgebung === null, t("umg.keine"), symbol(s.umgebung === null ? "haken" : "umgebung", true), umgebungTaste(s.profil, null), () => void api.umgebung(null)),
        ...UMGEBUNGEN.map((u) => {
          const an = s.umgebung === u;
          const text = an && !s.umgebung_hotkey ? `${umgebungName(u)} · ${t("umg.vonFigur")}` : umgebungName(u);
          return chip(an, text, symbol(u, an), umgebungTaste(s.profil, u), () => void api.umgebung(u));
        }),
      );
    }
    pegel.pegel(s.pegel_db);

    ausgaengeListe.replaceChildren(
      ...s.ausgaenge.map((a, i) =>
        h(
          "div",
          { class: "ausgang" },
          h("span", { class: "etikett leise ausgang-name" }, symbol(i === 0 ? "spiel" : "chat"), i === 0 ? t("ausgang.spiel") : t("ausgang.chat")),
          h("span", { class: "ausgang-zahl zahl", text: a.latenz_ms === null ? "–" : `${Math.round(a.latenz_ms)}` }, h("span", { class: "titel-s leise", text: ` ${t("allg.ms")}` })),
          // Balken: 0–120 ms, grün bis 80 ms (Ziel aus PLAN.md), darüber gelb.
          h("span", { class: `ausgang-balken${(a.latenz_ms ?? 0) > 80 ? " hoch" : ""}`, style: `--p:${Math.min(1, (a.latenz_ms ?? 0) / 120)}` }),
          h("span", { class: `text-s zahl ${a.aussetzer > 0 ? "warn-text" : "leise"}`, text: `${a.aussetzer} ${t("status.aussetzer")}` }),
        ),
      ),
    );

    laufenListe.replaceChildren(
      ...(s.sounds_laufen.length
        ? s.sounds_laufen.map((n) => h("button", { class: "chip gewaehlt", type: "button", title: t("snd.nochmal.stopp"), onclick: () => void api.ausloesen(n) }, h("span", { class: "eq", "aria-hidden": "true" }, h("i"), h("i"), h("i")), n, symbol("stopp", true)))
        : [h("p", { class: "text-m leise", text: t("ueb.nichtsLaeuft") })]),
    );

    const neu = `${s.profil}|${s.profil_von_hand}|${s.programm}`;
    if (neu !== profilZustand) {
      profilZustand = neu;
      profilText.textContent = `${t("status.programm")}: ${s.programm || "–"}`;
      // Chips statt Gruppe: brechen bei vielen Profilen sauber um.
      const gewaehlt = s.profil_von_hand ? s.profil : "";
      profilWahl.replaceChildren(
        ...[{ wert: "", text: t("hk.automatisch") }, ...profilNamen.map((n) => ({ wert: n, text: n }))].map((o) =>
          h(
            "button",
            { class: `chip${o.wert === gewaehlt ? " gewaehlt" : ""}`, type: "button", "aria-pressed": String(o.wert === gewaehlt), onclick: () => void api.profil(o.wert || null) },
            o.wert === gewaehlt ? symbol("haken", true) : o.wert ? symbol("spiel") : symbol("sync"),
            o.text,
          ),
        ),
      );
    }
  });

  const karten = [mikro, ausgaenge, laufen, profil];
  karten.forEach((k, i) => k.style.setProperty("--i", String(i + kachelListe.length)));
  return h("div", { class: "seite" }, fehler, stimme, umgebung, h("div", { class: "raster-uebersicht auftritt" }, ...karten));
}
