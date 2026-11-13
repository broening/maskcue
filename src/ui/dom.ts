// Kleiner Helfer zum Bauen von DOM-Elementen ohne Framework.

type Kind = Node | string | null | undefined | false;
type Attribute = {
  [k: string]: string | number | boolean | EventListener | undefined | null;
};

/** `h("div", { class: "x", onclick: f }, kind1, "Text")` */
export function h<K extends keyof HTMLElementTagNameMap>(tag: K, attr: Attribute = {}, ...kinder: Kind[]): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attr)) {
    if (v === undefined || v === null || v === false) continue;
    if (k.startsWith("on") && typeof v === "function") {
      el.addEventListener(k.slice(2), v as EventListener);
    } else if (k === "text") {
      el.textContent = String(v);
    } else if (v === true) {
      el.setAttribute(k, "");
    } else {
      el.setAttribute(k, String(v));
    }
  }
  anhaengen(el, kinder);
  return el;
}

export function anhaengen(el: Element, kinder: Kind[]) {
  for (const k of kinder) {
    if (k === null || k === undefined || k === false) continue;
    el.append(typeof k === "string" ? document.createTextNode(k) : k);
  }
}

/** Ruft `f` höchstens alle `ms` Millisekunden auf, den letzten Aufruf immer. */
export function gedrosselt<A extends unknown[]>(f: (...a: A) => void, ms: number): (...a: A) => void {
  let letzte = 0;
  let wartet: ReturnType<typeof setTimeout> | null = null;
  let args: A;
  return (...a: A) => {
    args = a;
    const jetzt = performance.now();
    if (jetzt - letzte >= ms) {
      letzte = jetzt;
      f(...args);
    } else if (!wartet) {
      wartet = setTimeout(() => {
        wartet = null;
        letzte = performance.now();
        f(...args);
      }, ms - (jetzt - letzte));
    }
  };
}
