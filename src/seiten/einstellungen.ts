// Einstellungen in Reitern: Allgemein (Sprache, Sound-Ausgang, Overlay, Fehler melden, Version, Lizenzen)
// und Einrichtung (Liste zum Abhaken für Audio-Software und Programme).

import { api, mitTauri } from "../api";
import { fehlerMelden } from "../fehlerbericht";
import { t } from "../i18n";
import { auswahl, flaeche, knopf, melden, schalter, segmente } from "../ui/bausteine";
import { h } from "../ui/dom";
import { aendern, ersetzen, z } from "../zustand";
import { einrichtungTeil } from "./einrichtung";
import { lizenzenZeigen } from "./lizenzen";
import { updateZeigen } from "../update";

type Reiter = "allgemein" | "einrichtung" | "sichern";
/** Zuletzt offener Reiter (bleibt beim Seitenwechsel). */
let reiter: Reiter = "allgemein";

/** Updates: automatisch, nur benachrichtigen oder aus, dazu „Jetzt prüfen“. */
function updateFlaeche(): HTMLElement {
  const o = () => z.e.oberflaeche;
  const stand = h("p", { class: "text-s leise", role: "status" });
  return flaeche(
    t("upd.titel"),
    "sync",
    segmente<"auto" | "hinweis" | "aus">({
      titel: t("upd.titel"),
      optionen: [
        { wert: "auto", text: t("upd.auto") },
        { wert: "hinweis", text: t("upd.hinweis") },
        { wert: "aus", text: t("upd.aus") },
      ],
      wert: o().updates ?? "auto",
      beiAenderung: (w) => aendern(() => (o().updates = w)),
    }),
    h("p", { class: "text-s leise", text: t("upd.text") }),
    h(
      "div",
      { class: "knopf-reihe" },
      knopf({
        text: t("upd.pruefen"),
        symbol: "sync",
        art: "tonal",
        beiKlick: async () => {
          stand.textContent = t("upd.sucht");
          try {
            const info = await api.updatePruefen();
            stand.textContent = info ? `${t("upd.neu")} ${info.version}` : `${t("upd.aktuell")} (${z.version})`;
            if (info) updateZeigen(info);
          } catch (f) {
            stand.textContent = `${t("upd.fehler")} ${String(f)}`;
          }
        },
      }),
    ),
    stand,
  );
}

/** Mit Windows starten. Der Stand steht in der Registry, darum erst lesen. */
function autostartFlaeche(): HTMLElement {
  const platz = h("div", { class: "block" });
  void api.autostartLesen().then(
    (an) =>
      platz.replaceChildren(
        schalter({
          text: t("ein.autostart"),
          hilfe: t("ein.autostartText"),
          wert: an,
          beiAenderung: async (jetzt) => {
            try {
              await api.autostartSetzen(jetzt);
            } catch (f) {
              melden(String(f));
            }
          },
        }),
      ),
    (f) => platz.replaceChildren(h("p", { class: "text-m leise", text: String(f) })),
  );
  return flaeche(t("ein.start"), "neustart", platz);
}

export function einstellungenSeite(): HTMLElement {
  const inhalt = h("div", { class: "einstellungen-inhalt" });
  const zeigen = () => inhalt.replaceChildren(reiter === "einrichtung" ? einrichtungTeil() : reiter === "sichern" ? sichernTeil() : allgemeinTeil());
  zeigen();
  return h(
    "div",
    { class: "seite" },
    h("div", { class: "seiten-kopf" }, h("h1", { class: "titel-xl", text: t("ein.titel") })),
    segmente<Reiter>({
      titel: t("ein.titel"),
      optionen: [
        { wert: "allgemein", text: t("ein.allgemein") },
        { wert: "einrichtung", text: t("nav.einrichtung") },
        { wert: "sichern", text: t("sich.titel") },
      ],
      haken: false,
      wert: reiter,
      beiAenderung: (w) => {
        reiter = w;
        zeigen();
      },
    }),
    inhalt,
  );
}

