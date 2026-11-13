// Figuren-Karten mit gemalter Szene. Jede Figur hat ihre eigene Landschaft
// (Himmel, Mond, Berge oder Wellen), berechnet aus ihrem Platz in der Liste.
// Keine Bilddateien: alles ist SVG und skaliert scharf.

import { symbol } from "../icons";
import { h } from "./dom";
import { bildUrl } from "../avatare";
import type { Ausschnitt } from "../typen";

const NS = "http://www.w3.org/2000/svg";

interface Stil {
  himmel: [string, string, string];
  mond: string;
  fern: string;
  nah: string;
  art: "wolken" | "berge" | "wellen" | "zacken" | "wald";
  sterne: boolean;
}

// Platz 0 = normale Stimme (Wolken in der Dämmerung), dann je Figur eine Szene.
const STILE: Stil[] = [
  { himmel: ["#7c3aed", "#db2777", "#fb923c"], mond: "#ffe8cc", fern: "#f9a8d4", nah: "#fdba74", art: "wolken", sterne: false },
  { himmel: ["#4c1d95", "#c026d3", "#fb7185"], mond: "#fff1d6", fern: "#7e22ce", nah: "#3b0764", art: "berge", sterne: true },
  { himmel: ["#450a0a", "#b91c1c", "#f97316"], mond: "#fde68a", fern: "#7f1d1d", nah: "#1c0606", art: "zacken", sterne: false },
  { himmel: ["#1e1b4b", "#2563eb", "#7dd3fc"], mond: "#e0f2fe", fern: "#1e3a8a", nah: "#0b1437", art: "wellen", sterne: true },
  { himmel: ["#052e16", "#047857", "#a3e635"], mond: "#fef9c3", fern: "#065f46", nah: "#022c22", art: "wald", sterne: true },
  { himmel: ["#2e1065", "#6d28d9", "#f0abfc"], mond: "#faf5ff", fern: "#581c87", nah: "#1e0a3c", art: "berge", sterne: true },
  { himmel: ["#172554", "#7c3aed", "#f472b6"], mond: "#fdf4ff", fern: "#3b0764", nah: "#0f0624", art: "wellen", sterne: true },
  { himmel: ["#431407", "#ea580c", "#fde047"], mond: "#fffbeb", fern: "#9a3412", nah: "#2a0a02", art: "wald", sterne: false },
];

/** Einfacher, fester Zufall: gleiche Figur = gleiche Szene. */
function zufall(saat: number) {
  let s = saat * 9301 + 49297;
  return () => {
    s = (s * 9301 + 49297) % 233280;
    return s / 233280;
  };
}

function el(tag: string, attr: Record<string, string | number>) {
  const e = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attr)) e.setAttribute(k, String(v));
  return e;
}

function linie(r: () => number, basis: number, huegel: number, zacken: boolean) {
  const punkte: string[] = [];
  const schritte = zacken ? 9 : 6;
  for (let i = 0; i <= schritte; i++) {
    const x = (i / schritte) * 160;
    const y = basis - r() * huegel;
    punkte.push(`${x.toFixed(1)} ${y.toFixed(1)}`);
  }
  if (zacken) return `M0 120 L${punkte.join(" L")} L160 120Z`;
  // Weich: Kurven durch die Punkte
  let d = `M0 120 L${punkte[0]}`;
  for (let i = 1; i < punkte.length; i++) {
    const [x0, y0] = punkte[i - 1].split(" ").map(Number);
    const [x1, y1] = punkte[i].split(" ").map(Number);
    d += ` Q${x0 + (x1 - x0) / 2} ${y0} ${(x0 + x1) / 2} ${(y0 + y1) / 2} T${x1} ${y1}`;
  }
  return `${d} L160 120Z`;
}

/** Gemalte Szene für Platz `platz` (0 = normale Stimme). */
export function szene(platz: number): SVGSVGElement {
  const stil = STILE[platz === 0 ? 0 : ((platz - 1) % (STILE.length - 1)) + 1];
  const r = zufall(platz + 3);
  const svg = el("svg", { viewBox: "0 0 160 120", preserveAspectRatio: "xMidYMid slice", "aria-hidden": "true", class: "szene" }) as SVGSVGElement;
  const id = `himmel-${platz}-${Math.floor(r() * 1e6)}`;
  const defs = el("defs", {});
  const verlauf = el("linearGradient", { id, x1: 0, y1: 0, x2: 0, y2: 1 });
  stil.himmel.forEach((farbe, i) => verlauf.append(el("stop", { offset: i / 2, "stop-color": farbe })));
  defs.append(verlauf);
  svg.append(defs, el("rect", { width: 160, height: 120, fill: `url(#${id})` }));

  if (stil.sterne) {
    for (let i = 0; i < 14; i++) {
      svg.append(el("circle", { cx: (r() * 160).toFixed(1), cy: (r() * 55).toFixed(1), r: (0.4 + r() * 0.9).toFixed(2), fill: "#fff", opacity: (0.4 + r() * 0.6).toFixed(2), class: "szene-stern" }));
    }
  }
  const mx = 30 + r() * 100;
  const my = 24 + r() * 18;
  svg.append(el("circle", { cx: mx, cy: my, r: 22, fill: stil.mond, opacity: 0.18, class: "szene-schein" }));
  svg.append(el("circle", { cx: mx, cy: my, r: 13, fill: stil.mond, class: "szene-mond" }));

  if (stil.art === "wolken") {
    for (const [x, y, s] of [
      [20, 78, 1],
      [92, 70, 1.3],
      [140, 86, 0.9],
    ]) {
      const g = el("g", { fill: stil.fern, opacity: 0.85, class: "szene-wolke", transform: `translate(${x} ${y}) scale(${s})` });
      g.append(el("ellipse", { cx: 0, cy: 6, rx: 26, ry: 10 }), el("circle", { cx: -8, cy: 0, r: 11 }), el("circle", { cx: 8, cy: -3, r: 14 }));
      svg.append(g);
    }
    svg.append(el("path", { d: linie(r, 104, 10, false), fill: stil.nah }));
  } else if (stil.art === "wellen") {
    svg.append(el("path", { d: linie(r, 82, 8, false), fill: stil.fern }));
    for (let i = 0; i < 3; i++) {
      svg.append(el("path", { d: `M0 ${92 + i * 9} q20 -6 40 0 t40 0 t40 0 t40 0 V120 H0Z`, fill: stil.nah, opacity: 0.55 + i * 0.2, class: "szene-welle" }));
    }
  } else {
    const zacken = stil.art === "zacken";
    svg.append(el("path", { d: linie(r, 84, 34, zacken), fill: stil.fern }));
    svg.append(el("path", { d: linie(r, 104, 20, zacken), fill: stil.nah }));
    if (stil.art === "wald") {
      for (let i = 0; i < 7; i++) {
        const x = 8 + i * 23 + r() * 8;
        const hoehe = 22 + r() * 18;
        svg.append(el("path", { d: `M${x} ${120 - hoehe} l7 ${hoehe * 0.55} h-4 l6 ${hoehe * 0.45} h-18 l6 ${-hoehe * 0.45} h-4z`, fill: stil.nah }));
      }
    }
  }
  return svg;
}

