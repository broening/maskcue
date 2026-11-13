# Changes

**English** | [Deutsch](CHANGELOG.de.md)

## 0.1.5 – 2026-11-13

- Safer import of **settings backups**: sounds from a backup without sound
  files keep only local paths (like `C:\...`). Network paths are removed.
  Numbers outside the range of the sliders are corrected, so the settings no
  longer get lost on the next start.
- **Sound files** with a broken sample rate or too many channels no longer
  crash MaskCue. They show an error instead. Supported: 8 to 192 kHz.
- **Exports** only take files that really are sounds.
- The **bug report** no longer contains file paths (they often hold your
  Windows user name).

## 0.1.4 – 2026-11-02

- New: **Updates** (Settings → General → Updates). Choose **Automatic**
  (download and install at startup, then restart), **Notify only** or
  **Off**. Plus a **Check now** button. Updates are signed.
- Versions up to 0.1.3 have no updater: install 0.1.4 once by hand, after
  that updates come by themselves.

## 0.1.3 – 2026-11-01

- New: **Start with Windows** (Settings → General → Startup). MaskCue then
  starts hidden in the tray when you sign in. Open the window with a click on
  the icon.

## 0.1.2 – 2026-11-01

- The AI voice Thorsten is now the only voice available for download.
- Thorsten now comes from the release “stimmen-v2”. Older versions can no
  longer download an AI voice, please update to 0.1.2.

## 0.1.1 – 2026-11-01

- Small change to the AI voices.

## 0.1.0 – 2026-11-01

First test version.

- Characters with their own voice, picture, space and hotkey
- Start characters: Demon, Goblin, Cyborg
- Voice methods: Fast, Natural, Light, None
- AI voice Thorsten to download, with rules
- 9 spaces, for example large building, radio, robot, space helmet
- Soundboard with hotkeys, categories and quieter music while you speak
- 10 sound effects in the category “Sound-Effects”
- Game profiles for FiveM and RedM
- Noise suppression, gate, compressor, EQ
- Recordings, overlay, export and import of characters, backup
- German and English
