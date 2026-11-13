// Hotkeys: globale Belegungen und Profile pro Spiel.

import { api } from "../api";
import { t } from "../i18n";
import { symbol } from "../icons";
import { auswahl, flaeche, hotkeyFeld, knopf, symbolknopf, texteingabe } from "../ui/bausteine";
import { h } from "../ui/dom";
import { PROFIL_AUS, UMGEBUNGEN, type Aktion, type Belegung, type Einstellungen, type Umgebung } from "../typen";
import { umgebungName } from "./umgebungen";
import { aendern, z } from "../zustand";

/** Aktion ↔ Wert im Auswahlfeld. */
function aktionWert(a: Aktion): string {
  if (typeof a === "string") return a;
  if ("Sound" in a) return `S:${a.Sound}`;
  if ("Figur" in a) return `F:${a.Figur}`;
  if ("Umgebung" in a) return `U:${a.Umgebung}`;
  return `P:${a.Profil}`;
}
function wertAktion(w: string): Aktion {
  const [art, ...rest] = w.split(":");
  const name = rest.join(":");
  if (art === "S") return { Sound: name };
  if (art === "F") return { Figur: name };
  if (art === "P") return { Profil: name };
  if (art === "U") return { Umgebung: name as Umgebung };
  return w as Aktion;
}

function aktionOptionen(global: boolean, aktuell: Aktion) {
  const o: { wert: string; text: string }[] = [
    { wert: "AllesStopp", text: t("akt.allesStopp") },
    { wert: "StummUmschalten", text: t("akt.stumm") },
    { wert: "AufnahmeUmschalten", text: t("akt.aufnahme") },
  ];
  if (global) {
    o.push({ wert: "ProfilAutomatisch", text: t("akt.automatisch") });
    for (const p of [...z.e.hotkeys.profile.map((p) => p.name), PROFIL_AUS]) o.push({ wert: `P:${p}`, text: `${t("akt.profil")}: ${p}` });
  } else {
    for (const s of z.e.sounds) o.push({ wert: `S:${s.name}`, text: `${t("akt.sound")}: ${s.name}` });
    for (const f of z.e.figuren) o.push({ wert: `F:${f.name}`, text: `${t("akt.figur")}: ${f.name}` });
    o.push({ wert: "FigurAus", text: t("akt.figurAus") });
    for (const u of UMGEBUNGEN) o.push({ wert: `U:${u}`, text: `${t("akt.umgebung")}: ${umgebungName(u)}` });
    o.push({ wert: "UmgebungAus", text: t("akt.umgebungAus") });
  }
  // Verweis auf etwas, das es nicht mehr gibt: trotzdem zeigen, damit nichts still verschwindet.
  const w = aktionWert(aktuell);
  if (!o.some((x) => x.wert === w)) o.unshift({ wert: w, text: `${w.slice(2) || w} (?)` });
  return o;
}

function belegungsListe(liste: () => Belegung[], global: boolean, neuZeichnen: () => void, pruefen: () => void) {
  const zeilen = h("div", { class: "belegungen" });
  liste().forEach((b, i) => {
    zeilen.append(
      h(
        "div",
        { class: "belegung" },
        hotkeyFeld({
          wert: b.taste,
          beiAenderung: (taste) => {
            aendern(() => (liste()[i].taste = taste));
            pruefen();
          },
        }),
        auswahl({
          titel: t("akt.sound"),
          optionen: aktionOptionen(global, b.aktion),
          wert: aktionWert(b.aktion),
          beiAenderung: (w) => {
            aendern(() => (liste()[i].aktion = wertAktion(w)));
            pruefen();
          },
        }),
        symbolknopf({
          symbol: "loeschen",
          titel: t("fig.loeschen"),
          beiKlick: () => {
            aendern(() => liste().splice(i, 1));
            neuZeichnen();
            pruefen();
          },
        }),
      ),
    );
  });
  if (!liste().length && !global) zeilen.append(h("p", { class: "text-m leise", text: t("hk.leer") }));

  // Neuer Hotkey: erst die Taste lernen, dann eintragen. Ein leerer Hotkey käme beim Motor nicht an.
  const neuZeile = h("div", { class: "belegung neu" });
  const neu = knopf({
    text: t("hk.neuBelegung"),
    symbol: "plus",
    art: "tonal",
    beiKlick: () => {
      const feld = hotkeyFeld({
        wert: "",
        beiAenderung: (taste) => {
          const start: Aktion = global ? "AllesStopp" : z.e.sounds[0] ? { Sound: z.e.sounds[0].name } : "FigurAus";
          aendern(() => liste().push({ taste, aktion: start }));
          neuZeichnen();
          pruefen();
        },
      });
      neuZeile.replaceChildren(feld, knopf({ text: t("hk.abbrechen"), beiKlick: () => neuZeichnen() }));
      feld.click();
    },
  });
  neuZeile.append(neu);
  zeilen.append(neuZeile);
  return zeilen;
}

function profilVerweiseAendern(e: Einstellungen, alt: string, neu: string | null) {
  const alle = [...e.hotkeys.global, ...e.hotkeys.profile.flatMap((p) => p.belegungen)];
  for (const b of alle) if (typeof b.aktion === "object" && "Profil" in b.aktion && b.aktion.Profil === alt && neu) b.aktion = { Profil: neu };
  if (!neu) e.hotkeys.global = e.hotkeys.global.filter((b) => !(typeof b.aktion === "object" && "Profil" in b.aktion && b.aktion.Profil === alt));
  if (e.hotkeys.ohne_spiel === alt) {
    if (neu) e.hotkeys.ohne_spiel = neu;
    else delete e.hotkeys.ohne_spiel;
  }
}

