// Sounds: Pads zum Abspielen, Einstellungen je Sound im Seitenblatt.
// Eigene Reiter (Kategorien) ordnen die Pads: Pad auf einen Reiter ziehen
// oder die Kategorie im Seitenblatt wählen. Hotkeys gibt es im Profil.

import { api } from "../api";
import { bildUrl } from "../avatare";
import { t } from "../i18n";
import { symbol } from "../icons";
import { auswahl, knopf, melden, regler, schalter, segmente, symbolknopf, texteingabe } from "../ui/bausteine";
import { bildWahl } from "../ui/bildwahl";
import { figurBild } from "../ui/formen";
import { h } from "../ui/dom";
import type { Aktion, Sound } from "../typen";
import { aendern, aufStatus, ersetzen, z } from "../zustand";

function soundBlatt(name: string, fertig: () => void) {
  const index = () => z.e.sounds.findIndex((s) => s.name === name);
  const s = (): Sound => z.e.sounds[index()];
  const schliessen = () => {
    schleier.remove();
    blatt.remove();
    removeEventListener("keydown", taste);
    fertig();
  };
  const taste = (ev: KeyboardEvent) => ev.key === "Escape" && schliessen();
  addEventListener("keydown", taste);
  const schleier = h("div", { class: "schleier", onclick: schliessen });
  const blatt = h(
    "aside",
    { class: "seitenblatt", role: "dialog", "aria-label": name },
    h("div", { class: "flaeche-kopf" }, h("h2", { class: "titel-m", text: t("snd.einstellen") }), symbolknopf({ symbol: "schliessen", titel: t("snd.schliessen"), beiKlick: schliessen })),
    h("p", { class: "text-s leise datei", text: s().datei }),
    bildBlock(s, fertig),
    texteingabe({
      titel: t("snd.name"),
      wert: name,
      beiAenderung: async (neu) => {
        try {
          ersetzen(await api.soundUmbenennen(name, neu));
          name = neu.trim();
        } catch (f) {
          melden(String(f));
        }
      },
    }),
    auswahl({
      titel: t("snd.kategorie"),
      optionen: [{ wert: "", text: t("snd.keineKategorie") }, ...(z.e.sound_kategorien ?? []).map((k) => ({ wert: k, text: k }))],
      wert: s().kategorie ?? "",
      beiAenderung: (w) =>
        aendern(() => {
          if (w) s().kategorie = w;
          else delete s().kategorie;
        }),
    }),
    regler({ titel: t("snd.lautstaerke"), min: 0, max: 200, schritt: 5, wert: Math.round(s().lautstaerke * 100), format: (v) => `${v} %`, beiAenderung: (v) => aendern(() => (s().lautstaerke = v / 100)) }),
    schalter({ text: t("snd.schleife"), wert: s().schleife, beiAenderung: (an) => aendern(() => (s().schleife = an)) }),
    schalter({ text: t("snd.musik"), wert: s().musik, beiAenderung: (an) => aendern(() => (s().musik = an)) }),
    h(
      "div",
      { class: "block" },
      h("span", { class: "etikett", text: t("snd.nochmal") }),
      segmente({
        titel: t("snd.nochmal"),
        optionen: [
          { wert: "stopp", text: t("snd.nochmal.stopp") },
          { wert: "neu", text: t("snd.nochmal.neu") },
        ],
        wert: s().nochmal,
        beiAenderung: (w) => aendern(() => (s().nochmal = w)),
      }),
    ),
    h("div", { class: "trenner" }),
    h("span", { class: "etikett", text: t("snd.ziele") }),
    schalter({ text: t("ausgang.spiel"), hilfe: t("mik.spielText"), wert: s().spiel, beiAenderung: (an) => aendern(() => (s().spiel = an)) }),
    schalter({ text: t("ausgang.chat"), hilfe: t("mik.chatText"), wert: s().chat, beiAenderung: (an) => aendern(() => (s().chat = an)) }),
    h("div", { class: "trenner" }),
    knopf({
      text: t("snd.entfernen"),
      symbol: "loeschen",
      art: "fehler",
      beiKlick: () => {
        aendern((e) => {
          e.sounds.splice(index(), 1);
          const passt = (a: Aktion) => typeof a === "object" && "Sound" in a && a.Sound === name;
          for (const p of e.hotkeys.profile) p.belegungen = p.belegungen.filter((b) => !passt(b.aktion));
        });
        schliessen();
      },
    }),
  );
  document.body.append(schleier, blatt);
  blatt.querySelector<HTMLElement>("input")?.focus();
}

