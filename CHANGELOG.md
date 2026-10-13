# Changelog

All notable changes to LiveWall are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.1.1] - 2026-10-13

### Added
- Settings → System: open the library at launch or not, open it filling the screen, in full screen (its own Space)
  or at the last size and position, and hide the menu bar icon.
- Scrubbing or stepping the preview of the desktop wallpaper moves and pauses the wallpaper too, so the frame you
  pick is the one that stays on the desktop. Putting a paused preview on the desktop keeps that frame.

### Fixed
- Videos from YouTube and similar sites now have sound: the separate audio stream is downloaded and joined.
- Locking the Mac while LiveWall was in front could switch to another Space after unlocking.
- Switching Spaces while LiveWall was in front could leave the menu bar empty; the focus now goes to the app in
  front on the new Space.
- Volume sliders no longer jump while dragging, and the panel no longer redraws many times a second while a
  preview plays.

## [1.1.0] - 2026-10-13

### Added
- **WebM, MKV and YouTube.** WebM and MKV files (VP9, H.264/HEVC, AV1 on M3 and newer) and MP4 files with codecs
  macOS can't play are converted to HEVC on import. Links to YouTube, Vimeo, TikTok and hundreds of other sites are
  downloaded with yt-dlp (plus Deno for YouTube); LiveWall asks before downloading these tools from their official
  GitHub releases and can update them in Settings. Videos that show up black get a *Convert* button.
- **Optimize for This Mac.** Re-encodes a video to HEVC at the size of your largest screen with a frame rate limit
  (24, 30 or 60 fps) — from the wallpaper panel, the card menu or *Settings → Performance → Optimize All Videos*.
  New videos are optimized automatically (can be turned off); videos that are already light are left alone.
- **Unload paused video** (like Wallpaper Engine's *Stop*). A video paused for a while (10 s – 5 min, or never) is
  unloaded from memory, its exact paused frame stays on screen, and it loads back from the same spot on resume.
- **Lock screen frame.** Choose which frame of a video goes to the lock screen with the timeline slider and
  frame-by-frame buttons under the preview, then *Use Current Frame*.
- Turning the lock-screen option off brings back the macOS wallpaper you had before LiveWall.
- **Safe hotkeys.** Combinations used by macOS (Spotlight, screenshots, Mission Control, input sources — including
  the ones you changed in System Settings), common app commands (⌘ + key, ⌘Tab, ⌃⌘Q, …) or another app can't be
  assigned; LiveWall says what uses them and keeps listening for another combination.
- The DMG includes **Install LiveWall.command**, which copies the app to Applications, clears the download flag and
  starts it.
- The wallpaper panel shows the video's resolution, frame rate and codec.

### Changed
- Library previews play on their own instead of mirroring the desktop wallpaper, which is usually paused behind the
  library window. When the wallpaper is paused, previews stop on the same frame and show a *Paused* badge.
- GIFs are animated in the library and pause with the wallpaper.
- Images no longer show pause and sound controls (library, menu bar).
- The lock-screen option is called *Show the wallpaper on the lock screen* and also covers a separately chosen
  lock-screen wallpaper.
- The wallpaper panel preview has a margin and rounded corners, and the panel scrolls when it doesn't fit.

### Fixed
- GIF wallpapers didn't always pause.
- Panel controls (hotkey scope, speed) could overflow the panel.
- The macOS wallpaper is no longer changed while the screen is locked or right after unlocking, and isn't set again
  when the right frame is already there.

### Performance
- Window coverage is only checked while a video or GIF is on, less often and with timer tolerance; the power source
  is reported by the system instead of being polled.
- A paused GIF costs nothing: its animation is removed until it resumes.
- Closing the library window frees its interface, thumbnails, images and preview players.
- At most three thumbnails are generated at a time; the library index is saved once per burst of changes.
- Local videos start without extra buffering and keep a smaller frame buffer.

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