export function hotkeysSeite(): HTMLElement {
  let gewaehlt = 0;
  const warnungen = h("section", { class: "flaeche warnungen", hidden: true });
  const pruefen = async () => {
    const w = await api.hotkeysPruefen(z.e);
    warnungen.hidden = !w.length;
    warnungen.replaceChildren(
      h("div", { class: "flaeche-kopf" }, h("div", {}, symbol("warnung", true), h("h2", { class: "titel-m", text: t("hk.warnungen") }))),
      ...w.map((x) => h("p", { class: "text-m", text: x })),
    );
  };

  const globalBlock = h("div");
  const profilBlock = h("div");

  const globalZeichnen = () => {
    globalBlock.replaceChildren(
      flaeche(t("hk.global"), "stopp_hand", h("p", { class: "text-m leise", text: t("hk.globalText") }), belegungsListe(() => z.e.hotkeys.global, true, globalZeichnen, pruefen)),
    );
  };

  const profilZeichnen = () => {
    const profile = z.e.hotkeys.profile;
    gewaehlt = Math.min(gewaehlt, Math.max(0, profile.length - 1));
    const reiter = h(
      "div",
      { class: "reiter", role: "tablist" },
      ...profile.map((p, i) =>
        h(
          "button",
          {
            class: `chip ebene${i === gewaehlt ? " gewaehlt" : ""}`,
            type: "button",
            role: "tab",
            "aria-selected": String(i === gewaehlt),
            onclick: () => {
              gewaehlt = i;
              profilZeichnen();
            },
          },
          symbol("spiel", i === gewaehlt),
          p.name,
        ),
      ),
      knopf({
        text: t("hk.neuProfil"),
        symbol: "plus",
        beiKlick: () => {
          let name = t("hk.neuProfil");
          for (let n = 2; z.e.hotkeys.profile.some((p) => p.name === name); n++) name = `${t("hk.neuProfil")} ${n}`;
          aendern((e) => e.hotkeys.profile.push({ name, programme: [], belegungen: [] }));
          gewaehlt = z.e.hotkeys.profile.length - 1;
          profilZeichnen();
          globalZeichnen();
        },
      }),
    );
    const p = profile[gewaehlt];
    if (!p) {
      profilBlock.replaceChildren(reiter, h("p", { class: "text-m leise", text: t("hk.aus") }));
      return;
    }
    const programmEingabe = texteingabe({
      titel: t("hk.programmHinzu"),
      wert: "",
      platzhalter: t("hk.programmNeu"),
      beiAenderung: (w) => {
        const x = w.trim();
        if (!x) return;
        aendern(() => p.programme.push(x));
        profilZeichnen();
        pruefen();
      },
    });
    const programme = h(
      "div",
      { class: "chips" },
      ...p.programme.map((prog, i) =>
        h(
          "button",
          {
            class: "chip ebene",
            type: "button",
            title: t("fig.loeschen"),
            onclick: () => {
              aendern(() => p.programme.splice(i, 1));
              profilZeichnen();
            },
          },
          symbol("spiel"),
          prog,
          symbol("schliessen"),
        ),
      ),
    );
    profilBlock.replaceChildren(
      reiter,
      h(
        "section",
        { class: "flaeche" },
        h(
          "div",
          { class: "profil-kopf" },
          texteingabe({
            titel: t("hk.profilName"),
            wert: p.name,
            beiAenderung: (w) => {
              const neu = w.trim();
              if (!neu || neu.toLowerCase() === PROFIL_AUS.toLowerCase() || profile.some((x, j) => j !== gewaehlt && x.name === neu)) return profilZeichnen();
              aendern((e) => {
                profilVerweiseAendern(e, p.name, neu);
                p.name = neu;
              });
              profilZeichnen();
              globalZeichnen();
            },
          }),
          knopf({
            text: t("hk.profilLoeschen"),
            symbol: "loeschen",
            art: "umriss",
            beiKlick: () => {
              aendern((e) => {
                profilVerweiseAendern(e, p.name, null);
                e.hotkeys.profile.splice(gewaehlt, 1);
              });
              profilZeichnen();
              globalZeichnen();
              pruefen();
            },
          }),
        ),
        h("p", { class: "text-m leise", text: t("hk.profilText") }),
        programme,
        programmEingabe,
        h("div", { class: "trenner" }),
        belegungsListe(() => p.belegungen, false, profilZeichnen, pruefen),
      ),
      h("p", { class: "text-m leise", text: t("hk.aus") }),
      flaeche(
        t("hk.ohneSpiel"),
        "uebersicht",
        auswahl({
          titel: t("hk.ohneSpiel"),
          optionen: [{ wert: "", text: t("hk.ohneSpielAus") }, ...profile.map((x) => ({ wert: x.name, text: x.name }))],
          wert: z.e.hotkeys.ohne_spiel ?? "",
          hilfe: t("hk.ohneSpielText"),
          beiAenderung: (w) =>
            aendern((e) => {
              if (w) e.hotkeys.ohne_spiel = w;
              else delete e.hotkeys.ohne_spiel;
            }),
        }),
      ),
    );
  };

  globalZeichnen();
  profilZeichnen();
  void pruefen();

  return h(
    "div",
    { class: "seite" },
    h("div", { class: "seiten-kopf" }, h("h1", { class: "titel-xl", text: t("hk.titel") })),
    warnungen,
    globalBlock,
    profilBlock,
  );
}
