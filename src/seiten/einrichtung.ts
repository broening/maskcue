// Einrichtung: Liste zum Abhaken für die Audio-Software und die Programme
// (PLAN.md, „App erinnert an die Einstellungen in den anderen Programmen“).
//
// Nicht jeder nutzt Wave Link. Darum wählt man oben seine Audio-Software.
// Die Texte nennen dann die passenden Geräte. In Programmen wie Discord wählt
// man immer das Gerät, das die Software anbietet (bei Wave Link einen Mix,
// nie einen Kanal). Abgehakt wird nur auf diesem Rechner (localStorage).
//
// Die Texte stehen hier statt in i18n.ts, weil sie je Software wechseln.

import { symbol, type SymbolName } from "../icons";
import { flaeche, segmente } from "../ui/bausteine";
import { h } from "../ui/dom";
import { z } from "../zustand";

type Software = "wavelink" | "voicemeeter" | "vbcable" | "andere";
type Text = [de: string, en: string];

const SPEICHER = "glottix-einrichtung";
const SPEICHER_SOFTWARE = "glottix-einrichtung-software";

/** Gerät, das die Programme als Mikrofon wählen: [Spiel, Chat]. */
const EINGAENGE: Record<Software, [Text, Text]> = {
  wavelink: [
    ["dein Mix für Spiele (z. B. „Spiel“)", "your mix for games (e.g. “Game”)"],
    ["der „Chat Mix“", "the “Chat Mix”"],
  ],
  voicemeeter: [
    ["„Voicemeeter Out B1“", "“Voicemeeter Out B1”"],
    ["„Voicemeeter Out B2“", "“Voicemeeter Out B2”"],
  ],
  vbcable: [
    ["„CABLE Output“", "“CABLE Output”"],
    ["„CABLE-A Output“", "“CABLE-A Output”"],
  ],
  andere: [
    ["das Gegenstück zum Spiel-Ausgang", "the counterpart of the game output"],
    ["das Gegenstück zum Chat-Ausgang", "the counterpart of the chat output"],
  ],
};

/** Schritte in der Audio-Software. {spiel} und {chat} = Ausgänge von MaskCue. */
const SOFTWARE_SCHRITTE: Record<Software, Text[]> = {
  wavelink: [
    ["Die Ausgänge von MaskCue ({spiel}, {chat}) erscheinen in Wave Link als Kanäle.", "MaskCue outputs ({spiel}, {chat}) show up in Wave Link as channels."],
    ["Mix für Spiele: Kanal „{spiel}“ an, echtes Mikrofon aus.", "Mix for games: channel “{spiel}” on, real microphone off."],
    ["„Chat Mix“: Kanal „{chat}“ an, echtes Mikrofon aus.", "“Chat Mix”: channel “{chat}” on, real microphone off."],
    ["In Programmen wählst du immer einen Mix, nie einen Kanal.", "In programs you always choose a mix, never a channel."],
  ],
  voicemeeter: [
    ["Auf der Mikrofon-Seite: Spiel-Ausgang = „Voicemeeter Input“, Chat-Ausgang = „Voicemeeter AUX Input“.", "On the microphone page: game output = “Voicemeeter Input”, chat output = “Voicemeeter AUX Input”."],
    ["In VoiceMeeter: „Voicemeeter Input“ nur auf B1, „AUX Input“ nur auf B2.", "In VoiceMeeter: “Voicemeeter Input” only to B1, “AUX Input” only to B2."],
    ["Das echte Mikrofon nicht auf B1 oder B2 legen.", "Do not send the real microphone to B1 or B2."],
  ],
  vbcable: [
    ["Für zwei Ausgänge brauchst du zwei Kabel, z. B. „CABLE“ und „CABLE-A“.", "For two outputs you need two cables, e.g. “CABLE” and “CABLE-A”."],
    ["Auf der Mikrofon-Seite: Spiel-Ausgang = „CABLE Input“, Chat-Ausgang = „CABLE-A Input“.", "On the microphone page: game output = “CABLE Input”, chat output = “CABLE-A Input”."],
  ],
  andere: [
    ["Du brauchst zwei virtuelle Geräte: eins für Spiele, eins für den Chat.", "You need two virtual devices: one for games, one for chat."],
    ["Auf der Mikrofon-Seite wählst du sie als Spiel- und Chat-Ausgang.", "On the microphone page, choose them as game and chat output."],
    ["Das echte Mikrofon nirgends zusätzlich einmischen.", "Do not mix the real microphone in anywhere else."],
  ],
};

/** Schritte in den Programmen. {spielEin} und {chatEin} = Gerät aus EINGAENGE. */
const PROGRAMME: { titel: Text; symbol: SymbolName; punkte: Text[] }[] = [
  {
    titel: ["Discord", "Discord"],
    symbol: "chat",
    punkte: [
      ["Eingabegerät: {chatEin}.", "Input device: {chatEin}."],
      ["Rauschunterdrückung (Krisp) aus. MaskCue entrauscht schon.", "Noise suppression (Krisp) off. MaskCue already denoises."],
      ["Echo-Unterdrückung aus.", "Echo cancellation off."],
      ["Automatische Verstärkung aus. Sonst zerhackt Discord Sounds und Musik.", "Automatic gain control off. Otherwise Discord chops sounds and music."],
    ],
  },
  {
    titel: ["TeamSpeak", "TeamSpeak"],
    symbol: "stimme",
    punkte: [
      ["Aufnahmegerät: {spielEin}.", "Capture device: {spielEin}."],
      ["Rauschfilter, Echo-Reduzierung und automatische Verstärkung aus.", "Noise filter, echo reduction and automatic gain off."],
    ],
  },
  {
    titel: ["FiveM und RedM", "FiveM and RedM"],
    symbol: "spiel",
    punkte: [
      ["Spracheingabe-Gerät: {spielEin}.", "Voice input device: {spielEin}."],
      ["Push-to-Talk: Sounds hören die anderen nur, solange du die Taste drückst.", "Push-to-talk: others only hear sounds while you hold the key."],
    ],
  },
  {
    titel: ["OBS", "OBS"],
    symbol: "laut",
    punkte: [
      ["Audio-Quelle: die Stimme, die deine Zuschauer hören sollen (meist {spielEin}).", "Audio source: the voice your viewers should hear (usually {spielEin})."],
      ["Keine zweite Mikrofon-Quelle in OBS.", "No second microphone source in OBS."],
    ],
  },
];

