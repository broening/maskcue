# Code signing policy

**English** | [Deutsch](#richtlinie-zum-signieren)

Free code signing provided by [SignPath.io](https://about.signpath.io),
certificate by [SignPath Foundation](https://signpath.org).

**Status:** applied for. Until the application is approved, the installers
are not signed with a code signing certificate. Updates inside the app are
signed separately with the updater key (see below).

## What gets signed

Only the Windows installer `MaskCue_<version>_x64-setup.exe`. It is built
automatically by GitHub Actions ([build.yml](.github/workflows/build.yml))
from the source code in this repository, at a version tag `v<version>`.
Nothing built elsewhere is signed with this certificate.

## Team roles

MaskCue is maintained by one person.

- Committers and reviewers: [broening](https://github.com/broening)
- Approvers (every signing request is approved by hand): [broening](https://github.com/broening)

All team members use multi-factor authentication for GitHub and SignPath.

## Privacy

MaskCue will not transfer any information to other networked systems
unless specifically requested by the user or the person installing or
operating it, with one exception: the update check. At startup MaskCue asks
GitHub whether there is a new version. This can be turned off in the app
(Settings → General → Updates). The AI voice is downloaded from GitHub only
when you ask for it. Details: [privacy policy](https://maskcue.com/datenschutz.html)
(German).

## Updates inside the app

The in-app updater only installs files signed with the MaskCue updater key
(minisign). The public key is in `src-tauri/tauri.conf.json`.

---

## Richtlinie zum Signieren

Kostenloses Code-Signing von [SignPath.io](https://about.signpath.io),
Zertifikat von der [SignPath Foundation](https://signpath.org).

**Stand:** beantragt. Bis der Antrag angenommen ist, sind die Installer nicht
mit einem Code-Signing-Zertifikat signiert. Updates in der App sind getrennt
davon mit dem Update-Schlüssel unterschrieben.

- Signiert wird nur der Windows-Installer. Er wird bei jedem Versions-Tag
  automatisch von GitHub Actions aus diesem Quellcode gebaut.
- Rollen: Committer, Prüfer und Freigabe: [broening](https://github.com/broening).
  Jede Signatur wird von Hand freigegeben. Alle nutzen Zwei-Faktor-Anmeldung.
- Datenschutz: MaskCue sendet nichts ins Netz, außer der Update-Suche bei
  GitHub beim Start (abschaltbar unter Einstellungen → Allgemein → Updates)
  und dem Download der KI-Stimme auf Wunsch.
  [Datenschutzerklärung](https://maskcue.com/datenschutz.html)
