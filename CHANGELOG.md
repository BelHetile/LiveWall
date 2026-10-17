# Changelog

All notable changes to LiveWall are listed here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- A folder sidebar replaces the toolbar folder menu: All Wallpapers plus every folder with its wallpaper count.
  Right-click a folder to change its icon and color, rename it in place, lock it, hide it while locked or
  delete it; ⌃⌘S shows and hides the sidebar.
- Folder icons and colors: pick an SF Symbol, one of the eight Finder tag colors or any #RRGGBB color (previewed
  as you type), or use your own PNG, JPEG or HEIC image; drop an image on the picker to add it. A locked folder
  shows a lock until you give it an icon of its own. `lwctl folder style` and `lwctl icon` do the same from the
  command line.
- Color styles in Settings → Appearance: System follows the macOS accent color live; Graphite, Sequoia, Harbour,
  Dusk, Terracotta, Sand, Linen and Slate are built in. Set the accent, the tint and its strength, or open
  Advanced to change any single part (canvas, cards, text, tiles, chips, sidebar, toolbar, inspector, settings).
  The first change to a built-in style makes your own copy; rename, delete, export and import styles as
  `.livewallstyle` files. `lwctl style` does the same from the command line.
- Drag wallpapers onto a folder in the sidebar to move them; drop files from Finder on a folder or on the
  library to import them there.
- Settings → Privacy → *Take Private Wallpapers off the Desktop*. It is on: locking the folders replaces a private
  wallpaper on the desktop with a safe one. Turn it off to keep a private wallpaper on the desktop and the lock
  screen while locked; the library and the menu bar still hide it.

### Fixed
- Spaces whose desktop picture file was deleted elsewhere no longer block LiveWall: they play a wallpaper like
  any other Space, and quitting leaves them alone instead of writing a dead path back.

### Changed
- One design scale for spacing, corner radii, type, icons and motion across the library, inspector, sidebar,
  toolbar, settings and pickers. Every color now follows light and dark mode, Increase Contrast, Reduce
  Transparency and Reduce Motion, and updates live when the macOS accent color changes.
- Tile chips are readable over any wallpaper without forcing dark mode; status lights, destructive buttons and
  settings sliders use the style colors.
- Your previous theme moves to the matching new style (Violet → Dusk, Rose → Terracotta, Ocean → Harbour,
  Forest → Sequoia, Amber → Sand, Graphite → Graphite); a custom accent becomes a style named Custom. Folder colors
  picked from the old presets become the nearest tag color.
- The whole app is in English and Russian and follows the macOS language; pick a language for LiveWall alone in
  System Settings → General → Language & Region → Applications. The language setting inside LiveWall is gone.
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
