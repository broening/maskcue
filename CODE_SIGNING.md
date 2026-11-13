# Code signing policy

**English** | [Deutsch](#richtlinie-zum-signieren)

**Status:** not signed yet. The installers are not signed with a code
signing certificate, so Windows SmartScreen may warn you. Click **More info**
and then **Run anyway**. You can check the file with the SHA-256 checksum on
the [release page](https://github.com/broening/maskcue/releases/latest).
Updates inside the app are signed separately with the updater key (see below).

This policy applies as soon as the installer is signed. MaskCue applied to
the SignPath Foundation in November 2026. The application was declined for
now, because the project is still new. We will apply again later.

## What gets signed

Only the Windows installer `MaskCue_<version>_x64-setup.exe`. It is built
automatically by GitHub Actions ([build.yml](.github/workflows/build.yml))
from the source code in this repository, at a version tag `v<version>`.
Nothing built elsewhere is signed with this certificate.

## Team roles

MaskCue is maintained by one person.

- Committers and reviewers: [broening](https://github.com/broening)
- Approvers (every signing request is approved by hand): [broening](https://github.com/broening)

All team members use multi-factor authentication.

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

**Stand:** noch nicht signiert. Die Installer haben kein
Code-Signing-Zertifikat. Darum warnt Windows SmartScreen vielleicht. Klick auf
**Weitere Informationen** und dann auf **Trotzdem ausführen**. Die Datei prüfst
du mit der SHA-256-Prüfsumme auf der
[Download-Seite](https://github.com/broening/maskcue/releases/latest).
Updates in der App sind getrennt davon mit dem Update-Schlüssel unterschrieben.

Diese Regeln gelten, sobald der Installer signiert ist. MaskCue hat sich im
November 2026 bei der SignPath Foundation beworben. Der Antrag wurde vorerst
abgelehnt, weil das Projekt noch neu ist. Wir bewerben uns später noch einmal.

- Signiert wird nur der Windows-Installer. Er wird bei jedem Versions-Tag
  automatisch von GitHub Actions aus diesem Quellcode gebaut.
- Rollen: Committer, Prüfer und Freigabe: [broening](https://github.com/broening).
  Jede Signatur wird von Hand freigegeben. Alle nutzen Zwei-Faktor-Anmeldung.
- Datenschutz: MaskCue sendet nichts ins Netz, außer der Update-Suche bei
  GitHub beim Start (abschaltbar unter Einstellungen → Allgemein → Updates)
  und dem Download der KI-Stimme auf Wunsch.
  [Datenschutzerklärung](https://maskcue.com/datenschutz.html)
