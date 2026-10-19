# Changelog

All notable changes to LiveWall are listed here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [2.1.0] - 2026-10-19

### Added
- A folder sidebar replaces the toolbar folder menu: All Wallpapers plus every folder with its wallpaper count.
  Right-click a folder to change its icon and color, rename it in place, lock it, hide it while locked or
  delete it; ⌥⌘S shows and hides the sidebar, ⌥⌘I the inspector.
- Folder icons and colors: pick an SF Symbol, one of the eight Finder tag colors or any #RRGGBB color (previewed
  as you type), or use your own PNG, JPEG or HEIC image; drop an image on the picker to add it. A locked folder
  shows a lock until you give it an icon of its own. `lwctl folder style` and `lwctl icon` do the same from the
  command line.
- Drag wallpapers onto a folder in the sidebar to move them; drop files from Finder on a folder or on the
  library to import them there.
- Settings → Privacy → *Take Private Wallpapers off the Desktop*. It is on: locking the folders replaces a private
  wallpaper on the desktop with a safe one. Turn it off to keep a private wallpaper on the desktop and the lock
  screen while locked; the library and the menu bar still hide it.
- Per-wallpaper shortcuts (from BelHetile): record one under Shortcut in the inspector and choose whether it
  switches the desktop, the lock screen or both. A second press switches back; ✕ clears it. `lwctl item-hotkey
  ID [COMBO] [--scope desktop|lock|both]` sets or clears it from the command line.
- Settings → Playback → Lock Screen → *Follow the Wallpaper*: the lock screen changes along with the wallpaper
  on Next, Previous and rotation instead of keeping the one picked with Set as Lock Screen.
  `lwctl lock-screen set ID [--time S]` and `lwctl lock-screen clear` pick or unpin it from the command line.
- Library keys: Return sets the selected wallpaper, Space plays or pauses it, comma and period step a frame, Esc
  clears the selection.
- The menu bar item has Settings… and Lock or Unlock Private Folders; Next, Previous and Random are disabled
  with fewer than two playable wallpapers.

### Changed
- The library window, sidebar, inspector, toolbar and Settings are rebuilt from standard macOS controls. They
  follow light and dark mode, the accent color, Increase Contrast and Reduce Transparency like any Mac app, and
  use Liquid Glass on macOS 26 and newer.
- File → Add Files… (⌘O) takes files and folders in one panel; Add Folder… is gone.
- Delete and the 1–9 and ` keys act on the selected wallpaper instead of the one under the pointer; Delete asks
  first.
- The Dock icon stays while Settings is open or the library window is minimized.
- Tiles show which desktops a wallpaper is on: a desktop icon for all desktops, the icon and the desktop numbers
  otherwise.
- Wallpaper → Mute is ⌥⌘↓, next to ⌘↑ and ⌘↓ for the volume.
- Folder colors picked from the old presets become the nearest tag color.
- The whole app is in English and Russian and follows the macOS language; pick a language for LiveWall alone in
  System Settings → General → Language & Region → Applications. The language setting inside LiveWall is gone.

### Removed
- Color themes and the custom accent color. Appearance keeps light, dark and system; older settings load and
  are rewritten without them.

### Fixed
- Photos load upright from their EXIF orientation, and rotated videos stay upright after Optimize for This Mac.
- Spaces whose desktop picture file was deleted elsewhere no longer block LiveWall: they play a wallpaper like
  any other Space, and quitting leaves them alone instead of writing a dead path back.
- After a crash, LiveWall no longer puts an old still of its own back as your desktop picture; quitting keeps the
  current wallpaper on screen instead, and a desktop picture you set yourself is never overwritten.

## [2.0.0] - 2026-10-16

LiveWall is now a native Rust app for Apple Silicon Macs on macOS 15 or newer. It starts with an empty library.

### Added
- Videos (mp4, mov, m4v, webm, mkv), animated GIFs and images (png, jpg, heic, webp, tiff, bmp) as wallpaper below
  the desktop icons on every Space, including full-screen app Spaces; one wallpaper for all desktops or one per
  desktop.
- Library window with folders, search, grid and list views, an inspector for every wallpaper, drag & drop and
  video links through yt-dlp (YouTube first). New files land in the folder being viewed; folders can be created
  private, renamed and deleted in the window; Delete removes the wallpaper under the pointer.
- Fill, Fit, Stretch, Center and Custom (position and zoom) per wallpaper, plus volume, mute and speed; the
  inspector preview can play with sound while the desktop copy stays silent.
- Animated transitions between wallpapers (fade, wipe, wave, grow, directional sweeps and more), with a global
  default and per-command options in `lwctl`.
- Optimize for This Mac: conversion to HEVC at screen size with a frame-rate limit using Apple's VideoToolbox;
  HDR and 10-bit kept; ffmpeg only for formats macOS can't read. New imports are optimized when it helps.
- Smart pause for full-screen apps, covered desktops, Low Power Mode, heat, focus (optional) and app rules;
  playback stops while the display sleeps, the screen is locked or the screen saver runs. The menu bar and the
  library toolbar show whether the wallpaper plays, is paused or unloaded and why, and how sound is muted. Paused videos are unloaded after a delay and replaced by their exact
  paused frame.
- The macOS desktop picture follows the wallpaper as a still frame, so the lock screen and Mission Control
  match; the frame can be picked, or a separate lock screen wallpaper chosen. The original desktop picture of
  every Space is restored on quit and after a crash; still frames nobody shows are deleted.
- Private folders: hidden from the library, the menu bar and the desktop until unlocked with Touch ID or the Mac
  password; they lock again on screen lock, display sleep, idle time or ⌃⌥⌘X. A folder can be hidden entirely
  while locked. Files of private folders live in a hidden, owner-only `private/` directory.
- Rotation on a timer, in order or shuffled.
- Global hotkeys for next, previous, random, pause, mute, volume and lock, checked against shortcuts macOS
  already uses; menu bar item; a Dock icon only while the library window is open; launch at login.
- Light, dark and system appearance with six color themes and a custom accent.
- `lwctl`, a command-line client for everything the app does, bundled inside the app.
- Menu bar item and messages in English and Russian.
- One-line installer that verifies the download against the release's SHA-256 checksums; DMG and zip on every
  release.
