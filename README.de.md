# MaskCue

[English](README.md) | **Deutsch**

Kostenloser Voice-Changer und Soundboard für Windows. Gemacht für Streamer und
Rollenspieler, zum Beispiel auf FiveM und RedM.

**[⬇ MaskCue herunterladen](https://github.com/broening/maskcue/releases/latest)** · Version 0.1.3 · [maskcue.com](https://maskcue.com) · [Discord](https://discord.gg/yhBvJD6Wa)

> Version 0.1.3 ist eine frühe Testversion. Es kann noch Fehler geben.
> Fehler meldest du im Discord oder unter [Issues](https://github.com/broening/maskcue/issues).

## Was MaskCue kann

- **Figuren:** Jede Figur ist eine eigene Stimme mit Bild, Tonhöhe, Klangfarbe
  und Hell/Dunkel. Du schaltest sie mit einem Klick oder einem Hotkey ein.
- **KI-Stimme (optional):** Thorsten, eine deutsche Stimme. Sie macht aus
  deiner Stimme eine andere Person und läuft auf deinem PC, ohne Internet.
- **Umgebungen:** Raum oder Effekt um die Stimme, zum Beispiel großes
  Gebäude, Höhle, Funkgerät, Roboter, Maske, Raumhelm oder Megafon.
- **Soundboard:** Eigene Sounds und Musik mit Hotkeys. Musik wird leiser,
  wenn du sprichst. 10 Sound-Effekte sind schon dabei.
- **Spiel-Profile:** Eigene Hotkeys je Spiel. FiveM und RedM sind schon
  angelegt. MaskCue erkennt das Spiel selbst.
- **Saubere Stimme:** Rauschunterdrückung, Gate, Kompressor und EQ für dein
  Mikrofon.
- **Aufnahmen, Overlay, Export:** Stimme aufnehmen, Overlay im Spiel, Figuren
  als Datei weitergeben, alle Einstellungen sichern.
- **Wenig Verzögerung:** MaskCue zeigt die Verzögerung in Millisekunden an.
- **Mit Windows starten:** wahlweise, MaskCue wartet dann unsichtbar im Tray.
- **Sprachen:** Die App gibt es auf Deutsch und Englisch.

## Was du brauchst

- Windows 10 oder 11, 64 Bit
- ein Mikrofon
- mindestens ein virtuelles Audio-Gerät, damit die Stimme in Discord,
  TeamSpeak und im Spiel ankommt. Zum Beispiel **Elgato Wave Link 3.0**,
  VoiceMeeter oder VB-Cable. Mit einem Gerät nimmst du den Ausgang „Spiel“
  überall als Mikrofon und schaltest die Figur aus, wenn du normal sprechen
  willst. **Empfohlen sind zwei Geräte:** „Spiel“ für Figur und Sounds,
  „Chat“ für deine echte, entrauschte Stimme, zum Beispiel in Discord.
  Die Seite **Einrichtung** in MaskCue zeigt dir die Schritte.

## Installieren

1. Lade `MaskCue_0.1.3_x64-setup.exe` von der
   [Download-Seite](https://github.com/broening/maskcue/releases/latest).
2. Starte die Datei.
3. Windows zeigt vielleicht „Der Computer wurde durch Windows geschützt“.
   Das kommt, weil der Installer noch nicht signiert ist. Klick auf
   **Weitere Informationen** und dann auf **Trotzdem ausführen**.
4. Starte MaskCue und folge der Seite **Einrichtung**.

**Prüfen (optional):** Die SHA-256-Prüfsumme jeder Datei steht auf der
Download-Seite. In PowerShell:

```text
Get-FileHash .\MaskCue_0.1.3_x64-setup.exe -Algorithm SHA256
```

## KI-Stimme

Die KI-Stimme ist nicht im Installer. Du lädst sie in MaskCue unter
**Figuren → Einstellen → KI-Stimme**. Sie ist etwa 18 MB groß.
Vorher stimmst du diesen Regeln zu:

- Ich nutze die Stimme für Spiele, Rollenspiel, Spaß und eigene Inhalte.
- Ich gebe mich damit nicht als echte Person aus.
- Ich täusche, betrüge oder belästige niemanden damit.
- Ich halte mich an die Regeln der Spiele und Server. Für das, was ich sage,
  bin ich selbst verantwortlich.

Die Stimme liegt im Release
[KI-Stimme Thorsten](https://github.com/broening/maskcue/releases/tag/stimmen-v2).

| Stimme | Herkunft | Lizenz |
|---|---|---|
| Thorsten | Thorsten-Voice, Datensatz von Thorsten Müller | CC0 1.0 |

Trainiert mit Project Beatrice, Beatrice 2 (MIT), Vortraining auf LibriTTS-R
(CC BY 4.0). Laut-Erkennung nachtrainiert auf Multilingual LibriSpeech Deutsch
(CC BY 4.0, Pratap et al., 2020).

## Sound-Effekte

Die 10 mitgelieferten Sounds sind von Wikimedia Commons. Sie sind CC0 oder
gemeinfrei: Applaus, Jubel, Lachen, Staunen, Trommelwirbel, Gong, Falsch,
Klopfen, Kasse und Explosion. Urheber und Quellen stehen in der Datei
`sounds\HERKUNFT.txt` im Installationsordner.

## Datenschutz

MaskCue läuft auf deinem PC. Deine Stimme und deine Einstellungen verlassen
den PC nicht. Ins Internet geht MaskCue nur, wenn du die KI-Stimme
herunterlädst. Dann lädt es die Datei von GitHub.

## Lizenz

MaskCue steht unter der GPL-3.0, siehe [LICENSE](LICENSE). Den Quellcode
bekommst du auf Anfrage über [Issues](https://github.com/broening/maskcue/issues).

Verwendete Bausteine, unter anderem: Tauri (MIT/Apache-2.0), DeepFilterNet
(MIT/Apache-2.0), RNNoise-Portierung nnnoiseless (BSD-3-Clause), Signalsmith
Stretch (MIT), Symphonia (MPL-2.0), cpal (Apache-2.0).

## Änderungen

Was sich in jeder Version geändert hat, steht in [CHANGELOG.de.md](CHANGELOG.de.md).
