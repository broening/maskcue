// Fehler melden (Dan, 21.10.2026): Tester schicken Fehler über Discord. Der
// Knopf kopiert einen kurzen Bericht in die Zwischenablage und öffnet den
// Discord-Server. Der Tester fügt ihn dort im Kanal #fehler ein.
//
// Im Bericht stehen keine Dateipfade und keine Namen von Sounds oder
// Aufnahmen: Pfade enthalten oft den Windows-Benutzernamen.

import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { mitTauri } from "./api";
import { t } from "./i18n";
import { letzteMeldungen, melden } from "./ui/bausteine";
import { z } from "./zustand";

/** Einladung zum MaskCue-Discord. Leer: Der Knopf kopiert nur den Bericht. */
export const DISCORD_LINK = "https://discord.gg/yhBvJD6Wa";

/** Windows-Version aus der Browser-Kennung (z. B. „Windows NT 10.0; Win64; x64“). */
function system(): string {
  return navigator.userAgent.match(/\(([^)]*)\)/)?.[1] ?? navigator.userAgent;
}

/**
 * Pfade aus einem Text entfernen. Meldungen und Fehler aus Rust enthalten oft
 * ganze Pfade (`C:\Users\<Name>\…\Aufnahme 3.wav: …`). Ein Pfad mit Endung
 * wird zu `<Datei>`, ein Ordner-Pfad zu `<Ordner>`. Auch `\\server\…`.
 * Links (`https://…`) bleiben, weil vor dem Laufwerk kein Buchstabe stehen darf.
 */
export function ohnePfade(text: string): string {
  const anfang = String.raw`(?:(?<![A-Za-z0-9])[A-Za-z]:[\\/]|\\\\)`;
  const ordner = String.raw`(?:[^\\/\n]*[\\/])*`;
  return text
    .replace(new RegExp(anfang + ordner + String.raw`[^\\/\n]*?\.[A-Za-z0-9]{1,5}\b`, "g"), "<Datei>")
    .replace(new RegExp(anfang + ordner + String.raw`[^\\/\s]*`, "g"), "<Ordner>");
}

export function berichtText(): string {
  const e = z.e;
  const s = z.status;
  const f = e.figuren.find((x) => x.name === s?.figur);
  const figur = f ? `${f.name} (Verfahren ${f.verfahren ?? "psola"}${f.ki_an && f.ki ? `, KI ${f.ki.modell}` : ""}${f.umgebung ? `, Umgebung ${f.umgebung}` : ""})` : "keine";
  const ausgaenge = (s?.ausgaenge ?? []).map((a) => `${a.name}: ${a.latenz_ms === null ? "?" : Math.round(a.latenz_ms)} ms, ${a.aussetzer} Aussetzer`).join("; ") || "–";
  const meldungen = letzteMeldungen().map((m) => `- ${m.zeit} ${ohnePfade(m.text)}`);
  return [
    "**MaskCue-Fehlerbericht**",
    "Was ist passiert? (bitte hier beschreiben)",
    "",
    "Was hast du davor gemacht?",
    "",
    "```",
    `Version: ${z.version || "?"}`,
    `System: ${system()}`,
    `Zeit: ${new Date().toISOString()}`,
    `Eingang: ${e.geraete.eingang || "–"} | Spiel: ${e.geraete.spiel || "–"} | Chat: ${e.geraete.chat || "–"} | Sound-Ausgang: ${e.oberflaeche.ui_toene_geraet || "–"}`,
    `Ton läuft: ${s?.laeuft ? "ja" : "nein"}${s?.fehler ? ` (Fehler: ${ohnePfade(s.fehler)})` : ""}`,
    `Ausgänge: ${ausgaenge}`,
    `Figur: ${figur}`,
    `Stumm: ${s?.stumm ? "ja" : "nein"}`,
    "Letzte Meldungen:",
    ...(meldungen.length ? meldungen : ["- keine"]),
    "```",
  ].join("\n");
}

/** Bericht kopieren und den Discord-Server öffnen. */
export async function fehlerMelden() {
  try {
    // In der App über Tauri (die Zwischenablage des Browsers ist dort nicht
    // sicher freigegeben), in der Vorschau über den Browser.
    if (mitTauri) await writeText(berichtText());
    else await navigator.clipboard.writeText(berichtText());
  } catch (f) {
    return melden(`${t("bug.kopierenFehler")} ${String(f)}`);
  }
  if (!DISCORD_LINK) return melden(t("bug.kopiert"), 8000);
  melden(t("bug.kopiertDiscord"), 8000);
  if (!mitTauri) return;
  try {
    await openUrl(DISCORD_LINK);
  } catch (f) {
    melden(String(f));
  }
}
