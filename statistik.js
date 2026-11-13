// Statistik mit PostHog (EU-Server Frankfurt), ohne Cookies und ohne
// Speicher im Browser (cookieless mode). Keine Aufzeichnung von Sitzungen,
// keine automatischen Klicks: nur die Ereignisse unten mit klaren Namen.
// Wer im Datenschutz „Statistik ausschalten“ wählt, wird nicht gezählt.
(function () {
  "use strict";
  var AUS = "maskcue_statistik_aus";
  var aus = false;
  try { aus = localStorage.getItem(AUS) === "1"; } catch (e) {}
  if (aus || navigator.doNotTrack === "1" || navigator.globalPrivacyControl === true) return;

  // Offizieller Lade-Code von PostHog (posthog-js, array.js).
  !function(t,e){var o,n,p,r;e.__SV||(window.posthog=e,e._i=[],e.init=function(i,s,a){function g(t,e){var o=e.split(".");2==o.length&&(t=t[o[0]],e=o[1]),t[e]=function(){t.push([e].concat(Array.prototype.slice.call(arguments,0)))}}(p=t.createElement("script")).type="text/javascript",p.crossOrigin="anonymous",p.async=!0,p.src=s.api_host.replace(".i.posthog.com","-assets.i.posthog.com")+"/static/array.js",(r=t.getElementsByTagName("script")[0]).parentNode.insertBefore(p,r);var u=e;for(void 0!==a?u=e[a]=[]:a="posthog",u.people=u.people||[],u.toString=function(t){var e="posthog";return"posthog"!==a&&(e+="."+a),t||(e+=" (stub)"),e},u.people.toString=function(){return u.toString(1)+".people (stub)"},o="init capture register register_once unregister identify reset get_distinct_id set_config opt_in_capturing opt_out_capturing has_opted_out_capturing".split(" "),n=0;n<o.length;n++)g(u,o[n]);e._i.push([i,s,a])},e.__SV=1)}(document,window.posthog||[]);

  window.posthog.init("phc_qHDV9EEeU4CPzYxRKYFGRn2BXUzockZz4zWNp2ocMN7x", {
    api_host: "https://eu.i.posthog.com",
    ui_host: "https://eu.posthog.com",
    cookieless_mode: "always",
    person_profiles: "identified_only",
    autocapture: false,
    capture_pageview: true,
    // Datensparsam: kein Verlassen-Ereignis (Verweildauer, Scrolltiefe),
    // keine Kampagnen-Parameter, keine Feature-Flags (die senden die ganze
    // Adresse mit). Siehe datenschutz.html.
    capture_pageleave: false,
    save_campaign_params: false,
    advanced_disable_flags: true,
    disable_session_recording: true,
    disable_surveys: true,
    capture_dead_clicks: false,
    capture_performance: false,
    enable_heatmaps: false,
    // „Statistik ausschalten“ gilt sofort, auch auf dieser Seite und in
    // anderen offenen Tabs. posthog.opt_out_capturing() tut im cookieless
    // mode nichts, darum wird jedes Ereignis vor dem Senden geprüft.
    before_send: function (ereignis) {
      try { if (localStorage.getItem(AUS) === "1") return null; } catch (e) {}
      // Adressen ohne ?… und #…: Dort können Namen, E-Mails oder Codes stehen.
      var p = ereignis.properties || {};
      ["$current_url", "$referrer"].forEach(function (k) {
        if (typeof p[k] === "string" && p[k].indexOf("http") === 0) {
          try { var u = new URL(p[k]); p[k] = u.origin + u.pathname; } catch (e) {}
        }
      });
      return ereignis;
    }
    // „Do Not Track“ und „Global Privacy Control“ prüft dieses Skript oben
    // selbst. PostHogs eigenes respect_dnt schaltet im cookieless mode alles ab.
  });

  var sprache = document.documentElement.lang || "de";
  function senden(name, werte) {
    var w = werte || {};
    w.sprache = sprache;
    window.posthog.capture(name, w);
  }
  function abschnitt(el) {
    var s = el.closest("section[id], header, footer");
    return s ? (s.id || s.tagName.toLowerCase()) : "";
  }

  document.addEventListener("click", function (e) {
    var a = e.target.closest("a, button, summary");
    if (!a) return;
    var href = a.getAttribute("href") || "";
    if (href.indexOf("github.com/broening/maskcue/releases") >= 0) return senden("download_geklickt", { ort: abschnitt(a) });
    if (href.indexOf("discord.gg") >= 0) return senden("discord_geklickt", { ort: abschnitt(a) });
    if (href.indexOf("paypalme/broening/") >= 0) return senden("kaffee_geklickt", { betrag: Number(href.replace(/^.*\/(\d+)EUR$/, "$1")), eigener_betrag: false });
    if (a.closest(".sprachen") && a.getAttribute("aria-current") !== "page") return senden("sprache_gewechselt", { nach: a.getAttribute("hreflang") });
    if (a.id === "spieler-start") return senden(document.getElementById("spieler").classList.contains("laeuft") ? "hoerprobe_angehalten" : "hoerprobe_gestartet");
    if (a.hasAttribute("data-stimme")) return senden("hoerprobe_stimme", { stimme: a.getAttribute("data-stimme") });
    if (a.hasAttribute("data-umgebung")) return senden("hoerprobe_umgebung", { umgebung: a.getAttribute("data-umgebung") });
    if (a.tagName === "SUMMARY" && !a.parentElement.open) return senden("frage_geoeffnet", { frage: a.textContent.trim() });
    // App-Vorschau: nur einmal je Bildschirm zählen, nicht jeden Klick.
    var f = a.closest(".app-fenster");
    if (f && !f.hasAttribute("data-gezaehlt")) {
      f.setAttribute("data-gezaehlt", "");
      return senden("app_vorschau_benutzt", { fenster: abschnitt(a) });
    }
  }, true);

  // Eigener Kaffee-Betrag (nur gültige Beträge, wie im Formular).
  var form = document.querySelector("form[data-paypal]");
  if (form) {
    form.addEventListener("submit", function () {
      var n = Math.round(Number(form.querySelector("input").value));
      if (n >= 1 && n <= 999) senden("kaffee_geklickt", { betrag: n, eigener_betrag: true });
    }, true);
  }
})();
