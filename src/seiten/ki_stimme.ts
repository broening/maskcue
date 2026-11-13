// KI-Stimme einer Figur (Beatrice): eine Stimme wählen, Thorsten oder
// Stimme W1. Sie läuft vor dem Verfahren (Dan, 21.10.2026). Jede Stimme ist
// ein eigenes Modell mit genau einer Stimme; die Oberfläche zeigt nur den Namen.

import { api } from "../api";
import { t } from "../i18n";
import { auswahl, knopf, melden, regler, schalter, segmente } from "../ui/bausteine";
import { anhaengen, h } from "../ui/dom";
import { symbol } from "../icons";
import type { KiModell, KiTempo, KiWahl } from "../typen";
import { aendern, z } from "../zustand";

let modelle: KiModell[] | null = null;

export async function kiModelle(): Promise<KiModell[]> {
  if (!modelle) modelle = await api.kiModelle();
  return modelle;
}

/** Name der Stimme: Name des Modells ohne Zusatz in Klammern („Thorsten“). */
function stimmText(m: KiModell): string {
  return m.name.replace(/\s*\(.*\)\s*$/, "");
}

/** Name der Stimme für den Ordner `ordner`, sobald die Modelle geladen sind. */
export function kiName(ordner: string): string {
  const m = modelle?.find((x) => x.ordner === ordner);
  return m ? stimmText(m) : ordner.charAt(0).toUpperCase() + ordner.slice(1);
}

/** Wahl für ein Modell: seine (einzige) Stimme ganz. */
function wahlFuer(m: KiModell, alt?: KiWahl): KiWahl {
  return { modell: m.ordner, stimmen: m.stimmen[0] ? [{ nr: m.stimmen[0].nr, gewicht: 1 }] : [], laut_anteil: alt?.laut_anteil ?? 0.25, tonhoehe_auto: alt?.tonhoehe_auto ?? true };
}

/** Startwert, wenn bei einer Figur die KI-Stimme angeht: die erste Stimme. */
export async function kiStart(): Promise<KiWahl | null> {
  const m = (await kiModelle())[0];
  return m ? wahlFuer(m) : null;
}

/** Hörprobe der gewählten Stimme mit dem Probesatz. Ohne Probesatz: Hinweis. */
async function anhoeren(wahl: KiWahl, i: number) {
  if (!z.e.ki?.probesatz) return melden(t("ki.probesatzFehlt"));
  const f = z.e.figuren[i];
  // Wie in der Figur: Mit „Ohne“ stellt die KI Tonhöhe und Formant ein.
  const ohne = f.verfahren === "ohne";
  try {
    await api.kiProbe(wahl, ohne ? f.halbtoene : 0, ohne ? f.formant : 0);
    melden(t("ki.probeLaedt"), 2500);
  } catch (e) {
    melden(String(e));
  }
}

// Fortschritt der Downloads: ein Hörer für die ganze App, verteilt an die
// gerade sichtbare Anzeige je Stimme.
const fortschritt = new Map<string, (anteil: number) => void>();
let hoertDownload = false;

/** Stimmen herunterladen. Vorher stimmt der Spieler den Regeln zu (Dan,
 *  01.11.2026). `null`, wenn alle Stimmen schon da sind. `fertig`: nach einem
 *  Download alles neu zeichnen. */
