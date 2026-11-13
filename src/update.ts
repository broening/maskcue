// Hinweis-Leiste für eine neue Version. Im Modus „Nur benachrichtigen“ mit
// Knopf „Jetzt aktualisieren“; im Modus „Automatisch“ zeigt sie, dass das
// Update geladen wird. MaskCue startet danach von selbst neu.

import { api } from "./api";
import { t } from "./i18n";
import { knopf, melden } from "./ui/bausteine";
import { h } from "./ui/dom";
import type { UpdateInfo } from "./typen";

const leiste = h("div", { class: "update-leiste", role: "status", hidden: true });
let aktuell: UpdateInfo | null = null;

function zeigen(info: UpdateInfo) {
  aktuell = info;
  leiste.hidden = false;
  leiste.replaceChildren(
    h("span", { class: "update-text" }, h("strong", { text: `${t("upd.neu")} ${info.version}` }), h("span", { class: "leise", text: t("upd.neuText") })),
    h(
      "div",
      { class: "knopf-reihe" },
      knopf({ text: t("upd.jetzt"), symbol: "importieren", art: "gefuellt", beiKlick: () => void installieren() }),
      knopf({ text: t("upd.spaeter"), art: "text", beiKlick: () => (leiste.hidden = true) }),
    ),
  );
}

function laedt(version: string, anteil?: number) {
  leiste.hidden = false;
  const prozent = anteil === undefined ? "" : ` ${Math.round(anteil * 100)} %`;
  leiste.replaceChildren(h("span", { class: "update-text" }, h("strong", { text: `${t("upd.laedt")} ${version}${prozent}` }), h("span", { class: "leise", text: t("upd.laedtText") })));
}

async function installieren() {
  if (!aktuell) return;
  laedt(aktuell.version);
  try {
    await api.updateInstallieren();
  } catch (f) {
    melden(`${t("upd.fehler")} ${String(f)}`);
    zeigen(aktuell);
  }
}

/** Leiste einhängen und auf neue Versionen hören. */
export function updateAnmelden(): HTMLElement {
  api.aufUpdate((info) => zeigen(info));
  let version = "";
  api.aufUpdateLaedt((v) => {
    version = v;
    laedt(v);
  });
  api.aufUpdateFortschritt((anteil) => laedt(version || aktuell?.version || "", anteil));
  // Hat MaskCue schon vor der Oberfläche etwas gefunden?
  void api.updateStand().then((info) => info && zeigen(info));
  return leiste;
}

/** Einstellungen: „Jetzt prüfen“ zeigt ein gefundenes Update auch in der Leiste. */
export function updateZeigen(info: UpdateInfo) {
  zeigen(info);
}
