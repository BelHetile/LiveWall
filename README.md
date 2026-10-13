<p align="center"><img src="Resources/AppIcon.iconset/icon_256x256.png" width="128" alt="LiveWall icon"></p>

<h1 align="center">LiveWall</h1>

<p align="center">
  Live video wallpapers for macOS — with smart pause, hotkeys, folders and lock-screen sync.
</p>

<p align="center">
  <a href="https://github.com/BelHetile/LiveWall/releases/latest"><img src="https://img.shields.io/github/v/release/BelHetile/LiveWall?label=download" alt="Latest release"></a>
  <a href="https://github.com/BelHetile/LiveWall/actions/workflows/build.yml"><img src="https://github.com/BelHetile/LiveWall/actions/workflows/build.yml/badge.svg" alt="Build"></a>
  <img src="https://img.shields.io/badge/macOS-13%2B-blue" alt="macOS 13+">
  <img src="https://img.shields.io/badge/Apple%20Silicon%20%26%20Intel-universal-lightgrey" alt="Universal">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-green" alt="MIT license"></a>
</p>

<p align="center"><b>English</b> · <a href="README.ru.md">Русский</a></p>

---

LiveWall puts videos, images and GIFs on your desktop like Wallpaper Engine does on Windows. It is a small native
Swift app: video is decoded by the Mac's hardware, playback pauses itself when you can't see the desktop, and
everything is controlled from the library window, the menu bar or global hotkeys.

## Install

**One command** (recommended) — paste into Terminal:

```bash
curl -fsSL https://raw.githubusercontent.com/BelHetile/LiveWall/main/scripts/install.sh | bash
```

It downloads the latest release, puts LiveWall into **Applications** and starts it. Run the same command again to
update.

<details>
<summary><b>Or install the DMG by hand</b></summary>

