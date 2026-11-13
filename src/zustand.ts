// Gemeinsamer Zustand der Oberfläche: Einstellungen, Status, Geräte.
// Änderungen gehen gedrosselt an den Motor. Der Motor speichert selbst.

import { api } from "./api";
import { gedrosselt } from "./ui/dom";
import type { Einstellungen, Geraete, Status } from "./typen";

export const z = {
  e: null as unknown as Einstellungen,
  geraete: { eingaenge: [], ausgaenge: [] } as Geraete,
  status: null as Status | null,
  version: "",
};

const senden = gedrosselt(() => void api.einstellungenSetzen(z.e), 40);

/** Einstellungen ändern und an den Motor schicken. */
export function aendern(f: (e: Einstellungen) => void) {
  f(z.e);
  senden();
}

/** Ganze Einstellungen ersetzen (z. B. nach „Sounds hinzufügen“). */
export function ersetzen(e: Einstellungen) {
  z.e = e;
}

// Status-Hörer gelten für die aktuelle Seite. Beim Seitenwechsel werden sie gelöscht.
const seitenHoerer = new Set<(s: Status) => void>();
const festeHoerer = new Set<(s: Status) => void>();

export function aufStatus(f: (s: Status) => void, fest = false) {
  (fest ? festeHoerer : seitenHoerer).add(f);
  if (z.status) f(z.status);
}

export function seitenHoererLeeren() {
  seitenHoerer.clear();
}

export function statusVerteilen(s: Status) {
  z.status = s;
  festeHoerer.forEach((f) => f(s));
  seitenHoerer.forEach((f) => f(s));
}