/** Bild der Kachel wählen (wie „Bild der normalen Stimme“). Vorschau zeigt die Kachel. */
function bildBlock(s: () => Sound, neuZeichnen: () => void) {
  const vorschau = h("button", { class: "figur-bild pad-vorschau", type: "button", title: t("fig.bild") });
  const zeichnen = () => {
    const [a, b] = padFarben(s());
    vorschau.setAttribute("style", `--pad-a:${a};--pad-b:${b}`);
    vorschau.replaceChildren(padBild(s()) ?? "", h("span", { class: "figur-bild-stift" }, symbol("bearbeiten", true)));
  };
  const waehlen = () =>
    bildWahl(s().name, s().bild, s().bild_ausschnitt, (bild, ausschnitt) => {
      aendern(() => {
        const x = s();
        if (bild) x.bild = bild;
        else delete x.bild;
        if (ausschnitt) x.bild_ausschnitt = ausschnitt;
        else delete x.bild_ausschnitt;
      });
      zeichnen();
      neuZeichnen();
    });
  vorschau.addEventListener("click", waehlen);
  zeichnen();
  return h(
    "div",
    { class: "block" },
    h("span", { class: "etikett", text: t("snd.bild") }),
    h("div", { class: "bild-zeile" }, vorschau, h("div", { class: "block" }, h("p", { class: "text-m leise", text: t("snd.bildText") }), knopf({ text: t("fig.bild"), symbol: "bearbeiten", art: "tonal", beiKlick: waehlen }))),
  );
}

/** Eigenes Bild der Kachel (Avatar oder Datei), sonst `null`. */
function padBild(s: Sound): Node | null {
  if (!s.bild || s.bild.startsWith("farbe:")) return null;
  const url = bildUrl(s.bild);
  return url ? h("span", { class: "pad-bild" }, figurBild(1, url, s.bild_ausschnitt)) : null;
}