1. Download **LiveWall.dmg** from the [latest release](https://github.com/BelHetile/LiveWall/releases/latest).
2. Open it and double-click **Install LiveWall.command**: it copies LiveWall to **Applications**, clears the
   download flag (so macOS won't ask about the developer again) and starts it.
3. LiveWall is free and not notarized by Apple, so macOS asks once before running the installer:
   - **macOS 15 and newer:** double-click it once, then go to **System Settings → Privacy & Security**, scroll down
     and click **Open Anyway**.
   - **macOS 13–14:** right-click **Install LiveWall.command** → **Open** → **Open**.

   You can also just drag **LiveWall** to **Applications**; then the same question comes up for LiveWall itself.

</details>

<details>
<summary><b>Or build from source</b></summary>

You need the Command Line Tools (`xcode-select --install`).

```bash
git clone https://github.com/BelHetile/LiveWall.git
cd LiveWall
./scripts/build.sh --install
```

`./scripts/build.sh` alone builds `build/LiveWall.app` without installing; `./scripts/build.sh --universal` builds for Apple Silicon
and Intel.

</details>

**Requirements:** macOS 13 Ventura or newer, Apple Silicon or Intel.

## Features

**Wallpapers**
- Videos (mp4, mov, m4v, webm, mkv), images (png, jpg, heic, webp) and animated GIFs. Formats macOS can't play
  (WebM/VP9, MKV, AV1 on M3 and newer) are converted to HEVC automatically.
- Add them with **Add**, by dragging files, folders or links into the window, or by pasting a link (**⌘V**).
  Links can point to a file, a web page that has one, or YouTube, Vimeo, TikTok and hundreds of other sites.
- Fill modes per wallpaper: **Fill, Fit, Stretch, Center**. Volume, mute and playback speed per wallpaper.
- Works on every display and every Space, below your desktop icons. Smooth crossfade when switching.

**Library**
- Full-screen library with live previews of the **Desktop** and **Lock Screen**.
- Click a wallpaper to open its panel: put it on the desktop or the lock screen, pause, volume, speed, fill mode,
  folder and hotkey.
- **Folders** with a built-in icon pack (54 icons × 8 colors) or your own images; drag wallpapers onto a folder.
- **Locked folders**: protect a folder with a password — its wallpapers disappear from the library, the menu and
  Next/Random until you unlock it.
- Rename anything with a double-click.

**Hotkeys**
- Give any wallpaper its own hotkey: press once to switch to it, press again to go back to the previous one. A
  hotkey can switch the desktop, the lock screen or both, and works even for wallpapers in locked folders.
- Global hotkeys for next / previous / random, pause, fill mode, mute and opening the library.

**Smart pause** (like Wallpaper Engine's performance settings) — the video pauses when:
- an app is full screen, or windows cover the desktop;
- another app is focused (optional);
- the Mac runs on battery (optional), in Low Power Mode or gets hot;
- an app from your **App rules** is running or in front (e.g. games);
- the display sleeps or the screen is locked.

**Performance**
- *Optimize for This Mac*: re-encodes a video to HEVC at your screen size and up to 30 fps (24/60 in Settings),
  automatically for new videos. A 4K 60 fps clip becomes much cheaper to decode.
- A video paused for a while (30 s by default) is unloaded from memory and replaced by its exact paused frame, then
  loaded back when playback resumes — like Wallpaper Engine's *Stop* setting.

**And also**
- Lock screen: a still frame of the chosen wallpaper (macOS doesn't allow live video there).
- Automatic rotation by timer, in order or shuffled.
- 8 color styles, your own styles (shareable as `.json`), English and Russian interface.
- Launch at login, Dock icon (optional), menu bar menu.

## Hotkeys

Wallpaper hotkeys are set in each wallpaper's panel — none are assigned until you choose one. The global ones can
be changed or cleared in **Settings → Hotkeys**:

| Action | Default |
| --- | --- |
| Next wallpaper | ⌃⌥⌘→ |
| Previous wallpaper | ⌃⌥⌘← |
| Random wallpaper | ⌃⌥⌘R |
| Pause / resume video | ⌃⌥⌘Space |
| Cycle fill mode | ⌃⌥⌘M |
| Mute / unmute | ⌃⌥⌘S |
| Open library | ⌃⌥⌘L |

Hotkeys don't need any permissions in System Settings.

## Good to know

- **Battery.** Hardware decoding keeps the cost low, but a full-screen video still uses some power. Smart pause
  stops playback whenever the desktop isn't visible; turn on *Running on battery* in Settings to pause on battery.
  For the lowest cost, optimize your videos (wallpaper panel → *Optimize for This Mac*).
- **Lock screen.** macOS doesn't let apps show live video on the lock screen, so LiveWall sets a still frame there
  (Wallpaper Engine on Windows does the same).
- **Locked folders** hide wallpapers inside LiveWall. Passwords are stored only as a salted PBKDF2-SHA256 hash and
  can't be recovered. The files themselves are not encrypted on disk.
- **Wallpaper Engine.** Video wallpapers (`.mp4`) work directly. *Scene* wallpapers (`scene.pkg`) only run inside
  Wallpaper Engine and can't be played on a Mac.
- **YouTube and other sites** are downloaded with [yt-dlp](https://github.com/yt-dlp/yt-dlp) and
  [Deno](https://deno.com). LiveWall asks before downloading them the first time (or uses them from Homebrew) and can
  update them in **Settings → YouTube and other sites**. VP8 videos can't be decoded on a Mac.

## Where your data lives

- Wallpapers, thumbnails and icons: `~/Library/Application Support/LiveWall/`
- Settings: `defaults read io.github.belhetile.livewall`

## Uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/BelHetile/LiveWall/main/scripts/uninstall.sh | bash
```

Add ` -s -- --all` at the end to also delete your wallpapers and settings. Or just drag LiveWall from Applications
to the Trash.

## Color style files

Styles are small JSON files you can share (**Settings → Color style → Export… / Import…**):

```json
{ "id": "my-style", "name": "Midnight", "primary": "#3B5BDB", "secondary": "#AE3EC9" }
```

## Contributing

Bug reports and pull requests are welcome — see [CONTRIBUTING.md](.github/CONTRIBUTING.md). Changes are listed in
[CHANGELOG.md](CHANGELOG.md).

## License

[MIT](LICENSE) © 2026 BelHetile
