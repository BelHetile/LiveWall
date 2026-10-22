# Changelog

All notable changes to LiveWall are listed here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [2.1.0] - 2026-10-16

### Added
- The whole app is in Russian as well as English, and a language picked in Settings applies at once.
- Your own styles: save the current colours under a name, and import or export them as `.livewall-style` files.
- Folder icons: an SF Symbol or one of your own images for any folder.
- A shortcut for any wallpaper: the first press puts it on the desktop, the lock screen or both, the next press
  brings back what was there before. It works for wallpapers in private folders without unlocking them.
- Turn a video or image that is stored on its side by 90°, from the inspector or the context menu.
- Settings → Privacy → *Take Private Wallpapers off the Desktop*.
- Settings → General → *Show in Dock*: turn it off to keep LiveWall in the menu bar only.
- `lwctl rotate`, `lwctl item-hotkey` and `lwctl folder icon`.

### Fixed
- Locking a private folder no longer takes its wallpaper off the desktop, and automatic change leaves it there
  until you unlock.
- A private wallpaper on the desktop or picked for the lock screen is no longer swapped for another one on the
  lock screen, in Mission Control and after quitting LiveWall.
- Photos taken in portrait no longer show on their side on the desktop.
- Some optimized videos showed shifted or turned.
- The toolbar fits again on macOS 26: Add, Search and Settings no longer hide behind ». Buttons stay readable
  when the window is in the background, and Russian labels are no longer cut off.
- *Pause When Desktop Is Covered* works again with a zoomed window: coverage is measured on the screen between the
  menu bar and the Dock, as in 1.x, so a window that fills it pauses the wallpaper.

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