/** Farben der Kachel: gewählte Farbe, sonst ein Paar aus dem Namen. */
function padFarben(s: Sound): [string, string] {
  const f = s.bild?.startsWith("farbe:") ? s.bild.slice(6) : null;
  if (f && /^#[0-9a-f]{6}$/i.test(f)) return [f, `color-mix(in srgb, ${f} 55%, #060a17)`];
  return farbe(s.name);
}

/** Sekunden als m:ss. */
function zeit(sek: number) {
  const s = Math.max(0, Math.floor(sek));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

// Farbpaare der Pads (oben, unten): Dämmerung, Glut, Meer, Wald, Neon.
const PAD_FARBEN: [string, string][] = [
  ["#7c3aed", "#db2777"],
  ["#db2777", "#fb923c"],
  ["#2563eb", "#7c3aed"],
  ["#0d9488", "#2563eb"],
  ["#ea580c", "#facc15"],
  ["#9333ea", "#ec4899"],
  ["#047857", "#65a30d"],
  ["#be123c", "#f97316"],
];

function farbe(name: string) {
  let x = 0;
  for (const c of name) x = (x * 31 + c.charCodeAt(0)) >>> 0;
  return PAD_FARBEN[x % PAD_FARBEN.length];
}

const REITER_SPEICHER = "glottix-sound-reiter";
const ZIEH_TYP = "application/x-glottix-sound";

function reiterLesen(): string {
  try {
    return localStorage.getItem(REITER_SPEICHER) ?? "";
  } catch {
    return "";
  }
}

function reiterMerken(k: string) {
  try {
    localStorage.setItem(REITER_SPEICHER, k);
  } catch {
    // Kein Speicher (z. B. privat): dann eben „Alle“ beim nächsten Mal.
  }
}

export function soundsSeite(): HTMLElement {
  const pads = h("div", { class: "pads auftritt" });
  const padMap = new Map<string, HTMLElement>();
  const reiterLeiste = h("div", { class: "chips sound-reiter", role: "tablist", "aria-label": t("snd.kategorien") });
  // Aktiver Reiter ("" = Alle). Gibt es ihn nicht mehr, gilt „Alle“.
  let reiter = reiterLesen();
  if (reiter && !(z.e.sound_kategorien ?? []).includes(reiter)) reiter = "";
  /** Eingabefeld statt „+“ bzw. statt des Reiters beim Umbenennen. */
  let eingabe: { alt: string | null } | null = null;

  const kategorien = () => z.e.sound_kategorien ?? [];
  const waehlen = (k: string) => {
    reiter = k;
    reiterMerken(k);
    zeichnen();
  };
  const verschieben = (name: string, k: string) => {
    aendern((e) => {
      const s = e.sounds.find((x) => x.name === name);
      if (!s) return;
      if (k) s.kategorie = k;
      else delete s.kategorie;
    });
    zeichnen();
  };
  const ablegen = (el: HTMLElement, k: string) => {
    el.addEventListener("dragover", (ev) => {
      if (!ev.dataTransfer?.types.includes(ZIEH_TYP)) return;
      ev.preventDefault();
      el.classList.add("ziel-drueber");
    });
    el.addEventListener("dragleave", () => el.classList.remove("ziel-drueber"));
    el.addEventListener("drop", (ev) => {
      el.classList.remove("ziel-drueber");
      const name = ev.dataTransfer?.getData(ZIEH_TYP);
      if (!name) return;
      ev.preventDefault();
      verschieben(name, k);
    });
  };

  const eingabeFertig = (wert: string) => {
    const alt = eingabe?.alt ?? null;
    eingabe = null;
    const neu = wert.trim();
    const doppelt = kategorien().some((k) => k.toLowerCase() === neu.toLowerCase() && k !== alt);
    if (!neu || doppelt || neu === alt) {
      if (doppelt) melden(t("snd.kategorieDoppelt"));
      return zeichnen();
    }
    aendern((e) => {
      const liste = e.sound_kategorien ?? [];
      if (alt === null) liste.push(neu);
      else {
        liste[liste.indexOf(alt)] = neu;
        for (const s of e.sounds) if (s.kategorie === alt) s.kategorie = neu;
      }
      e.sound_kategorien = liste;
    });
    waehlen(neu);
  };

  const reiterZeichnen = () => {
    const anzahl = (k: string) => (k ? z.e.sounds.filter((s) => s.kategorie === k).length : z.e.sounds.length);
    const chip = (k: string, text: string) => {
      const an = k === reiter;
      const el = h(
        "button",
        { class: `chip${an ? " gewaehlt" : ""}`, type: "button", role: "tab", "aria-selected": String(an), onclick: () => waehlen(k) },
        text,
        h("span", { class: "leise zahl", text: ` ${anzahl(k)}` }),
      );
      ablegen(el, k);
      return el;
    };
    const feld = (alt: string | null) => {
      const f = texteingabe({ titel: t("snd.kategorie"), wert: alt ?? "", platzhalter: t("snd.kategorieNeu"), beiAenderung: eingabeFertig });
      f.classList.add("reiter-eingabe");
      queueMicrotask(() => f.querySelector("input")?.focus());
      return f;
    };
    const teile: Node[] = [chip("", t("snd.alle"))];
    for (const k of kategorien()) teile.push(eingabe?.alt === k ? feld(k) : chip(k, k));
    if (eingabe && eingabe.alt === null) teile.push(feld(null));
    else
      teile.push(
        symbolknopf({
          symbol: "plus",
          titel: t("snd.kategorieNeu"),
          beiKlick: () => {
            eingabe = { alt: null };
            reiterZeichnen();
          },
        }),
      );
    if (reiter && !eingabe) {
      teile.push(
        h("span", { class: "reiter-trenner" }),
        symbolknopf({
          symbol: "bearbeiten",
          titel: t("snd.kategorieUmbenennen"),
          beiKlick: () => {
            eingabe = { alt: reiter };
            reiterZeichnen();
          },
        }),
        symbolknopf({
          symbol: "loeschen",
          titel: t("snd.kategorieLoeschen"),
          beiKlick: () => {
            if (!confirm(`${t("snd.kategorieLoeschenFrage")} „${reiter}“`)) return;
            const weg = reiter;
            aendern((e) => {
              e.sound_kategorien = (e.sound_kategorien ?? []).filter((k) => k !== weg);
              for (const s of e.sounds) if (s.kategorie === weg) delete s.kategorie;
            });
            waehlen("");
          },
        }),
      );
    }
    reiterLeiste.replaceChildren(...teile);
  };

  const zeichnen = () => {
    padMap.clear();
    tastenProfil = null;
    reiterZeichnen();
    if (!z.e.sounds.length) {
      pads.replaceChildren(h("p", { class: "text-l leise leer", text: t("snd.leer") }));
      return;
    }
    const sichtbar = reiter ? z.e.sounds.filter((s) => s.kategorie === reiter) : z.e.sounds;
    if (!sichtbar.length) {
      pads.replaceChildren(h("p", { class: "text-l leise leer", text: t("snd.kategorieLeer") }));
      return;
    }
    pads.replaceChildren(
      ...sichtbar.map((s) => {
        const merkmale = h("span", { class: "pad-merkmale" });
        if (s.schleife) merkmale.append(h("span", { class: "chip klein", title: t("snd.schleife") }, symbol("wiederholen")));
        if (s.musik) merkmale.append(h("span", { class: "chip klein", title: t("snd.musik") }, symbol("musik")));
        if (s.chat) merkmale.append(h("span", { class: "chip klein", title: t("ausgang.chat") }, symbol("chat")));
        if (!s.spiel) merkmale.append(h("span", { class: "chip klein warnung", title: t("ausgang.spiel") }, symbol("spiel"), t("allg.aus")));
        const status = h("span", { class: "pad-status text-s" });
        const [oben, unten] = padFarben(s);
        // Laufzeit-Leiste: zeigt die Stelle, Klicken oder Ziehen springt dorthin.
        const stelle = h("input", { class: "pad-stelle", type: "range", min: 0, max: 1000, step: 1, value: 0, "aria-label": `${t("snd.stelle")}: ${s.name}` }) as HTMLInputElement;
        stelle.addEventListener("input", () => {
          stelle.dataset.zieht = "1";
          stelle.style.setProperty("--anteil", String(Number(stelle.value) / 1000));
        });
        stelle.addEventListener("change", () => {
          void api.soundSpringen(s.name, Number(stelle.value) / 1000);
          // Kurz warten, bis der Status die neue Stelle meldet (sonst springt der Griff zurück).
          setTimeout(() => delete stelle.dataset.zieht, 300);
        });
        const pause = symbolknopf({ symbol: "pause", titel: t("snd.pause"), beiKlick: () => void api.soundPause(s.name) });
        pause.classList.add("pad-pause");
        const steuer = h("span", { class: "pad-steuer" }, pause, symbolknopf({ symbol: "stopp", titel: t("snd.stopp"), beiKlick: () => void api.soundStopp(s.name) }));
        const bild = padBild(s);
        const pad = h(
          "div",
          { class: `pad${bild ? " mit-bild" : ""}`, draggable: "true", title: t("snd.ziehen"), style: `--pad-a:${oben};--pad-b:${unten};--i:${Math.min(padMap.size, 12)}` },
          bild,
          h(
            "button",
            { class: "pad-knopf ebene", type: "button", title: s.datei, onclick: () => void api.ausloesen(s.name) },
            h("span", { class: "pad-symbol" }, symbol("play", true)),
            h("span", { class: "pad-taste", hidden: true }),
            h("span", { class: "eq", "aria-hidden": "true" }, h("i"), h("i"), h("i"), h("i")),
            h("span", { class: "titel-s pad-name", text: s.name }),
            status,
          ),
          h("div", { class: "pad-lauf" }, stelle, h("span", { class: "pad-zeit text-s zahl" })),
          h("div", { class: "pad-fuss" }, merkmale, steuer, symbolknopf({ symbol: "regler", titel: t("snd.einstellen"), beiKlick: () => soundBlatt(s.name, zeichnen) })),
        );
        // Ziehen an der Leiste ist kein Ziehen der Kachel auf einen Reiter:
        // Solange die Maus auf der Leiste gedrückt ist, ist die Kachel fest.
        stelle.addEventListener("pointerdown", () => {
          pad.draggable = false;
          addEventListener("pointerup", () => (pad.draggable = true), { once: true });
        });
        pad.addEventListener("dragstart", (ev) => {
          ev.dataTransfer?.setData(ZIEH_TYP, s.name);
          if (ev.dataTransfer) ev.dataTransfer.effectAllowed = "move";
          pad.classList.add("wird-gezogen");
        });
        pad.addEventListener("dragend", () => pad.classList.remove("wird-gezogen"));
        padMap.set(s.name, pad);
        return pad;
      }),
    );
  };
  let tastenProfil: string | null = null;
  aufStatus((st) => {
    // Hotkey-Abzeichen aus dem aktiven Profil
    if (st.profil !== tastenProfil) {
      tastenProfil = st.profil;
      const belegungen = z.e.hotkeys.profile.find((p) => p.name === st.profil)?.belegungen ?? [];
      for (const [name, pad] of padMap) {
        const b = belegungen.find((x) => typeof x.aktion === "object" && "Sound" in x.aktion && x.aktion.Sound === name);
        const el = pad.querySelector(".pad-taste") as HTMLElement;
        el.hidden = !b;
        el.textContent = b?.taste ?? "";
      }
    }
    const laeuft = new Set(st.sounds_laufen);
    const fehler = new Map(st.sounds_fehler);
    const stand = new Map((st.sounds_stand ?? []).map((x) => [x.name, x]));
    for (const [name, pad] of padMap) {
      const an = laeuft.has(name);
      if (pad.classList.contains("laeuft") !== an) {
        pad.classList.toggle("laeuft", an);
        pad.querySelector(".pad-symbol")?.replaceChildren(symbol(an ? "stopp" : "play", true));
      }
      const x = stand.get(name);
      const pausiert = !!x?.pausiert;
      if (pad.classList.contains("pausiert") !== pausiert) {
        pad.classList.toggle("pausiert", pausiert);
        const p = pad.querySelector(".pad-pause") as HTMLElement;
        p.replaceChildren(symbol(pausiert ? "play" : "pause", true));
        p.title = pausiert ? t("snd.weiter") : t("snd.pause");
        p.setAttribute("aria-label", p.title);
      }
      const stelle = pad.querySelector(".pad-stelle") as HTMLInputElement;
      if (!stelle.dataset.zieht) {
        const anteil = x && x.laenge_s > 0 ? Math.min(1, x.pos_s / x.laenge_s) : 0;
        stelle.value = String(Math.round(anteil * 1000));
        stelle.style.setProperty("--anteil", String(anteil));
      }
      const zeitText = x ? `${zeit(x.pos_s)} / ${zeit(x.laenge_s)}` : "";
      const zeitEl = pad.querySelector(".pad-zeit") as HTMLElement;
      if (zeitEl.textContent !== zeitText) zeitEl.textContent = zeitText;
      const f = fehler.get(name);
      const text = f ? t("snd.fehler") : "";
      const el = pad.querySelector(".pad-status") as HTMLElement;
      if (el.textContent !== text) {
        el.textContent = text;
        el.title = f ?? "";
        pad.classList.toggle("kaputt", !!f);
      }
    }
  });
  zeichnen();

  return h(
    "div",
    { class: "seite" },
    h(
      "div",
      { class: "seiten-kopf" },
      h("h1", { class: "titel-xl", text: t("snd.titel") }),
      knopf({
        text: t("snd.hinzufuegen"),
        symbol: "plus",
        art: "gefuellt",
        gross: true,
        beiKlick: async () => {
          const vorher = new Set(z.e.sounds.map((s) => s.name));
          ersetzen(await api.soundsWaehlen());
          // Neue Sounds kommen in den offenen Reiter.
          if (reiter) {
            const ziel = reiter;
            aendern((e) => {
              for (const s of e.sounds) if (!vorher.has(s.name)) s.kategorie = ziel;
            });
          }
          zeichnen();
        },
      }),
    ),
    h("p", { class: "hinweis text-m" }, symbol("info", true), t("snd.ptt")),
    h(
      "section",
      { class: "flaeche flach" },
      regler({
        titel: t("snd.ducking"),
        min: -30,
        max: 0,
        schritt: 1,
        wert: z.e.mischer.ducking_db,
        format: (v) => (v === 0 ? t("allg.aus") : `${v} ${t("allg.db")}`),
        beiAenderung: (v) => aendern((e) => (e.mischer.ducking_db = v)),
      }),
    ),
    reiterLeiste,
    pads,
  );
}
