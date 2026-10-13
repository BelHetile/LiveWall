# Changelog

All notable changes to LiveWall are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Performance
- Window coverage is only checked while a video or GIF is on, less often and with timer tolerance; the power source
  is reported by the system instead of being polled.
- GIF wallpapers pause together with videos (covered desktop, fullscreen apps, battery, …).
- Closing the library window frees its interface, thumbnails, images and the preview player.
- At most three thumbnails are generated at a time; the library index is saved once per burst of changes.
- Local videos start without extra buffering and keep a smaller frame buffer.

### Changed
- Repository layout: scripts live in `scripts/`, `Info.plist` in `Resources/`.

## [1.0.0] - 2026-10-13

First public release.

### Wallpapers
- Videos (mp4, mov, m4v), images (png, jpg, heic, webp) and animated GIFs as desktop wallpaper on every display and Space.
- Add files and folders, drag & drop, or paste a link (direct file or a page with `og:video` / `<video>` / `og:image`).
- Per-wallpaper fill mode (Fill, Fit, Stretch, Center), volume, mute and playback speed.
- Lock screen: a still frame of the desktop wallpaper or of a separately chosen one.

### Library
- Full-screen library with live Desktop and Lock Screen previews and a panel for every wallpaper.
- Folders with a built-in icon pack or your own images; password-locked folders.
- Renaming of wallpapers and folders.

### Control
- Per-wallpaper toggle hotkeys for the desktop, the lock screen or both — also for wallpapers in locked folders.
- Global hotkeys, menu bar menu, Dock icon, launch at login.
- Smart pause: fullscreen apps, covered desktop, focused app, battery, Low Power Mode, heat, app rules, sleep and lock.
- Automatic rotation by timer.

### Look
- Eight color styles, custom styles with import/export, English and Russian interface.

### Install
- One-line installer, DMG and universal (Apple Silicon + Intel) builds published automatically for tagged releases.
