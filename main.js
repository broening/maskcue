// MaskCue – Website: Figuren-Bühne oben und sanftes Auftauchen der Abschnitte.

(function () {
  "use strict";

  // Werte wie in der App (Start-Figuren und Bilder aus src/assets/avatare).
  // Die Seite gibt es auf Deutsch (/de/) und Englisch (/en/).
  var EN = document.documentElement.lang === "en";
  var FIGUREN = EN ? [
    { name: "Demon", bild: "daemon", ton: "Fast · −7 semitones", umgebung: "Stalactite cave" },
    { name: "Goblin", bild: "kobold", ton: "Fast · +5 semitones", umgebung: "no space" },
    { name: "Cyborg", bild: "roboter", ton: "Fast · −2 semitones", umgebung: "Robot" },
    { name: "Ghost", bild: "geist", ton: "Natural · +2 semitones", umgebung: "Large building" },
    { name: "Witch", bild: "hexe", ton: "Fast · +3 semitones", umgebung: "Telepathy" },
    { name: "Pirate", bild: "pirat", ton: "Fast · −3 semitones", umgebung: "Megaphone" },
    { name: "Alien", bild: "alien", ton: "Light · +6 semitones", umgebung: "Spaceship" },
    { name: "Princess", bild: "prinzessin", ton: "Natural · +4 semitones", umgebung: "no space" }
  ] : [
    { name: "Dämon", bild: "daemon", ton: "Schnell · −7 Halbtöne", umgebung: "Tropfsteinhöhle" },
    { name: "Goblin", bild: "kobold", ton: "Schnell · +5 Halbtöne", umgebung: "ohne Umgebung" },
    { name: "Cyborg", bild: "roboter", ton: "Schnell · −2 Halbtöne", umgebung: "Roboter" },
    { name: "Geist", bild: "geist", ton: "Natürlich · +2 Halbtöne", umgebung: "Großes Gebäude" },
    { name: "Hexe", bild: "hexe", ton: "Schnell · +3 Halbtöne", umgebung: "Telepathie" },
    { name: "Pirat", bild: "pirat", ton: "Schnell · −3 Halbtöne", umgebung: "Megafon" },
    { name: "Alien", bild: "alien", ton: "Leicht · +6 Halbtöne", umgebung: "Raumschiff" },
    { name: "Prinzessin", bild: "prinzessin", ton: "Natürlich · +4 Halbtöne", umgebung: "ohne Umgebung" }
  ];
  var FIGUR = EN ? "Character " : "Figur ";
  var pfad = function (f) { return "/assets/img/figuren/" + f.bild + ".webp"; };

  var masken = document.getElementById("masken");
  var wahl = document.getElementById("wahl");
  var name = document.getElementById("figur-name");
  var ton = document.getElementById("figur-ton");
  var umgebung = document.getElementById("figur-umgebung");
  var buehne = document.getElementById("buehne");
  if (!masken || !wahl) return;

  var ruhig = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var aktuell = 0;
  var vorne = masken.querySelector(".maske.vorne");
  var knoepfe = [];

  function zeigen(i) {
    if (i === aktuell) return;
    aktuell = i;
    var f = FIGUREN[i];
    var karte = document.createElement("div");
    karte.className = "maske vorne";
    var bild = document.createElement("img");
    bild.src = pfad(f);
    bild.alt = FIGUR + f.name;
    bild.width = 512;
    bild.height = 512;
    bild.decoding = "async";
    karte.appendChild(bild);
    masken.insertBefore(karte, masken.firstChild);
    // Die alte Maske liegt darüber und fällt nach hinten weg.
    if (vorne) {
      var alt = vorne;
      alt.classList.remove("vorne");
      alt.classList.add("hinten");
      setTimeout(function () { if (alt.parentNode) alt.parentNode.removeChild(alt); }, 900);
    }
    vorne = karte;
    name.textContent = f.name;
    ton.textContent = f.ton;
    umgebung.textContent = f.umgebung;
    knoepfe.forEach(function (k, j) { k.setAttribute("aria-pressed", String(j === i)); });
  }

  FIGUREN.forEach(function (f, i) {
    var k = document.createElement("button");
    k.type = "button";
    k.setAttribute("aria-label", f.name);
    k.setAttribute("aria-pressed", String(i === 0));
    var b = document.createElement("img");
    b.src = pfad(f);
    b.alt = "";
    b.width = 52;
    b.height = 52;
    b.loading = "lazy";
    k.appendChild(b);
    k.addEventListener("click", function () { zeigen(i); halten = true; });
    wahl.appendChild(k);
    knoepfe.push(k);
  });

  // Von selbst weiter, bis jemand selbst wählt oder mit der Maus darauf zeigt.
  var halten = false;
  var zeigt = false;
  buehne.addEventListener("pointerenter", function () { zeigt = true; });
  buehne.addEventListener("pointerleave", function () { zeigt = false; });
  buehne.addEventListener("focusin", function () { zeigt = true; });
  buehne.addEventListener("focusout", function () { zeigt = false; });
  if (!ruhig) {
    setInterval(function () {
      if (halten || zeigt || document.hidden) return;
      zeigen((aktuell + 1) % FIGUREN.length);
    }, 3200);
  }

  // Abschnitte tauchen beim Scrollen auf.
  if (!ruhig && "IntersectionObserver" in window) {
    var teile = document.querySelectorAll(".abschnitt > *, .schluss > *");
    var io = new IntersectionObserver(function (eintraege) {
      eintraege.forEach(function (e) {
        if (e.isIntersecting) {
          e.target.classList.add("da");
          io.unobserve(e.target);
        }
      });
    }, { rootMargin: "0px 0px -8% 0px" });
    teile.forEach(function (t) {
      t.classList.add("auftauchen");
      io.observe(t);
    });
  }
})();

