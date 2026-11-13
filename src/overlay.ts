// Overlay über dem Spiel, so klein wie möglich (Dan): eine schmale Pille mit
// Bild und Name der Figur, dazu ein Zeichen, wenn Sounds (Anzahl) oder Musik
// laufen. Normale Stimme ohne Sounds: nur das kleine Bild.
// Läuft im Fenster "overlay" (lib.rs, overlay_abgleichen). Klicks gehen durch.

import { api } from "./api";
import { bilderOrdnerSetzen } from "./avatare";
import { spracheSetzen, t } from "./i18n";
import { symbol } from "./icons";
import { h } from "./ui/dom";
import { bildVon } from "./ui/formen";
import type { Einstellungen, Status } from "./typen";

export async function overlayStarten() {
  document.documentElement.classList.add("overlay-modus");
  const start = await api.start();
  let e: Einstellungen = start.einstellungen;
  bilderOrdnerSetzen(start.bilder_ordner);
  spracheSetzen(e.oberflaeche.sprache);

  const bild = h("span", { class: "ov-bild" });
  const name = h("span", { class: "ov-name" });
  const sounds = h("span", { class: "ov-zeichen sounds", hidden: true });
  const musik = h("span", { class: "ov-zeichen musik", hidden: true }, symbol("musik", true));
  const stumm = h("span", { class: "ov-zeichen stumm", hidden: true }, symbol("stumm", true));
  const umgebung = h("span", { class: "ov-zeichen umgebung", hidden: true });
  const aufnahme = h("span", { class: "ov-zeichen aufnahme", hidden: true }, symbol("aufnahme", true));
  const pille = h("div", { class: "ov-pille" }, bild, name, aufnahme, stumm, umgebung, sounds, musik);
  const ecke = () => document.body.setAttribute("data-ecke", e.oberflaeche.overlay_ecke || "oben-rechts");
  ecke();
  document.body.append(pille);

  let figur: string | null | undefined;
  let stand = "";
  const zeichnen = (s: Status) => {
    // Rahmen: grün beim Reden, rot wenn stumm (MaskCue-Stummtaste).
    pille.classList.toggle("spricht", s.spricht);
    pille.classList.toggle("ist-stumm", s.stumm);
    stumm.hidden = !s.stumm;
    aufnahme.hidden = s.aufnahme_s === null;
    if (s.figur !== figur) {
      figur = s.figur;
      const i = s.figur ? e.figuren.findIndex((f) => f.name === s.figur) : -1;
      const f = i >= 0 ? e.figuren[i] : null;
      bild.replaceChildren(f ? bildVon(i + 1, f.bild, f.name, f.bild_ausschnitt) : bildVon(0, e.normal_bild, t("ueb.normal"), e.normal_ausschnitt));
      name.textContent = s.figur ?? t("ueb.normal");
      pille.animate([{ scale: 1 }, { scale: 1.08, offset: 0.4 }, { scale: 1 }], { duration: 380, easing: "ease-out" });
    }
    const musikNamen = new Set(e.sounds.filter((x) => x.musik).map((x) => x.name));
    const anzahl = s.sounds_laufen.filter((n) => !musikNamen.has(n)).length;
    const mitMusik = s.sounds_laufen.some((n) => musikNamen.has(n));
    const neu = `${anzahl}|${mitMusik}|${!!s.figur}|${s.stumm}|${s.umgebung}`;
    if (neu === stand) return;
    stand = neu;
    sounds.hidden = anzahl === 0;
    sounds.replaceChildren(h("span", { class: "eq" }, h("i"), h("i"), h("i")), anzahl > 1 ? String(anzahl) : "");
    musik.hidden = !mitMusik;
    umgebung.hidden = !s.umgebung;
    umgebung.replaceChildren(...(s.umgebung ? [symbol(s.umgebung, true)] : []));
    // Normale Stimme und nichts läuft: nur das Bild, kein Name.
    pille.classList.toggle("klein", !s.figur && anzahl === 0 && !mitMusik && !s.umgebung);
  };

  api.aufStatus(zeichnen);
  api.aufEinstellungen((neu) => {
    e = neu;
    spracheSetzen(e.oberflaeche.sprache);
    ecke();
    figur = undefined;
    stand = "";
  });
}