const SOFTWARE_NAMEN: Record<Software, Text> = {
  wavelink: ["Wave Link", "Wave Link"],
  voicemeeter: ["VoiceMeeter", "VoiceMeeter"],
  vbcable: ["VB-Cable", "VB-Cable"],
  andere: ["Andere", "Other"],
};

function lesen<T>(schluessel: string, standard: T): T {
  try {
    const w = localStorage.getItem(schluessel);
    return w === null ? standard : (JSON.parse(w) as T);
  } catch {
    return standard;
  }
}

function schreiben(schluessel: string, wert: unknown) {
  try {
    localStorage.setItem(schluessel, JSON.stringify(wert));
  } catch {
    // Ohne Speicher geht die Liste trotzdem, nur nicht über einen Neustart.
  }
}

/** Kurzer Gerätename: "MaskCue (Elgato Virtual Audio)" → "MaskCue". */
function kurz(name: string) {
  return name.replace(/\s*\(.*\)\s*$/, "") || name;
}

/** Inhalt des Reiters „Einrichtung“ in den Einstellungen. */
export function einrichtungTeil(): HTMLElement {
  const en = z.e.oberflaeche.sprache === "en";
  const tx = (t: Text) => t[en ? 1 : 0];
  let software = lesen<Software>(SPEICHER_SOFTWARE, "wavelink");
  const erledigt = new Set(lesen<string[]>(SPEICHER, []));
  const fortschritt = h("span", { class: "chip gelb zahl" });
  const inhalt = h("div", { class: "raster-2" });

  const zeichnen = () => {
    const spiel = kurz(z.e.geraete.spiel) || (en ? "Game" : "Spiel");
    const chat = kurz(z.e.geraete.chat) || "Chat";
    const [spielEin, chatEin] = EINGAENGE[software].map(tx);
    const ersetzen = (s: string) => s.split("{spielEin}").join(spielEin).split("{chatEin}").join(chatEin).split("{spiel}").join(spiel).split("{chat}").join(chat);

    const gruppen: { titel: string; symbol: SymbolName; punkte: { id: string; text: string }[] }[] = [
      { titel: tx(SOFTWARE_NAMEN[software]), symbol: "eq", punkte: SOFTWARE_SCHRITTE[software].map((p, i) => ({ id: `${software}-${i}`, text: ersetzen(tx(p)) })) },
      ...PROGRAMME.map((g, gi) => ({ titel: tx(g.titel), symbol: g.symbol, punkte: g.punkte.map((p, i) => ({ id: `${software}-p${gi}-${i}`, text: ersetzen(tx(p)) })) })),
    ];
    const alle = gruppen.flatMap((g) => g.punkte.map((p) => p.id));
    const zaehlen = () => {
      fortschritt.textContent = `${alle.filter((id) => erledigt.has(id)).length} / ${alle.length}`;
    };
    inhalt.replaceChildren(
      ...gruppen.map((g) =>
        flaeche(
          g.titel,
          g.symbol,
          h(
            "div",
            { class: "haken-liste" },
            ...g.punkte.map((p) => {
              const box = h("input", { type: "checkbox" });
              box.checked = erledigt.has(p.id);
              box.addEventListener("change", () => {
                if (box.checked) erledigt.add(p.id);
                else erledigt.delete(p.id);
                schreiben(SPEICHER, [...erledigt]);
                zaehlen();
              });
              return h("label", { class: "haken" }, box, h("span", { class: "haken-kasten" }, symbol("haken", true)), h("span", { class: "text-m", text: p.text }));
            }),
          ),
        ),
      ),
    );
    zaehlen();
  };
  zeichnen();

  const wahl = segmente<Software>({
    titel: en ? "Your audio software" : "Deine Audio-Software",
    optionen: (Object.keys(SOFTWARE_NAMEN) as Software[]).map((s) => ({ wert: s, text: tx(SOFTWARE_NAMEN[s]) })),
    wert: software,
    beiAenderung: (s) => {
      software = s;
      schreiben(SPEICHER_SOFTWARE, s);
      zeichnen();
    },
  });

  return h(
    "div",
    { class: "einstellungen-teil" },
    h(
      "div",
      { class: "seiten-kopf" },
      h("p", { class: "text-l leise", text: en ? "Tick off what is done. This way your voice arrives correctly in every program." : "Hak ab, was erledigt ist. So kommt deine Stimme richtig in jedem Programm an." }),
      fortschritt,
    ),
    h("div", { class: "block" }, h("span", { class: "etikett leise", text: en ? "Your audio software" : "Deine Audio-Software" }), wahl),
    inhalt,
  );
}
