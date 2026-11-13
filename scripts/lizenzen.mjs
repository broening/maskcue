// Baut die Liste der fremden Bausteine (3rd-Party-Software) mit ihren Lizenztexten.
// Läuft vor jedem Build (package.json, "build"). Schreibt:
//   src/lizenzen.json        für die App (Einstellungen, Lizenzen)
//   website/lizenzen.html    für die Website
// Quellen: cargo metadata (Rust), node_modules (Web), scripts/lizenzen-extra.json (Hand-Liste).

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, realpathSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const wurzel = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const LIZENZ_DATEI = /^(licen[cs]e|copying|notice|unlicense)([-._].*)?$/i;

/** Lizenztexte, jeder Text nur einmal. Die Pakete zeigen auf den Index. */
const texte = [];
const textIndex = new Map();
function textMerken(text) {
  const t = text.replace(/\r\n?/g, "\n").trim();
  if (!t) return null;
  if (!textIndex.has(t)) {
    textIndex.set(t, texte.length);
    texte.push(t);
  }
  return textIndex.get(t);
}

/** Alle Lizenzdateien in einem Ordner (nicht in Unterordnern). */
function lizenzDateien(ordner) {
  if (!existsSync(ordner)) return [];
  return readdirSync(ordner)
    .filter((n) => LIZENZ_DATEI.test(n) && statSync(join(ordner, n)).isFile())
    .sort()
    .map((n) => textMerken(readFileSync(join(ordner, n), "utf8")))
    .filter((i) => i !== null);
}

/** scripts/lizenztexte/<name>.txt, für „x-sys“ und „x-macros“ auch x.txt. */
function ersatzText(name) {
  for (const n of [name, name.replace(/-(sys|macros)$/, "")]) {
    const datei = join(wurzel, "scripts", "lizenztexte", `${n}.txt`);
    if (existsSync(datei)) return [textMerken(readFileSync(datei, "utf8"))];
  }
  return [];
}

// ---- Rust: alle normalen Abhängigkeiten, die in maskcue.exe landen ----
function rustPakete() {
  const meta = JSON.parse(
    execFileSync("cargo", ["metadata", "--format-version", "1", "--filter-platform", "x86_64-pc-windows-msvc", "--locked"], {
      cwd: join(wurzel, "src-tauri"),
      maxBuffer: 256 * 1024 * 1024,
      encoding: "utf8",
    }),
  );
  const pakete = new Map(meta.packages.map((p) => [p.id, p]));
  const knoten = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const wurzelId = meta.resolve.root;
  const gesehen = new Set([wurzelId]);
  const offen = [wurzelId];
  while (offen.length) {
    const n = knoten.get(offen.pop());
    for (const d of n.deps) {
      // Nur normale Abhängigkeiten (kind null). Build- und Test-Teile landen nicht in der App.
      if (!d.dep_kinds.some((k) => k.kind === null) || gesehen.has(d.pkg)) continue;
      gesehen.add(d.pkg);
      offen.push(d.pkg);
    }
  }
  gesehen.delete(wurzelId);
  return [...gesehen].map((id) => {
    const p = pakete.get(id);
    const ordner = dirname(p.manifest_path);
    // Teile eines Workspace (z. B. DeepFilterNet) haben die Lizenz oft eine Ebene höher.
    let t = lizenzDateien(ordner);
    if (!t.length) t = lizenzDateien(dirname(ordner));
    // Manche Pakete liefern keine Lizenzdatei mit: Text aus dem Original-Repo in scripts/lizenztexte/.
    if (!t.length) t = ersatzText(p.name);
    return {
      name: p.name,
      version: p.version,
      lizenz: p.license ?? "siehe Text",
      urheber: (p.authors ?? []).map((a) => a.replace(/\s*<[^>]*>/, "")).join(", "),
      quelle: p.repository ?? p.homepage ?? `https://crates.io/crates/${p.name}`,
      art: "rust",
      texte: t,
    };
  });
}

// ---- Web: Pakete aus "dependencies" (landen im Fenster der App) ----
function paketOrdner(name, von) {
  // pnpm: Abhängigkeiten liegen im node_modules neben dem Paket oder weiter oben.
  for (let d = von; ; d = dirname(d)) {
    const kandidat = d.endsWith("node_modules") ? join(d, name) : join(d, "node_modules", name);
    if (existsSync(join(kandidat, "package.json"))) return realpathSync(kandidat);
    if (dirname(d) === d) return null;
  }
}

function webPakete() {
  const gesehen = new Map();
  const offen = Object.keys(JSON.parse(readFileSync(join(wurzel, "package.json"), "utf8")).dependencies).map((n) => [n, wurzel]);
  while (offen.length) {
    const [name, von] = offen.pop();
    const ordner = paketOrdner(name, von);
    if (!ordner || gesehen.has(ordner)) continue;
    const p = JSON.parse(readFileSync(join(ordner, "package.json"), "utf8"));
    const repo = typeof p.repository === "string" ? p.repository : p.repository?.url;
    const autor = typeof p.author === "string" ? p.author : p.author?.name;
    gesehen.set(ordner, {
      name: p.name,
      version: p.version,
      lizenz: p.license ?? "siehe Text",
      urheber: (autor ?? "").replace(/\s*<[^>]*>/, ""),
      quelle: (repo ?? p.homepage ?? `https://www.npmjs.com/package/${p.name}`).replace(/^git\+/, "").replace(/\.git$/, ""),
      art: "web",
      texte: lizenzDateien(ordner),
    });
    for (const d of Object.keys(p.dependencies ?? {})) offen.push([d, ordner]);
  }
  return [...gesehen.values()];
}