/** Neutrales Bild der normalen Stimme: Mikrofon in ruhigen Ringen. */
function neutral(): Node {
  return h("span", { class: "neutral" }, h("span", { class: "neutral-ring" }), h("span", { class: "neutral-ring zwei" }), h("span", { class: "neutral-mitte" }, symbol("mikrofon", true)));
}

/** CSS für einen Bild-Ausschnitt: Mitte verschieben (object-position), dorthin zoomen. */
export function ausschnittStil(a?: Ausschnitt): string {
  if (!a) return "";
  return `object-position:${a.x}% ${a.y}%;transform-origin:${a.x}% ${a.y}%;transform:scale(${a.zoom})`;
}

/** Farbige Kachel mit dem Anfangsbuchstaben (Bild „farbe:#rrggbb“). */
function farbBild(farbe: string, name: string | undefined): Node {
  const zeichen = (name ?? "").trim().charAt(0).toUpperCase() || "?";
  return h("span", { class: "farbbild", style: `--farbe:${/^#[0-9a-f]{6}$/i.test(farbe) ? farbe : "#5b5ff5"}` }, h("span", { class: "farbbild-zeichen", text: zeichen }));
}

/** Bild aus dem gespeicherten Wert: Farbe, Avatar, eigenes Bild (mit Ausschnitt), sonst Szene. */
export function bildVon(platz: number, bild: string | undefined, name: string | undefined, ausschnitt?: Ausschnitt): Node {
  if (bild?.startsWith("farbe:")) return farbBild(bild.slice(6), name);
  return figurBild(platz, bildUrl(bild, platz === 0 ? undefined : name), ausschnitt);
}

/** Bild der Figur, sonst ihre gemalte Szene (Platz 0 = normale Stimme: neutral). */
export function figurBild(platz: number, url: string | null, ausschnitt?: Ausschnitt): Node {
  if (!url) return platz === 0 ? neutral() : (szene(platz) as unknown as Node);
  const img = h("img", { class: "foto", src: url, alt: "", draggable: "false", decoding: "async", style: ausschnittStil(ausschnitt) });
  // Kaputtes eigenes Bild (Datei gelöscht): Szene statt leerem Rahmen.
  img.addEventListener("error", () => img.replaceWith(platz === 0 ? neutral() : szene(platz)));
  return img;
}

export interface KachelElement extends HTMLButtonElement {
  aktiv(an: boolean): void;
  taste(t: string | null): void;
}

export function figurKachel(o: { platz: number; name: string; unterzeile: string; bild: Node; aktiv: boolean; beiKlick: () => void }): KachelElement {
  const abzeichen = h("span", { class: "kachel-taste", hidden: true });
  const el = h(
    "button",
    { class: "kachel", type: "button", onclick: () => o.beiKlick() },
    h("span", { class: "kachel-bild" }, o.bild, abzeichen, h("span", { class: "kachel-aktiv etikett" }, "AKTIV")),
    h("span", { class: "kachel-text" }, h("span", { class: "titel-s kachel-name", text: o.name }), h("span", { class: "text-s leise zahl", text: o.unterzeile })),
  ) as KachelElement;
  let zustand: boolean | null = null;
  el.aktiv = (an) => {
    if (an === zustand) return;
    // Beim Wechsel auf „aktiv“ einmal hüpfen (nicht beim ersten Zeichnen).
    if (an && zustand === false) {
      el.classList.remove("huepft");
      void el.offsetWidth;
      el.classList.add("huepft");
    }
    zustand = an;
    el.classList.toggle("aktiv", an);
    el.setAttribute("aria-pressed", String(an));
  };
  el.taste = (t) => {
    abzeichen.hidden = !t;
    abzeichen.textContent = t ?? "";
  };
  el.aktiv(o.aktiv);
  return el;
}