/** Reiter „Sichern“: alle Einstellungen als ZIP exportieren und einspielen. */
function sichernTeil(): HTMLElement {
  let mitSounds = true;
  return h(
    "div",
    { class: "raster-2" },
    flaeche(
      t("sich.export"),
      "exportieren",
      h("p", { class: "text-m leise", text: t("sich.exportText") }),
      schalter({ text: t("sich.mitSounds"), hilfe: t("sich.mitSoundsText"), wert: mitSounds, beiAenderung: (an) => void (mitSounds = an) }),
      h(
        "div",
        { class: "knopf-reihe" },
        knopf({
          text: t("sich.exportKnopf"),
          symbol: "exportieren",
          art: "gefuellt",
          beiKlick: async () => {
            try {
              const pfad = await api.einstellungenExportieren(mitSounds);
              if (pfad) melden(`${t("fig.exportFertig")} ${pfad}`);
            } catch (f) {
              melden(String(f));
            }
          },
        }),
      ),
    ),
    flaeche(
      t("sich.import"),
      "importieren",
      h("p", { class: "text-m leise", text: t("sich.importText") }),
      h(
        "div",
        { class: "knopf-reihe" },
        knopf({
          text: t("sich.importKnopf"),
          symbol: "importieren",
          art: "umriss",
          beiKlick: async () => {
            try {
              const r = await api.einstellungenImportieren();
              if (!r) return;
              ersetzen(r.einstellungen);
              melden(`${t("sich.importFertig")} ${r.sounds} ${t("fig.importSounds")}, ${r.bilder} ${t("sich.bilder")}.`, 8000);
              dispatchEvent(new Event("glottix-neu"));
            } catch (f) {
              melden(String(f));
            }
          },
        }),
      ),
    ),
  );
}

function allgemeinTeil(): HTMLElement {
  const o = () => z.e.oberflaeche;
  // Ein Sound-Ausgang für Klick-Töne und das Anhören der Aufnahmen.
  // Ein gespeichertes Gerät, das gerade fehlt, bleibt in der Liste.
  const namen = z.geraete.ausgaenge.includes(o().ui_toene_geraet) || !o().ui_toene_geraet ? z.geraete.ausgaenge : [o().ui_toene_geraet, ...z.geraete.ausgaenge];
  const geraet = auswahl({
    titel: t("ein.soundAusgang"),
    optionen: [{ wert: "", text: t("mik.waehlen") }, ...namen.map((n) => ({ wert: n, text: n }))],
    wert: o().ui_toene_geraet,
    hilfe: t("ein.soundAusgangText"),
    beiAenderung: (w) => aendern(() => (o().ui_toene_geraet = w)),
  });

  return h(
    "div",
    { class: "einstellungen-teil" },
    h(
      "div",
      { class: "raster-2" },
      flaeche(
        t("ein.sprache"),
        "einstellungen",
        segmente({
          titel: t("ein.sprache"),
          optionen: [
            { wert: "de", text: "Deutsch" },
            { wert: "en", text: "English" },
          ],
          wert: o().sprache === "en" ? "en" : "de",
          beiAenderung: (w) => {
            aendern(() => (o().sprache = w));
            dispatchEvent(new Event("glottix-neu"));
          },
        }),
      ),
      autostartFlaeche(),
      updateFlaeche(),
      flaeche(
        t("ein.soundAusgang"),
        "hoeren",
        geraet,
        schalter({
          text: t("ein.uiToene"),
          hilfe: t("ein.uiToeneText"),
          wert: o().ui_toene,
          beiAenderung: (an) => aendern(() => (o().ui_toene = an)),
        }),
      ),
      flaeche(
        t("ein.overlay"),
        "uebersicht",
        h("p", { class: "text-m leise", text: t("ein.overlayText") }),
        schalter({ text: t("ein.overlayAn"), wert: o().overlay, beiAenderung: (an) => aendern(() => (o().overlay = an)) }),
        h(
          "div",
          { class: "block" },
          h("span", { class: "etikett leise", text: t("ein.ecke") }),
          segmente({
            titel: t("ein.ecke"),
            optionen: [
              { wert: "oben-links", text: t("ein.ecke.ol") },
              { wert: "oben-rechts", text: t("ein.ecke.or") },
              { wert: "unten-links", text: t("ein.ecke.ul") },
              { wert: "unten-rechts", text: t("ein.ecke.ur") },
            ],
            wert: o().overlay_ecke || "oben-rechts",
            beiAenderung: (w) => aendern(() => (o().overlay_ecke = w)),
          }),
        ),
      ),
      flaeche(
        t("bug.titel"),
        "info",
        h("p", { class: "text-m leise", text: t("bug.text") }),
        h("div", { class: "knopf-reihe" }, knopf({ text: t("bug.knopf"), symbol: "exportieren", art: "tonal", beiKlick: () => void fehlerMelden() })),
      ),
      flaeche(
        t("ein.version"),
        "info",
        h("p", { class: "text-l zahl", text: `MaskCue ${z.version}` }),
        mitTauri ? null : h("p", { class: "text-m leise", text: t("ein.vorschau") }),
        h("div", { class: "knopf-reihe" }, knopf({ text: t("ein.ordner"), symbol: "ordner", art: "tonal", beiKlick: () => void api.ordnerOeffnen() }), knopf({ text: t("status.neustart"), symbol: "neustart", art: "umriss", beiKlick: () => void api.neustart() }), knopf({ text: t("liz.titel"), symbol: "info", art: "umriss", beiKlick: () => void lizenzenZeigen() })),
      ),
    ),
  );
}
