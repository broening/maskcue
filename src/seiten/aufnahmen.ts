// Aufnahmen: aufnehmen, was aus „Spiel“ kommt (Stimme mit Figur und Umgebung,
// dazu Sounds und Musik), anhören und als Sound ins Soundboard legen. So
// lassen sich RP-Sätze vorbereiten und später per Hotkey abspielen.

import { api } from "../api";
import { t } from "../i18n";
import { symbol } from "../icons";
import { flaeche, knopf, melden, pegelAnzeige, symbolknopf, texteingabe } from "../ui/bausteine";
import { h } from "../ui/dom";
import type { AufnahmeInfo } from "../typen";
import { aufStatus, ersetzen, z } from "../zustand";

/** Liste neu laden, wenn die Seite offen ist (nach „Aufnahme fertig“). */
let neuLaden: (() => void) | null = null;

/** Einmal beim Start anmelden: Meldung zeigen, auch auf anderen Seiten (Hotkey im Spiel). */
export function aufnahmeFertigAnmelden() {
  api.aufAufnahmeFertig((r) => {
    if (r.Ok) melden(`${t("auf.gespeichert")}: ${r.Ok}`);
    if (r.Err) melden(r.Err);
    neuLaden?.();
  });
}

export function dauer(s: number) {
  const m = Math.floor(s / 60);
  return `${m}:${String(Math.floor(s % 60)).padStart(2, "0")}`;
}

/** Hotkey für „Aufnahme an/aus“ (global oder im aktiven Profil). */
function aufnahmeTaste(profil: string | undefined): string | null {
  const liste = [...z.e.hotkeys.global, ...(z.e.hotkeys.profile.find((p) => p.name === profil)?.belegungen ?? [])];
  return liste.find((b) => b.aktion === "AufnahmeUmschalten")?.taste ?? null;
}