async function ladenTeil(fertig: () => void): Promise<HTMLElement | null> {
  const angebot = await api.kiAngebot();
  const fehlen = angebot.filter((a) => !a.installiert);
  if (!fehlen.length) return null;
  if (!hoertDownload) {
    hoertDownload = true;
    api.aufKiDownload(([ordner, anteil]) => fortschritt.get(ordner)?.(anteil));
  }
  const el = h("div", { class: "ki-laden" });
  const zeichnen = () => {
    const ok = z.e.ki?.regeln_ok;
    el.replaceChildren();
    anhaengen(el, [
      h("h4", { class: "titel-s", text: t("ki.ladenTitel") }),
      h("p", { class: "text-m leise", text: t("ki.ladenText") }),
      h(
        "div",
        { class: "ki-regeln" },
        h("span", { class: "etikett", text: t("ki.regelnTitel") }),
        h("ul", { class: "text-m" }, ...[t("ki.regel1"), t("ki.regel2"), t("ki.regel3"), t("ki.regel4")].map((r) => h("li", { text: r }))),
        ok
          ? h("p", { class: "text-s leise", text: `${t("ki.regelnZugestimmt")} ${new Date(ok).toLocaleDateString(document.documentElement.lang || undefined)}` })
          : schalter({
              text: t("ki.regelnOk"),
              wert: false,
              beiAenderung: (an) => {
                if (!an) return;
                aendern((e) => {
                  e.ki ??= {};
                  e.ki.regeln_ok = new Date().toISOString().slice(0, 10);
                });
                zeichnen();
              },
            }),
      ),
      ...fehlen.map((a) => {
        const mb = (a.bytes / 1_000_000).toLocaleString(document.documentElement.lang || undefined, { maximumFractionDigits: 0 });
        const stand = h("span", { class: "text-s leise", text: `${mb} MB` });
        const b = knopf({
          text: t("ki.laden"),
          symbol: "importieren",
          art: "gefuellt",
          beiKlick: async () => {
            b.disabled = true;
            fortschritt.set(a.ordner, (anteil) => (stand.textContent = `${Math.round(anteil * 100)} %`));
            try {
              await api.kiHerunterladen(a.ordner);
              kiNeuLaden();
              melden(`${t("ki.geladen")}: ${a.name}`);
              fertig();
            } catch (f) {
              melden(String(f));
              b.disabled = false;
              stand.textContent = `${mb} MB`;
            } finally {
              fortschritt.delete(a.ordner);
            }
          },
        });
        b.disabled = !ok;
        return h("div", { class: "ki-laden-zeile" }, h("span", { class: "titel-s", text: a.name }), stand, b);
      }),
    ]);
  };
  zeichnen();
  return el;
}

/** Nach einem Download: Modelle neu lesen. */
function kiNeuLaden() {
  modelle = null;
}

/** Probesatz wählen und daran die eigene Tonhöhe messen. */
async function probesatzTeil(): Promise<HTMLElement> {
  const el = h("div", { class: "block" });
  const aufnahmen = (await api.aufnahmenListe()).sort((a, b) => b.zeit - a.zeit);
  const zeichnen = () => {
    const hz = z.e.ki?.eigene_hz;
    el.replaceChildren();
    anhaengen(el, [
      aufnahmen.length
        ? auswahl({
            titel: t("ki.probesatz"),
            optionen: [{ wert: "", text: t("ki.probesatzWaehlen") }, ...aufnahmen.map((a) => ({ wert: a.name, text: `${a.name} (${Math.round(a.sekunden)} s)` }))],
            wert: z.e.ki?.probesatz ?? "",
            hilfe: t("ki.probesatzText"),
            beiAenderung: async (name) => {
              aendern((e) => {
                e.ki ??= {};
                e.ki.probesatz = name;
              });
              if (!name) return zeichnen();
              try {
                const gemessen = await api.kiTonhoeheMessen(name);
                if (gemessen) {
                  aendern((e) => {
                    e.ki ??= {};
                    e.ki.eigene_hz = Math.round(gemessen);
                  });
                } else melden(t("ki.zuWenigStimme"));
              } catch (f) {
                melden(String(f));
              }
              zeichnen();
            },
          })
        : h("p", { class: "text-s leise", text: t("ki.probesatzKeine") }),
      h("p", { class: "text-s leise", text: hz ? `${t("ki.eigeneHz")}: ${hz} Hz` : t("ki.eigeneHzFehlt") }),
    ]);
  };
  zeichnen();
  return el;
}

/** Ergebnis des Geschwindigkeitstests je Modell (einmal je Start der App). */
const tempoStand = new Map<string, Promise<KiTempo>>();

/** Stufe aus dem Test. Die Spitze zählt mit: Über 100 % knackt es (Dan, 02.11.2026). */
function tempoStufe(r: KiTempo): "gut" | "knapp" | "langsam" {
  if (r.mittel < 0.5 && r.spitze < 0.8) return "gut";
  if (r.mittel < 0.8 && r.spitze < 1.0) return "knapp";
  return "langsam";
}

/** Schafft dieser PC die KI-Stimme? Misst beim ersten Einschalten von selbst. */
function tempoTeil(wahl: KiWahl): HTMLElement {
  const el = h("div", { class: "ki-tempo", role: "status" });
  const zeigen = async (neu: boolean) => {
    if (neu || !tempoStand.has(wahl.modell)) tempoStand.set(wahl.modell, api.kiTempo(wahl));
    el.className = "ki-tempo";
    el.replaceChildren(h("p", { class: "text-s leise", text: t("ki.tempoMisst") }));
    try {
      const r = await tempoStand.get(wahl.modell)!;
      const stufe = tempoStufe(r);
      el.className = `ki-tempo ki-tempo-${stufe}`;
      el.replaceChildren(
        h("p", { class: "text-m" }, symbol(stufe === "gut" ? "haken" : "warnung", true), t(`ki.tempo.${stufe}`)),
        h("p", { class: "text-s leise", text: `${t("ki.tempoWert")} ${Math.round(r.mittel * 100)} %, ${t("ki.tempoSpitze")} ${Math.round(r.spitze * 100)} %` }),
        knopf({ text: t("ki.tempoNeu"), symbol: "neustart", art: "umriss", beiKlick: () => void zeigen(true) }),
      );
    } catch (f) {
      tempoStand.delete(wahl.modell);
      el.replaceChildren(h("p", { class: "text-s leise", text: String(f) }));
    }
  };
  void zeigen(false);
  return el;
}

