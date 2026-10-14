<p align="center"><img src="Resources/AppIcon.iconset/icon_256x256.png" width="128" alt="LiveWall icon"></p>

<h1 align="center">LiveWall</h1>

<p align="center">
  A native macOS app that plays videos, images and GIFs as desktop wallpaper, below the desktop icons,
  on every display and Space.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/macOS-13%2B-blue" alt="macOS 13+">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-green" alt="MIT license"></a>
</p>

<p align="center"><b>English</b> · <a href="README.ru.md">Русский</a></p>

## Requirements

- macOS 13 Ventura or newer.
- Apple Silicon or Intel.
- Command Line Tools to build from source (`xcode-select --install`).

## Build from source

```bash
git clone https://github.com/BelHetile/LiveWall.git
cd LiveWall
./scripts/build.sh --install
```

- `./scripts/build.sh` builds `build/LiveWall.app` without installing.
- `./scripts/build.sh --universal` builds a universal binary (Apple Silicon + Intel).

## Install

From a release:

```bash
curl -fsSL https://raw.githubusercontent.com/BelHetile/LiveWall/main/scripts/install.sh | bash
```

The script downloads the latest release, installs it to `/Applications` and launches it. Run it again to update.

From the DMG:

1. Download `LiveWall.dmg` from the [latest release](https://github.com/BelHetile/LiveWall/releases/latest).
2. Open it and double-click **Install LiveWall.command**: it copies LiveWall to `/Applications`, clears the
   download flag and launches it.
3. LiveWall is not notarized, so macOS asks once before running the installer:
   - **macOS 15 and newer:** double-click the command once, then open **System Settings → Privacy & Security**,
     scroll down and click **Open Anyway**.
   - **macOS 13–14:** right-click **Install LiveWall.command** → **Open** → **Open**.

   Dragging **LiveWall** to **Applications** manually works too; the same prompt then appears for the app itself.

## Features

- Videos (mp4, mov, m4v, webm, mkv), images (png, jpg, heic, webp) and animated GIFs as wallpaper, on every
  display and Space, below the desktop icons.
- WebM and MKV files (VP9, H.264/HEVC, AV1 on M3 and newer) and MP4 files with codecs macOS can't play are
  converted to HEVC on import; videos that show up black get a **Convert** button.
- Wallpapers are added with a button, by drag & drop, or by pasting a link: a direct file, a page that contains
  one, or YouTube, Vimeo, TikTok and other sites supported by [yt-dlp](https://github.com/yt-dlp/yt-dlp) (plus
  [Deno](https://deno.com) for YouTube). The tools are downloaded on first use after a confirmation prompt, or
  taken from Homebrew, and can be updated in Settings. VP8 video can't be decoded on a Mac.
- Per-wallpaper fill mode (**Fill, Fit, Stretch, Center**), volume, mute and playback speed.
- Smart pause: playback stops when an app is full screen or windows cover the desktop, when another app is
  focused (optional), when the Mac runs on battery, is in Low Power Mode or gets hot (optional), when an app
  from user-defined rules is running, and when the display sleeps or the screen is locked.
- Hotkeys: a toggle hotkey per wallpaper (desktop, lock screen, or both — works for wallpapers in locked
  folders), plus global next / previous / random, pause, fill mode, mute and library shortcuts.
- Folders with a built-in icon pack or custom images; folders can be protected with a password (stored only as a
  salted PBKDF2-SHA256 hash; the files themselves are not encrypted).
- Lock screen sync: a still frame of the chosen wallpaper — macOS doesn't allow live video on the lock screen.
  The frame is picked with the preview timeline; turning the option off restores the previous macOS wallpaper.
- The wallpaper panel shows the video's resolution, frame rate and codec.
- Full-screen library with live desktop and lock screen previews; the timeline scrubber moves and pauses the
  desktop wallpaper, frame by frame.
- **Optimize for This Mac**: re-encodes a video to HEVC at screen size with a 24/30/60 fps limit; a video paused
  long enough is unloaded from memory and replaced by its exact paused frame.
- Automatic rotation by timer, in order or shuffled; eight color styles plus custom styles as `.json` files;
  English and Russian interface; launch at login.
- Wallpaper Engine `.mp4` wallpapers work directly; `scene.pkg` wallpapers only run inside Wallpaper Engine and
  can't be played on macOS.

## Hotkeys

Wallpaper hotkeys are set in each wallpaper's panel. Global hotkeys can be changed or cleared in
**Settings → Hotkeys**:

| Action | Default |
| --- | --- |
| Next wallpaper | ⌃⌥⌘→ |
| Previous wallpaper | ⌃⌥⌘← |
| Random wallpaper | ⌃⌥⌘R |
| Pause / resume video | ⌃⌥⌘Space |
| Cycle fill mode | ⌃⌥⌘M |
| Mute / unmute | ⌃⌥⌘S |
| Open library | ⌃⌥⌘L |

Hotkeys need no permissions in System Settings. Combinations that conflict with known macOS shortcuts are
refused, and LiveWall reports what uses them. Combinations held by other apps can't be detected reliably; if a
hotkey doesn't fire, pick another one.

## Data & uninstall

- Wallpapers, thumbnails, icons and downloaded video tools: `~/Library/Application Support/LiveWall/`
- Settings: `defaults read io.github.belhetile.livewall`

Uninstall:

```bash
curl -fsSL https://raw.githubusercontent.com/BelHetile/LiveWall/main/scripts/uninstall.sh | bash
```

This removes the app and keeps your data. Add ` -s -- --all` to also delete wallpapers, settings, saved
application state and caches. Alternatively, drag LiveWall from Applications to the Trash.

## License

[MIT](LICENSE) © 2026 BelHetile