export function aufnahmenSeite(): HTMLElement {
  // --- Aufnehmen ---
  const uhr = h("span", { class: "aufnahme-uhr zahl", text: "0:00" });
  const knopfPlatz = h("div", { class: "knopf-reihe" });
  const tasteText = h("p", { class: "text-s leise" });
  const pegel = pegelAnzeige({ schwelle: null, titel: t("ueb.pegel") });
  const aufnehmen = flaeche(t("auf.aufnehmen"), "aufnahme", h("p", { class: "text-m leise", text: t("auf.text") }), h("div", { class: "aufnahme-zeile" }, knopfPlatz, uhr), pegel, tasteText);

  // --- Anhören auf: der Sound-Ausgang aus den Einstellungen (wie die Klick-Töne) ---
  const ausgang = z.e.oberflaeche.ui_toene_geraet;
  const anhoeren = flaeche(
    t("auf.anhoeren"),
    "hoeren",
    h("p", { class: "text-m", text: ausgang ? `${t("auf.geraet")}: ${ausgang}` : t("auf.keinAusgang") }),
    h("p", { class: "text-s leise", text: t("auf.geraetText") }),
    h(
      "div",
      { class: "knopf-reihe" },
      knopf({ text: t("auf.ausgangAendern"), symbol: "einstellungen", art: "umriss", beiKlick: () => dispatchEvent(new CustomEvent("glottix-seite", { detail: "einstellungen" })) }),
    ),
  );

  // --- Liste ---
  const liste = h("div", { class: "aufnahme-liste" });
  let abhoerenName: string | null = null;
  let abhoerenLaeuft = false;

  const zeichnenListe = (eintraege: AufnahmeInfo[]) => {
    if (!eintraege.length) {
      liste.replaceChildren(h("p", { class: "text-m leise", text: t("auf.leer") }));
      return;
    }
    liste.replaceChildren(
      ...eintraege.map((a) => {
        const imBoard = z.e.sounds.find((s) => s.datei.replace(/\\/g, "/").toLowerCase() === a.datei.replace(/\\/g, "/").toLowerCase());
        const spielt = abhoerenLaeuft && abhoerenName === a.name;
        return h(
          "div",
          { class: "aufnahme-eintrag flaeche" },
          symbolknopf({
            symbol: spielt ? "stopp" : "play",
            titel: spielt ? t("auf.stopp") : t("auf.anhoeren"),
            tonal: true,
            gewaehlt: spielt,
            beiKlick: () => {
              abhoerenName = spielt ? null : a.name;
              void api.aufnahmeAbhoeren(spielt ? null : a.name).catch((f) => melden(String(f)));
            },
          }),
          texteingabe({
            titel: t("fig.name"),
            wert: a.name,
            beiAenderung: async (neu) => {
              if (neu.trim() === a.name) return;
              try {
                ersetzen(await api.aufnahmeUmbenennen(a.name, neu));
              } catch (f) {
                melden(String(f));
              }
              void laden();
            },
          }),
          h("span", { class: "text-m zahl leise", text: `${dauer(a.sekunden)} · ${new Date(a.zeit * 1000).toLocaleString()}` }),
          imBoard
            ? h("span", { class: "chip klein gewaehlt", title: t("auf.imBoardText") }, symbol("sounds", true), imBoard.name)
            : knopf({
                text: t("auf.insBoard"),
                symbol: "sounds",
                art: "tonal",
                beiKlick: async () => {
                  try {
                    const [e, name] = await api.aufnahmeInsSoundboard(a.name);
                    ersetzen(e);
                    melden(`${t("auf.imBoard")}: ${name}. ${t("auf.imBoardText")}`, 7000);
                  } catch (f) {
                    melden(String(f));
                  }
                  void laden();
                },
              }),
          symbolknopf({
            symbol: "loeschen",
            titel: t("fig.loeschen"),
            beiKlick: async () => {
              if (!confirm(`${t("auf.loeschenFrage")} „${a.name}“`)) return;
              try {
                ersetzen(await api.aufnahmeLoeschen(a.name));
              } catch (f) {
                melden(String(f));
              }
              void laden();
            },
          }),
        );
      }),
    );
  };
  let letzte: AufnahmeInfo[] = [];
  const laden = async () => {
    letzte = await api.aufnahmenListe();
    zeichnenListe(letzte);
  };
  void laden();
  neuLaden = () => {
    if (liste.isConnected) void laden();
  };

  let stand = "";
  aufStatus((s) => {
    pegel.pegel(s.pegel_db);
    uhr.textContent = dauer(s.aufnahme_s ?? 0);
    uhr.classList.toggle("laeuft", s.aufnahme_s !== null);
    if (abhoerenLaeuft !== s.abhoeren) {
      abhoerenLaeuft = s.abhoeren;
      if (!s.abhoeren) abhoerenName = null;
      zeichnenListe(letzte);
    }
    const neu = `${s.aufnahme_s !== null}|${s.profil}|${s.laeuft}`;
    if (neu === stand) return;
    stand = neu;
    const laeuft = s.aufnahme_s !== null;
    knopfPlatz.replaceChildren(
      knopf({
        text: laeuft ? t("auf.stoppen") : t("auf.start"),
        symbol: laeuft ? "stopp" : "aufnahme",
        art: laeuft ? "fehler" : "gefuellt",
        gross: true,
        beiKlick: () => void api.aufnahme(!laeuft),
      }),
    );
    const k = aufnahmeTaste(s.profil);
    tasteText.textContent = k ? `${t("umg.hotkey")}: ${k}` : t("auf.keinHotkey");
  });

  return h(
    "div",
    { class: "seite" },
    h("div", { class: "seiten-kopf" }, h("div", {}, h("h1", { class: "titel-xl", text: t("auf.titel") }), h("p", { class: "text-l leise", text: t("auf.untertitel") }))),
    h("div", { class: "raster-2" }, aufnehmen, anhoeren),
    h("h2", { class: "titel-m", text: t("auf.liste") }),
    liste,
  );
}
