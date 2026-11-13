// Figuren: Stimmen anlegen, einstellen, einschalten.

import { api } from "../api";
import { t } from "../i18n";
import { auswahl, knopf, melden, regler, schalter, symbolknopf } from "../ui/bausteine";
import { h } from "../ui/dom";
import { bildVon } from "../ui/formen";
import { bildWahl } from "../ui/bildwahl";
import { symbol, type SymbolName } from "../icons";
import { UMGEBUNGEN, umgebungStaerke, type Aktion, type Einstellungen, type Figur, type Umgebung } from "../typen";
import { umgebungName } from "./umgebungen";
import { kiModelle, kiName, kiTeil } from "./ki_stimme";
import { aendern, aufStatus, ersetzen, z } from "../zustand";

const halbtoene = (v: number) => `${v > 0 ? "+" : ""}${v} ${t("fig.halbtoene")}`;

/** Zahl mit Vorzeichen und 2 Nachkommastellen, Komma nach Sprache (Tonhöhe in 0,05-Schritten). */
export const vorzeichenFein = (v: number) =>
  `${v > 0 ? "+" : ""}${v.toLocaleString(document.documentElement.lang || undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;

const halbtoeneFein = (v: number) => `${vorzeichenFein(v)} ${t("fig.halbtoene")}`;

/** Hotkeys, die auf die Figur `alt` zeigen, zeigen danach auf `neu` (oder werden gelöscht). */
function verweiseAendern(e: Einstellungen, alt: string, neu: string | null) {
  const passt = (a: Aktion) => typeof a === "object" && "Figur" in a && a.Figur === alt;
  for (const p of e.hotkeys.profile) {
    p.belegungen = p.belegungen.filter((b) => neu !== null || !passt(b.aktion));
    for (const b of p.belegungen) if (passt(b.aktion) && neu !== null) b.aktion = { Figur: neu };
  }
}

function eindeutig(name: string, ausser = -1) {
  let n = name.trim() || t("fig.neuName");
  let i = 2;
  const basis = n;
  while (z.e.figuren.some((f, j) => j !== ausser && f.name.toLowerCase() === n.toLowerCase())) n = `${basis} ${i++}`;
  return n;
}

/** Seitenblatt zum Exportieren: Sounds wählen, dann als ZIP speichern. */
function exportBlatt(figur: string) {
  const gewaehlt = new Set<string>();
  const schliessen = () => {
    schleier.remove();
    blatt.remove();
    removeEventListener("keydown", taste);
  };
  const taste = (ev: KeyboardEvent) => ev.key === "Escape" && schliessen();
  addEventListener("keydown", taste);
  const schleier = h("div", { class: "schleier", onclick: schliessen });
  const liste = z.e.sounds.length
    ? h(
        "div",
        { class: "export-liste" },
        ...z.e.sounds.map((s) => schalter({ text: s.name, wert: false, beiAenderung: (an) => void (an ? gewaehlt.add(s.name) : gewaehlt.delete(s.name)) })),
      )
    : h("p", { class: "text-m leise", text: t("fig.exportKeine") });
  const blatt = h(
    "aside",
    { class: "seitenblatt", role: "dialog", "aria-label": `${t("fig.exportieren")}: ${figur}` },
    h("div", { class: "flaeche-kopf" }, h("h2", { class: "titel-m", text: `${t("fig.exportieren")}: ${figur}` }), symbolknopf({ symbol: "schliessen", titel: t("snd.schliessen"), beiKlick: schliessen })),
    h("p", { class: "text-m leise", text: t("fig.exportText") }),
    h("span", { class: "etikett", text: t("fig.exportSounds") }),
    liste,
    knopf({
      text: t("fig.exportieren"),
      symbol: "exportieren",
      art: "gefuellt",
      gross: true,
      beiKlick: async () => {
        try {
          const pfad = await api.figurExportieren(figur, [...gewaehlt]);
          if (pfad) {
            melden(`${t("fig.exportFertig")} ${pfad}`);
            schliessen();
          }
        } catch (f) {
          melden(String(f));
        }
      },
    }),
  );
  document.body.append(schleier, blatt);
}

/** Verfahren für Tonhöhe und Klangfarbe: Schnell (PSOLA), Natürlich
 *  (Signalsmith), Leicht (WSOLA, Helium-Klang) oder Ohne. Die KI-Stimme ist
 *  getrennt davon und läuft davor (`kiTeil`). Unter der Auswahl steht die
 *  Beschreibung des gewählten Verfahrens. `neu`: Karte neu zeichnen. */
function verfahrenWahl(i: number, neu: () => void): HTMLElement {
  const f = () => z.e.figuren[i];
  const w = f().verfahren ?? "psola";
  const text = { psola: t("fig.verfahrenPsolaText"), signalsmith: t("fig.verfahrenSignalsmithText"), wsola: t("fig.verfahrenWsolaText"), ohne: t("fig.verfahrenOhneText") };
  return auswahl({
    titel: t("fig.verfahren"),
    optionen: [
      { wert: "psola", text: t("fig.verfahrenPsola") },
      { wert: "signalsmith", text: t("fig.verfahrenSignalsmith") },
      { wert: "wsola", text: t("fig.verfahrenWsola") },
      { wert: "ohne", text: t("fig.verfahrenOhne") },
    ],
    wert: w,
    hilfe: text[w],
    beiAenderung: (neuW) => {
      aendern((e) => {
        const g = e.figuren[i];
        const vorher = g.verfahren ?? "psola";
        if (neuW === "signalsmith" || neuW === "wsola" || neuW === "ohne") g.verfahren = neuW;
        else delete g.verfahren;
        // Mit KI: Bei „Ohne“ stellt die KI die Tonhöhe ein (Regler = Feinabstimmung,
        // Klangfarbe nur ±2), sonst das Verfahren. Beim Wechsel bei 0 anfangen.
        if (g.ki_an && (vorher === "ohne") !== (neuW === "ohne")) {
          g.halbtoene = 0;
          g.formant = 0;
        }
      });
      neu();
    },
  });
}

/** Umgebung der Figur mit eigener Stärke. Die Stärke gehört nur dieser Figur
 *  (getrennt von der Stärke auf der Seite „Umgebungen“). */
function umgebungWahl(i: number): HTMLElement {
  const f = () => z.e.figuren[i];
  const staerkeTeil = h("div", { class: "block" });
  const zeichnenStaerke = () => {
    const u = f().umgebung;
    staerkeTeil.hidden = !u;
    if (!u) return staerkeTeil.replaceChildren();
    staerkeTeil.replaceChildren(
      regler({
        titel: t("umg.figurStaerke"),
        min: 0,
        max: 100,
        schritt: 5,
        wert: Math.round((f().umgebung_staerke ?? umgebungStaerke(z.e, u)) * 100),
        format: (v) => `${v} %`,
        hilfe: t("umg.figurStaerkeText"),
        beiAenderung: (v) => aendern((e) => (e.figuren[i].umgebung_staerke = v / 100)),
      }),
    );
  };
  zeichnenStaerke();
  return h(
    "div",
    { class: "raster-2 umgebung-figur" },
    auswahl({
      titel: t("umg.figur"),
      optionen: [{ wert: "", text: t("umg.keine") }, ...UMGEBUNGEN.map((u) => ({ wert: u, text: umgebungName(u) }))],
      wert: f().umgebung ?? "",
      hilfe: t("umg.figurText"),
      beiAenderung: (w) => {
        aendern((e) => {
          if (w) {
            e.figuren[i].umgebung = w as Umgebung;
            // Startwert: die globale Stärke, ab dann eigenständig.
            e.figuren[i].umgebung_staerke = umgebungStaerke(e, w as Umgebung);
          } else {
            delete e.figuren[i].umgebung;
            delete e.figuren[i].umgebung_staerke;
          }
        });
        zeichnenStaerke();
      },
    }),
    staerkeTeil,
  );
}

/** Regler für Tonhöhe, Klangfarbe und Hell/Dunkel. Welche Regler es gibt,
 *  hängt vom Verfahren und von der KI-Stimme ab. */
function tonRegler(i: number): HTMLElement {
  const f = z.e.figuren[i];
  // Ohne: keine Tonhöhe, keine Klangfarbe, außer die KI-Stimme ist an
  // (dann stellt sie beides ein). Leicht hält den Formant nicht, der
  // Klangfarbe-Regler würde dort nichts tun.
  const kiStellt = !!f.ki_an && f.verfahren === "ohne";
  const zeigeTon = f.verfahren !== "ohne" || kiStellt;
  const zeigeKlang = (f.verfahren !== "wsola" && f.verfahren !== "ohne") || kiStellt;
  return h(
    "div",
    { class: "raster-2" },
    zeigeTon
      ? kiStellt && (f.ki?.tonhoehe_auto ?? true)
        ? regler({
            titel: t("fig.tonhoeheFein"),
            min: -6,
            max: 6,
            schritt: 1,
            mitte: true,
            wert: Math.max(-6, Math.min(6, f.halbtoene)),
            format: halbtoene,
            beiAenderung: (v) => aendern((e) => (e.figuren[i].halbtoene = v)),
          })
        : kiStellt
          ? regler({ titel: t("fig.tonhoehe"), min: -12, max: 12, schritt: 1, mitte: true, wert: f.halbtoene, format: halbtoene, beiAenderung: (v) => aendern((e) => (e.figuren[i].halbtoene = v)) })
          : // −15 bis +30 wie Clownfish. Nullpunkt 0, nicht die Mitte (+7,5).
            regler({
              titel: t("fig.tonhoehe"),
              min: -15,
              max: 30,
              schritt: 0.05,
              nullpunkt: 0,
              wert: Math.max(-15, Math.min(30, f.halbtoene)),
              format: halbtoeneFein,
              beiAenderung: (v) => aendern((e) => (e.figuren[i].halbtoene = v)),
            })
      : null,
    zeigeKlang
      ? kiStellt
        ? regler({
            titel: t("fig.klangfarbe"),
            min: -2,
            max: 2,
            schritt: 0.5,
            mitte: true,
            wert: Math.max(-2, Math.min(2, f.formant)),
            format: halbtoene,
            hilfe: t("ki.formantText"),
            beiAenderung: (v) => aendern((e) => (e.figuren[i].formant = v)),
          })
        : regler({
            titel: t("fig.klangfarbe"),
            min: -4,
            max: 12,
            schritt: 1,
            wert: f.formant,
            format: halbtoene,
            hilfe: t("fig.klangfarbeText"),
            beiAenderung: (v) => aendern((e) => (e.figuren[i].formant = v)),
          })
      : null,
    regler({
      titel: t("fig.hell"),
      min: -6,
      max: 6,
      schritt: 1,
      mitte: true,
      wert: f.hell ?? 0,
      format: (v) => `${v > 0 ? "+" : ""}${v} dB`,
      hilfe: t("fig.hellText"),
      beiAenderung: (v) =>
        aendern((e) => {
          if (v) e.figuren[i].hell = v;
          else delete e.figuren[i].hell;
        }),
    }),
  );
}

/** Die drei Stufen, in der Reihenfolge, in der die Stimme sie durchläuft. */
type Stufe = "ki" | "ton" | "umgebung";
const STUFEN: { wert: Stufe; symbol: SymbolName; titel: () => string; text: () => string }[] = [
  { wert: "ki", symbol: "stimme", titel: () => t("fig.stufeKi"), text: () => t("fig.stufeKiText") },
  { wert: "ton", symbol: "regler", titel: () => t("fig.stufeTon"), text: () => t("fig.stufeTonText") },
  { wert: "umgebung", symbol: "umgebung", titel: () => t("fig.stufeUmgebung"), text: () => t("fig.stufeUmgebungText") },
];

/** Kurzer Name des Verfahrens, ohne die Zeit in Klammern („Schnell“). */
function verfahrenKurz(f: Figur): string {
  const lang = { psola: t("fig.verfahrenPsola"), signalsmith: t("fig.verfahrenSignalsmith"), wsola: t("fig.verfahrenWsola"), ohne: t("fig.verfahrenOhne") }[f.verfahren ?? "psola"];
  return lang.replace(/\s*\(.*\)\s*$/, "");
}

/** Wert einer Stufe in ein, zwei Wörtern: „Thorsten“, „Schnell · −4 Halbtöne“, „Halle 60 %“. */
function stufenWert(f: Figur, s: Stufe): string {
  if (s === "ki") return f.ki_an && f.ki ? kiName(f.ki.modell) : t("fig.aus");
  if (s === "umgebung") return f.umgebung ? `${umgebungName(f.umgebung)} ${Math.round((f.umgebung_staerke ?? umgebungStaerke(z.e, f.umgebung)) * 100)} %` : t("umg.keine");
  const ton = f.verfahren !== "ohne" || (f.ki_an && f.ki) ? f.halbtoene : 0;
  const zahl = Number.isInteger(ton) ? `${ton > 0 ? "+" : ""}${ton}` : vorzeichenFein(ton);
  return ton ? `${verfahrenKurz(f)} · ${zahl} ${t("fig.halbtoene")}` : verfahrenKurz(f);
}

// Offene Figur (Name) und gewählte Stufe. Bleiben beim Neuzeichnen und Seitenwechsel.
let offen: string | null = null;
let stufe: Stufe = "ton";

export function figurenSeite(): HTMLElement {
  const liste = h("div", { class: "figuren-liste auftritt" });
  let gezeichnet = false;

  /** Einstellungen einer Figur: oben der Weg der Stimme (Mikrofon → KI → Ton →
   *  Umgebung → Ausgang), jede Stufe ein Reiter mit ihrem Wert. Darunter die
   *  Regler der gewählten Stufe. `kurz`: Chips in der Kopfzeile auffrischen. */
  const einstellungen = (i: number, kurz: () => void): HTMLElement => {
    const f = () => z.e.figuren[i];
    const werte = new Map<Stufe, HTMLElement>();
    const weg = h(
      "div",
      { class: "stimm-weg", role: "tablist", "aria-label": t("fig.weg") },
      h("span", { class: "weg-ende etikett" }, symbol("mikrofon"), t("fig.wegMikro")),
      ...STUFEN.flatMap((s, n) => {
        const wert = h("span", { class: "weg-wert text-s", text: stufenWert(f(), s.wert) });
        werte.set(s.wert, wert);
        return [
          h("span", { class: "weg-pfeil", "aria-hidden": "true", text: "→" }),
          h(
            "button",
            {
              class: `weg-stufe${s.wert === stufe ? " gewaehlt" : ""}`,
              type: "button",
              role: "tab",
              title: s.text(),
              "aria-selected": String(s.wert === stufe),
              onclick: () => {
                stufe = s.wert;
                zeichnen();
              },
            },
            h("span", { class: "weg-nr", text: String(n + 1) }),
            h("span", { class: "weg-text" }, h("span", { class: "etikett", text: s.titel() }), wert),
          ),
        ];
      }),
      h("span", { class: "weg-pfeil", "aria-hidden": "true", text: "→" }),
      h("span", { class: "weg-ende etikett" }, symbol("laut"), t("fig.wegAusgang")),
    );
    const s = STUFEN.find((x) => x.wert === stufe)!;
    // Ton: Verfahren und Regler zusammen in zwei Spalten, über die ganze Breite.
    const ton = () => {
      const r = tonRegler(i);
      r.prepend(verfahrenWahl(i, zeichnen));
      return r;
    };
    const inhalt = stufe === "ki" ? kiTeil(i, zeichnen) : stufe === "umgebung" ? umgebungWahl(i) : ton();
    const teil = h(
      "div",
      { class: "figur-einstellungen", role: "tabpanel" },
      weg,
      // Die KI-Stimme erklärt sich mit ihrem Schalter selbst.
      stufe === "ki" ? null : h("div", { class: "stufe-kopf" }, h("h3", { class: "titel-s", text: s.titel() }), h("p", { class: "text-m leise", text: s.text() })),
      inhalt,
    );
    // Regler ändern die Werte ohne Neuzeichnen: Weg und Chips hier nachziehen.
    const auffrischen = () =>
      queueMicrotask(() => {
        for (const [st, el] of werte) el.textContent = stufenWert(f(), st);
        kurz();
      });
    teil.addEventListener("input", auffrischen);
    teil.addEventListener("change", auffrischen);
    return teil;
  };

  const karte = (f: Figur, i: number, aktiv: string | null): HTMLElement => {
    const istOffen = offen === f.name;
    const kachel = h(
      "button",
      {
        class: "figur-bild",
        type: "button",
        title: t("fig.bild"),
        onclick: () =>
          bildWahl(f.name, f.bild, f.bild_ausschnitt, (bild, ausschnitt) => {
            aendern((e) => {
              e.figuren[i].bild = bild;
              if (ausschnitt) e.figuren[i].bild_ausschnitt = ausschnitt;
              else delete e.figuren[i].bild_ausschnitt;
            });
            zeichnen();
          }),
      },
      bildVon(i + 1, f.bild, f.name, f.bild_ausschnitt),
      h("span", { class: "figur-bild-stift" }, symbol("bearbeiten", true)),
    );
    const name = h("input", { class: "figur-name titel-m", type: "text", value: f.name, "aria-label": t("fig.name"), title: t("fig.name") });
    name.addEventListener("keydown", (ev) => ev.key === "Enter" && name.blur());
    name.addEventListener("change", () => {
      const neu = eindeutig(name.value, i);
      aendern((e) => {
        verweiseAendern(e, f.name, neu);
        e.figuren[i].name = neu;
      });
      if (offen === f.name) offen = neu;
      if (aktiv === f.name) void api.figur(neu);
      zeichnen();
    });
    // Chips: jede Stufe auf einen Blick. Ein Klick öffnet diese Stufe.
    const chips = h("div", { class: "figur-chips" });
    const kurz = () => {
      const g = z.e.figuren[i];
      chips.replaceChildren(
        ...STUFEN.map((s) => {
          const aus = (s.wert === "ki" && !(g.ki_an && g.ki)) || (s.wert === "umgebung" && !g.umgebung);
          return h(
            "button",
            {
              class: `chip klein${aus ? " aus" : ""}`,
              type: "button",
              title: `${s.titel()}: ${stufenWert(g, s.wert)}`,
              onclick: () => {
                offen = g.name;
                stufe = s.wert;
                zeichnen();
              },
            },
            symbol(s.symbol, !aus),
            aus ? t(s.wert === "ki" ? "fig.ohneKi" : "fig.ohneUmgebung") : stufenWert(g, s.wert),
          );
        }),
      );
    };
    kurz();
    const einstellen = knopf({
      text: t("fig.einstellen"),
      symbol: istOffen ? "pfeil_runter" : "regler",
      art: istOffen ? "tonal" : "umriss",
      beiKlick: () => {
        offen = istOffen ? null : f.name;
        zeichnen();
      },
    });
    einstellen.setAttribute("aria-expanded", String(istOffen));
    return h(
      "section",
      { class: `flaeche figur${aktiv === f.name ? " aktiv" : ""}${istOffen ? " offen" : ""}` },
      h(
        "div",
        { class: "figur-kopf" },
        kachel,
        h("div", { class: "figur-titel" }, name, chips),
        h(
          "div",
          { class: "figur-knoepfe" },
          knopf({
            text: aktiv === f.name ? t("fig.aktiv") : t("fig.einschalten"),
            symbol: "stimme",
            art: aktiv === f.name ? "spielen" : "gefuellt",
            beiKlick: () => void api.figur(aktiv === f.name ? null : f.name),
          }),
          einstellen,
          symbolknopf({ symbol: "exportieren", titel: t("fig.exportieren"), beiKlick: () => exportBlatt(f.name) }),
          symbolknopf({
            symbol: "loeschen",
            titel: t("fig.loeschen"),
            beiKlick: () => {
              if (!confirm(`${t("fig.loeschenFrage")} „${f.name}“`)) return;
              aendern((e) => {
                verweiseAendern(e, f.name, null);
                e.figuren.splice(i, 1);
              });
              if (offen === f.name) offen = null;
              zeichnen();
            },
          }),
        ),
      ),
      istOffen ? einstellungen(i, kurz) : null,
    );
  };

  const zeichnen = () => {
    const aktiv = z.status?.figur ?? null;
    const karten = z.e.figuren.map((f, i) => karte(f, i, aktiv));
    // Nur beim ersten Mal auftreten. Danach (Aufklappen, Ändern) nicht neu animieren.
    if (gezeichnet) karten.forEach((k) => k.classList.add("da"));
    gezeichnet = true;
    liste.replaceChildren(...karten);
  };
  // Namen der KI-Stimmen für die Chips.
  void kiModelle().then(() => zeichnen());
  let letzte: string | null | undefined;
  aufStatus((s) => {
    if (s.figur !== letzte) {
      letzte = s.figur;
      zeichnen();
    }
  });
  zeichnen();

  return h(
    "div",
    { class: "seite" },
    h(
      "div",
      { class: "seiten-kopf" },
      h("div", {}, h("h1", { class: "titel-xl", text: t("fig.titel") }), h("p", { class: "text-l leise", text: t("fig.text") })),
      h(
        "div",
        { class: "knopf-reihe" },
        knopf({
          text: t("fig.importieren"),
          symbol: "importieren",
          art: "umriss",
          gross: true,
          beiKlick: async () => {
            try {
              const r = await api.figurImportieren();
              if (!r) return;
              ersetzen(r.einstellungen);
              zeichnen();
              let text = `${t("fig.importFertig")} ${r.name}, ${r.sounds} ${t("fig.importSounds")}`;
              if (r.uebersprungen) text += ` · ${r.uebersprungen} ${t("fig.importUebersprungen")}`;
              melden(text);
            } catch (f) {
              melden(String(f));
            }
          },
        }),
        knopf({
        text: t("fig.neu"),
        symbol: "plus",
        art: "gefuellt",
        gross: true,
        beiKlick: () => {
          const name = eindeutig(t("fig.neuName"));
          aendern((e) => e.figuren.push({ name, halbtoene: 0, formant: 0 }));
          offen = name;
          stufe = "ton";
          zeichnen();
        },
      }),
      ),
    ),
    liste,
  );
}