/** Teil in der Figur-Karte: KI-Stimme an/aus, Stimme wählen, anhören,
 *  Tonhöhe automatisch, Aussprache. `neu`: ganze Karte neu zeichnen (die
 *  Regler für Tonhöhe und Klangfarbe hängen davon ab). */
export function kiTeil(i: number, neu: () => void): HTMLElement {
  const el = h("div", { class: "block ki-teil" });
  const zeichnen = async () => {
    const f = z.e.figuren[i];
    const liste = await kiModelle();
    const an = !!f.ki_an;
    const schalterAn = schalter({
      text: t("ki.titel"),
      hilfe: t("ki.anText"),
      wert: an,
      beiAenderung: async (jetzt) => {
        const start = jetzt && !z.e.figuren[i].ki ? await kiStart() : null;
        aendern((e) => {
          const g = e.figuren[i];
          if (jetzt) {
            g.ki_an = true;
            if (start) g.ki = start;
            if ((g.verfahren ?? "psola") === "ohne") {
              // KI stellt die Tonhöhe selbst ein: Regler wird Feinabstimmung.
              g.halbtoene = 0;
              g.formant = Math.max(-2, Math.min(2, g.formant));
            }
          } else delete g.ki_an;
        });
        neu();
      },
    });
    const laden = await ladenTeil(() => void zeichnen());
    // Ohne Stimme gibt es nichts einzuschalten: nur der Download.
    if (!liste.length) return el.replaceChildren(laden ?? h("p", { class: "text-m leise", text: t("ki.keineModelle") }));
    if (!an || !f.ki) return el.replaceChildren(...[schalterAn, laden].filter((x) => x !== null));
    const k = f.ki;
    const setzen = (fn: (x: KiWahl) => void) =>
      aendern((e) => {
        const x = e.figuren[i].ki;
        if (x) fn(x);
      });
    el.replaceChildren();
    anhaengen(el, [
      schalterAn,
      segmente({
        titel: t("ki.stimme"),
        optionen: liste.map((m) => ({ wert: m.ordner, text: stimmText(m) })),
        wert: k.modell,
        beiAenderung: (w) => {
          const m = liste.find((x) => x.ordner === w);
          if (!m) return;
          aendern((e) => (e.figuren[i].ki = wahlFuer(m, e.figuren[i].ki)));
          void zeichnen();
        },
      }),
      h(
        "div",
        { class: "knopf-reihe" },
        knopf({ text: t("ki.anhoeren"), symbol: "play", art: "umriss", beiKlick: () => void anhoeren(z.e.figuren[i].ki!, i) }),
        knopf({ text: t("ki.stopp"), symbol: "stopp", art: "umriss", beiKlick: () => void api.aufnahmeAbhoeren(null) }),
      ),
      tempoTeil(k),
      await probesatzTeil(),
      schalter({
        text: t("ki.tonhoeheAuto"),
        hilfe: t("ki.tonhoeheAutoText"),
        wert: k.tonhoehe_auto ?? true,
        beiAenderung: (jetzt) => {
          aendern((e) => {
            const x = e.figuren[i].ki;
            if (x) x.tonhoehe_auto = jetzt;
            // Mit „Ohne“: Wechsel zwischen fein (±6) und ganz (±12), bei 0 anfangen.
            if ((e.figuren[i].verfahren ?? "psola") === "ohne") e.figuren[i].halbtoene = 0;
          });
          neu();
        },
      }),
      regler({
        titel: t("ki.aussprache"),
        min: 0,
        max: 100,
        schritt: 5,
        wert: Math.round((k.laut_anteil ?? 1) * 100),
        format: (v) => (v >= 100 ? t("ki.ziel") : v <= 0 ? t("ki.eigene") : `${v} %`),
        hilfe: t("ki.ausspracheText"),
        beiAenderung: (v) => setzen((x) => (x.laut_anteil = v / 100)),
      }),
      h("p", { class: "text-s leise", text: t("ki.quelle") }),
      laden,
    ]);
  };
  void zeichnen();
  return el;
}
