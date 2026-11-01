# MaskCue

**English** | [Deutsch](README.de.md)

Free voice changer and soundboard for Windows. Made for streamers and role
players, for example on FiveM and RedM.

**[⬇ Download MaskCue](https://github.com/broening/maskcue/releases/latest)** · Version 0.1.4 · [maskcue.com](https://maskcue.com/en/) · [Discord](https://discord.gg/yhBvJD6Wa)

> Version 0.1.4 is an early test version. There may still be bugs.
> Report them on Discord or under [Issues](https://github.com/broening/maskcue/issues).

## What MaskCue does

- **Characters:** Every character is a voice of its own with picture, pitch,
  timbre and bright/dark. You turn it on with one click or a hotkey.
- **AI voice (optional):** Thorsten, a German voice. It turns your voice into
  another person and runs on your PC, without internet.
- **Spaces:** A room or effect around your voice, for example large building,
  cave, radio, robot, mask, space helmet or megaphone.
- **Soundboard:** Your own sounds and music on hotkeys. Music gets quieter
  when you speak. 10 sound effects are included.
- **Game profiles:** Separate hotkeys for each game. FiveM and RedM are
  already set up. MaskCue detects the game by itself.
- **Clean voice:** Noise suppression, gate, compressor and EQ for your
  microphone.
- **Recordings, overlay, export:** Record your voice, see an overlay in game,
  share characters as a file, back up all settings.
- **Low latency:** MaskCue shows the latency in milliseconds.
- **Start with Windows:** optional, MaskCue then waits hidden in the tray.
- **Updates:** automatic, notify only or off. Updates are signed.
- **Languages:** The app is available in English and German.

## What you need

- 64-bit Windows 10 or 11
- a microphone
- at least one virtual audio device, so your voice reaches Discord,
  TeamSpeak and the game. For example **Elgato Wave Link 3.0**, VoiceMeeter
  or VB-Cable. With one device you use the “Game” output as your microphone
  everywhere and turn the character off when you want to speak normally.
  **Two devices are recommended:** “Game” for character and sounds, “Chat”
  for your real, noise-free voice, for example in Discord.
  The **Setup** page in MaskCue shows you the steps.

## Install

1. Download `MaskCue_0.1.4_x64-setup.exe` from the
   [download page](https://github.com/broening/maskcue/releases/latest).
2. Run the file.
3. Windows may show “Windows protected your PC”. This happens because the
   installer is not signed yet. Click **More info** and then **Run anyway**.
4. Start MaskCue and follow the **Setup** page.

**Verify (optional):** The SHA-256 checksum of every file is on the download
page. In PowerShell:

```text
Get-FileHash .\MaskCue_0.1.4_x64-setup.exe -Algorithm SHA256
```

## AI voice

The AI voice is not part of the installer. You download it in MaskCue under
**Characters → Settings → AI voice**. It is about 18 MB. Before that you
agree to these rules:

- I use the voice for games, role play, fun and my own content.
- I do not pretend to be a real person with it.
- I do not deceive, defraud or harass anyone with it.
- I follow the rules of games and servers. I am responsible for what I say.

The voice is in the release
[AI voice Thorsten](https://github.com/broening/maskcue/releases/tag/stimmen-v2).

| Voice | Origin | License |
|---|---|---|
| Thorsten | Thorsten-Voice, dataset by Thorsten Müller | CC0 1.0 |

Trained with Project Beatrice, Beatrice 2 (MIT), pretrained on LibriTTS-R
(CC BY 4.0). Phoneme recognition fine-tuned on Multilingual LibriSpeech German
(CC BY 4.0, Pratap et al., 2020).

## Sound effects

The 10 included sounds are from Wikimedia Commons. They are CC0 or public
domain: applause, cheering, laughter, wow, drum roll, gong, wrong, knocking,
cash register and explosion. Authors and sources are listed in
`sounds\HERKUNFT.txt` in the installation folder.

## Privacy

MaskCue runs on your PC. Your voice and your settings never leave your PC.
MaskCue only goes online for two things, both at GitHub: when you download
the AI voice, and to look for updates at startup. The update check can be
set to automatic, notify only or off (Settings → General → Updates).

## License

MaskCue is licensed under the GPL-3.0, see [LICENSE](LICENSE). You can get
the source code on request via [Issues](https://github.com/broening/maskcue/issues).

Components used include: Tauri (MIT/Apache-2.0), DeepFilterNet
(MIT/Apache-2.0), the RNNoise port nnnoiseless (BSD-3-Clause), Signalsmith
Stretch (MIT), Symphonia (MPL-2.0), cpal (Apache-2.0).

## Changes

What changed in each version is in [CHANGELOG.md](CHANGELOG.md).