// Hörprobe: drei Stimmen je Umgebung laufen gleichzeitig, nur eine ist laut.
// Ein Wechsel der Stimme blendet um, ein Wechsel der Umgebung startet die
// neuen drei Spuren an derselben Stelle. Spuren laden erst bei Bedarf.
(function () {
  "use strict";
  var wurzel = document.getElementById("spieler");
  if (!wurzel || !(window.AudioContext || window.webkitAudioContext)) return;
  var STIMMEN = ["original", "thorsten", "w1"];
  var EN = document.documentElement.lang === "en";
  var SPIELEN = EN ? "Play" : "Abspielen";
  var ANHALTEN = EN ? "Pause" : "Anhalten";
  var LAENGE = 30.2; // erste Strophe (27,2 s) plus Nachhall, in Sekunden
  var start = document.getElementById("spieler-start");
  var stand = document.getElementById("spieler-stand");
  var zeit = document.getElementById("spieler-zeit");
  var ctx = null;
  var roh = {}; // Umgebung -> Promise<ArrayBuffer[]>
  var fertig = {}; // Umgebung -> Promise<AudioBuffer[]>
  var stimme = "original";
  var umgebung = "keine";
  var lauf = null; // { umgebung, spuren: [{quelle, laut}], null: ctx-Zeit bei Stelle 0 }
  var stelle = 0;

  function holen(u) {
    if (!roh[u]) {
      roh[u] = Promise.all(STIMMEN.map(function (v) {
        return fetch("/assets/hoerprobe/" + u + "/" + v + ".mp3").then(function (r) {
          if (!r.ok) throw new Error(r.status);
          return r.arrayBuffer();
        });
      }));
    }
    return roh[u];
  }
  function dekodiert(u) {
    if (!fertig[u]) {
      fertig[u] = holen(u).then(function (teile) {
        return Promise.all(teile.map(function (t) {
          return new Promise(function (ja, nein) { ctx.decodeAudioData(t.slice(0), ja, nein); });
        }));
      });
    }
    return fertig[u];
  }
  function jetzt() {
    return lauf ? Math.max(0, Math.min(LAENGE, ctx.currentTime - lauf.null)) : stelle;
  }
  function starten(u, puffer, ab, bei, einblenden) {
    var spuren = puffer.map(function (b, i) {
      var q = ctx.createBufferSource();
      q.buffer = b;
      var g = ctx.createGain();
      var ziel = STIMMEN[i] === stimme ? 1 : 0;
      g.gain.setValueAtTime(einblenden ? 0 : ziel, bei);
      if (einblenden) g.gain.linearRampToValueAtTime(ziel, bei + 0.04);
      q.connect(g).connect(ctx.destination);
      q.start(bei, Math.min(ab, b.duration - 0.01));
      return { quelle: q, laut: g };
    });
    return { umgebung: u, spuren: spuren, null: bei - ab };
  }
  function anhalten(l, bei) {
    l.spuren.forEach(function (s) {
      s.laut.gain.cancelScheduledValues(bei);
      s.laut.gain.setValueAtTime(s.laut.gain.value, bei);
      s.laut.gain.linearRampToValueAtTime(0, bei + 0.04);
      try { s.quelle.stop(bei + 0.05); } catch (e) {}
    });
  }
  function zeichnen() {
    var t = jetzt();
    stand.style.transform = "scaleX(" + (t / LAENGE) + ")";
    zeit.textContent = "0:" + String(Math.floor(t)).padStart(2, "0");
    if (lauf) {
      if (t >= LAENGE - 0.05) return stoppen(true);
      requestAnimationFrame(zeichnen);
    }
  }
  function stoppen(ende) {
    if (lauf) {
      stelle = ende ? 0 : jetzt();
      anhalten(lauf, ctx.currentTime);
      lauf = null;
    }
    wurzel.classList.remove("laeuft");
    start.setAttribute("aria-label", SPIELEN);
    zeichnen();
  }
  function laden(an) {
    wurzel.classList.toggle("laedt", an);
  }

  var startet = false;
  start.addEventListener("click", function () {
    if (lauf) return stoppen(false);
    if (startet) return;
    startet = true;
    if (!ctx) ctx = new (window.AudioContext || window.webkitAudioContext)();
    // Im Klick freigeben (Autoplay-Regel), aber nicht darauf warten.
    ctx.resume();
    laden(true);
    dekodiert(umgebung).then(function (puffer) {
      startet = false;
      laden(false);
      lauf = starten(umgebung, puffer, stelle, ctx.currentTime + 0.05, false);
      wurzel.classList.add("laeuft");
      start.setAttribute("aria-label", ANHALTEN);
      zeichnen();
    }).catch(function () { startet = false; laden(false); });
  });

  function wahl(gruppe, attr, setzen) {
    var knoepfe = document.querySelectorAll("#" + gruppe + " button");
    knoepfe.forEach(function (k) {
      k.addEventListener("click", function () {
        knoepfe.forEach(function (x) { x.setAttribute("aria-pressed", String(x === k)); });
        setzen(k.getAttribute(attr));
      });
    });
  }
  wahl("wahl-stimme", "data-stimme", function (v) {
    stimme = v;
    if (!lauf) return;
    var t = ctx.currentTime;
    lauf.spuren.forEach(function (s, i) {
      s.laut.gain.cancelScheduledValues(t);
      s.laut.gain.setTargetAtTime(STIMMEN[i] === v ? 1 : 0, t, 0.015);
    });
  });
  wahl("wahl-umgebung", "data-umgebung", function (u) {
    umgebung = u;
    if (!ctx) { holen(u); return; }
    if (!lauf) { dekodiert(u); return; }
    laden(true);
    dekodiert(u).then(function (puffer) {
      laden(false);
      if (!lauf || umgebung !== u || lauf.umgebung === u) return;
      var bei = ctx.currentTime + 0.05;
      var ab = bei - lauf.null;
      anhalten(lauf, bei);
      lauf = starten(u, puffer, ab, bei, true);
    }).catch(function () { laden(false); });
  });

  // Erste Spuren vorladen, sobald der Spieler in die Nähe kommt.
  if ("IntersectionObserver" in window) {
    var io = new IntersectionObserver(function (e) {
      if (e[0].isIntersecting) { io.disconnect(); holen("keine"); }
    }, { rootMargin: "400px" });
    io.observe(wurzel);
  }
})();