// ---- Hand-Liste: C++, Modelle, Stimmen, Sounds ----
function extraPakete() {
  const extra = JSON.parse(readFileSync(join(wurzel, "scripts", "lizenzen-extra.json"), "utf8"));
  return extra.map(({ textDateien = [], ...e }) => ({
    version: "",
    urheber: "",
    ...e,
    texte: textDateien.map((d) => textMerken(readFileSync(join(wurzel, d), "utf8"))).filter((i) => i !== null),
  }));
}

const reihenfolge = { cpp: 0, rust: 1, web: 2, modell: 3, stimme: 4, sound: 5 };
const pakete = [...extraPakete(), ...rustPakete(), ...webPakete()].sort(
  (a, b) => reihenfolge[a.art] - reihenfolge[b.art] || a.name.localeCompare(b.name) || a.version.localeCompare(b.version),
);
const version = JSON.parse(readFileSync(join(wurzel, "package.json"), "utf8")).version;

writeFileSync(join(wurzel, "src", "lizenzen.json"), JSON.stringify({ version, pakete, texte }) + "\n");

// ---- Website ----
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
const artName = {
  cpp: "C++",
  rust: "Rust",
  web: "Web",
  modell: "Modell / Model",
  stimme: "KI-Stimme / AI voice",
  sound: "Sound",
};
const zeilen = pakete
  .map((p) => {
    const lizenz = p.lizenzUrl ? `<a href="${esc(p.lizenzUrl)}">${esc(p.lizenz)}</a>` : esc(p.lizenz);
    const textLinks = p.texte.map((i) => `<a href="#text-${i}">Text ${i + 1}</a>`).join(" ");
    return `<tr><td><a href="${esc(p.quelle)}">${esc(p.name)}</a>${p.urheber ? `<br><span class="leise">${esc(p.urheber)}</span>` : ""}</td><td>${esc(p.version)}</td><td>${esc(artName[p.art])}</td><td>${lizenz}</td><td>${textLinks}</td></tr>`;
  })
  .join("\n");
const textBloecke = texte
  .map((t, i) => `<details id="text-${i}"><summary>Text ${i + 1}</summary><pre>${esc(t)}</pre></details>`)
  .join("\n");

const html = `<!doctype html>
<html lang="de">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Lizenzen – MaskCue</title>
<meta name="robots" content="noindex">
<link rel="canonical" href="https://maskcue.com/lizenzen.html">
<meta name="theme-color" content="#060a17">
<link rel="icon" href="/favicon.png" type="image/png">
<link rel="stylesheet" href="/style.css">
</head>
<body>
<!-- Erzeugt von scripts/lizenzen.mjs. Nicht von Hand ändern. -->
<header class="kopf">
  <a class="marke" href="/de/" aria-label="MaskCue Startseite"><img src="/assets/img/logo.svg" alt="" width="40" height="40"><span>MaskCue</span></a>
  <a class="knopf klein" href="/de/" style="margin-left:auto">Zur Startseite</a>
</header>
<main class="text-seite breit">
  <h1>Lizenzen <span class="leise">/ Licenses</span></h1>
  <p>MaskCue ${esc(version)} ist freie Software unter der <a href="https://www.gnu.org/licenses/gpl-3.0.html">GNU General Public License 3.0</a>. MaskCue nutzt diese ${pakete.length} fremden Bausteine. Danke an alle, die sie gebaut haben.</p>
  <p lang="en">MaskCue ${esc(version)} is free software under the GNU General Public License 3.0. It uses these ${pakete.length} third-party components. Thanks to everyone who built them.</p>
  <div class="tabelle-rahmen">
  <table class="lizenz-tabelle">
  <thead><tr><th>Name</th><th>Version</th><th>Art / Type</th><th>Lizenz / License</th><th>Lizenztext / License text</th></tr></thead>
  <tbody>
${zeilen}
  </tbody>
  </table>
  </div>
  <h2>Lizenztexte <span class="leise">/ License texts</span></h2>
${textBloecke}
</main>
<footer class="fuss">
  <div class="marke klein"><img src="/assets/img/logo.svg" alt="" width="28" height="28"><span>MaskCue</span></div>
  <nav aria-label="Rechtliches und Links"><a href="/de/">Startseite</a><a href="/impressum.html">Impressum</a><a href="/datenschutz.html">Datenschutz</a><a href="/lizenzen.html">Lizenzen</a></nav>
</footer>
<script src="/statistik.js" defer></script>
</body>
</html>
`;
// Die Website liegt nicht in jedem Checkout (z. B. im öffentlichen Repo).
if (existsSync(join(wurzel, "website"))) writeFileSync(join(wurzel, "website", "lizenzen.html"), html);

const ohneText = pakete.filter((p) => !p.texte.length && !p.lizenzUrl);
console.log(`Lizenzen: ${pakete.length} Bausteine, ${texte.length} Texte.`);
if (ohneText.length) console.log(`Ohne Lizenztext: ${ohneText.map((p) => `${p.name} ${p.version}`).join(", ")}`);
