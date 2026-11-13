// Mitgelieferte Avatare (KI-Bilder, Herkunft: kunst/HERKUNFT.md) und eigene Bilder.

import { convertFileSrc } from "@tauri-apps/api/core";
import { mitTauri } from "./api";
import { z } from "./zustand";

const dateien = import.meta.glob("./assets/avatare/*.webp", { eager: true, query: "?url", import: "default" }) as Record<string, string>;

const NAMEN: Record<string, [string, string]> = {
  affe: ["Affe", "Monkey"],
  alien: ["Alien", "Alien"],
  baer: ["Bär", "Bear"],
  daemon: ["Dämon", "Demon"],
  drache: ["Drache", "Dragon"],
  elfin: ["Elfin", "Elf"],
  engel: ["Engel", "Angel"],
  eule: ["Eule", "Owl"],
  fee: ["Fee", "Fairy"],
  frosch: ["Frosch", "Frog"],
  fuchs: ["Fuchs", "Fox"],
  geist: ["Geist", "Ghost"],
  hexe: ["Hexe", "Witch"],
  kobold: ["Kobold", "Goblin"],
  krankenschwester: ["Krankenschwester", "Nurse"],
  loewe: ["Löwe", "Lion"],
  medusa: ["Medusa", "Medusa"],
  moench: ["Mönch", "Monk"],
  nonne: ["Nonne", "Nun"],
  oma: ["Oma", "Grandma"],
  opa: ["Opa", "Grandpa"],
  ork: ["Ork", "Orc"],
  papagei: ["Papagei", "Parrot"],
  pirat: ["Pirat", "Pirate"],
  prinzessin: ["Prinzessin", "Princess"],
  ritter: ["Ritter", "Knight"],
  roboter: ["Roboter", "Robot"],
  sukkubus: ["Sukkubus", "Succubus"],
  tiger: ["Tiger", "Tiger"],
  vampir: ["Vampir", "Vampire"],
  wolf: ["Wolf", "Wolf"],
  zombie: ["Zombie", "Zombie"],
};

export interface Avatar {
  id: string;
  name: string;
  url: string;
}

export function avatare(): Avatar[] {
  const en = z.e?.oberflaeche.sprache === "en";
  return Object.entries(dateien)
    .map(([pfad, url]) => {
      const id = pfad.split("/").pop()!.replace(".webp", "");
      return { id, name: NAMEN[id]?.[en ? 1 : 0] ?? id, url };
    })
    .sort((a, b) => a.name.localeCompare(b.name));
}

let bilderOrdner = "";
export function bilderOrdnerSetzen(o: string) {
  bilderOrdner = o;
}

/** Bild-Adresse einer Figur. Ohne eigenes Bild: Avatar mit gleichem Namen, wenn es einen gibt. */
export function bildUrl(bild: string | undefined, figurName?: string): string | null {
  if (bild?.startsWith("avatar:")) return dateien[`./assets/avatare/${bild.slice(7)}.webp`] ?? null;
  if (bild?.startsWith("datei:")) {
    // Vorschau im Browser: ein mitgeliefertes Bild als Ersatz für eigene Bilder.
    if (!mitTauri) return dateien["./assets/avatare/eule.webp"] ?? null;
    if (!bilderOrdner) return null;
    return convertFileSrc(`${bilderOrdner}\\${bild.slice(6)}`);
  }
  if (figurName) {
    const n = figurName.toLowerCase();
    const treffer = Object.entries(NAMEN).find(([id, [de, en]]) => id === n || de.toLowerCase() === n || en.toLowerCase() === n);
    if (treffer) return dateien[`./assets/avatare/${treffer[0]}.webp`] ?? null;
  }
  return null;
}
