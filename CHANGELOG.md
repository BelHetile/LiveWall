# Changelog

All notable changes to LiveWall are listed here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- LiveWall can be given its own language in System Settings → General → Language & Region → Applications; the
  choice is picked up on the next launch.

### Changed
- Deno is downloaded only for YouTube links; other sites need yt-dlp alone.
- The interface translation moved to standard `Localizable.strings` / `Localizable.stringsdict` resources.
- `uninstall.sh --all` also removes saved application state and caches.
- yt-dlp and Deno are checked against the SHA-256 checksums published with their release before they are
  installed; `install.sh` checks LiveWall's download against the release's `SHA256SUMS.txt`.
- HDR (PQ, HLG) and 10-bit videos are converted and optimized to 10-bit HEVC with their color information kept;
  other videos are tagged as BT.709.
- GIFs are decoded in the background, once for all displays and previews; very large GIFs are scaled to the
  screen size.
- New folder passwords use 600,000 PBKDF2 rounds; existing passwords are upgraded the next time the folder is
  unlocked.
- Launch at login follows the state in System Settings and shows when macOS still needs approval.
- Builds get an increasing build number; release builds pin their GitHub Actions and can no longer move a
  published tag.

### Fixed
- Converted WebM, MKV and MP4 videos keep their sound (AAC, Opus, Vorbis, MP3, AC-3, ALAC). A sound track macOS
  can't decode is left out, and LiveWall says so.
- A damaged entry in the library file no longer empties the whole library: damaged entries are skipped, and a
  copy of the original file is kept next to it.
- Turning off the lock screen wallpaper restores each display's own previous wallpaper; displays that are not
  connected get theirs back when they reconnect.
- Hotkeys keep working after the library window is closed while a hotkey is being recorded.
- Quitting LiveWall stops running yt-dlp downloads; leftover download files are removed.
- The last frame of a GIF is shown.
- Screen lock and unlock are noticed right away even while LiveWall is in the background.
- Plural forms are correct in both languages (“1 icon”, “2 icons”; «2 иконки», «21 иконка»).
- A video whose separate sound track can't be joined now shows an error instead of being added without sound.
- Link downloads waiting for the video tools all continue once the tools are installed.
- Progress of YouTube downloads no longer jumps back when the sound track starts downloading.
- A failed download no longer retries with another format unless the requested format was unavailable.
- Import hints list all supported formats (webm, mkv, webp).
- The installers wait for a running LiveWall to quit instead of a fixed delay.

## [1.1.2] - 2026-10-13

### Fixed
- The menu bar no longer disappears when switching Spaces with the library open and the Dock icon turned off:
  LiveWall now shows its Dock icon while the library window is open, because macOS needs it to draw the menu bar.
- LiveWall no longer moves focus to another app in the first seconds after unlocking, which could switch Spaces.

## [1.1.1] - 2026-10-13

### Added
- Settings → System: open the library at launch (or not), open it filling the screen, in full screen (its own
  Space) or at the last size and position; hide the menu bar icon.
- Scrubbing or stepping the desktop-wallpaper preview moves and pauses the wallpaper itself, so the frame shown
  is the frame placed on the desktop; placing a paused preview keeps that frame.

### Fixed
- Videos from YouTube and similar sites now have sound: the separate audio stream is downloaded and joined.
- Locking the Mac while LiveWall was in front could switch to another Space after unlocking.
- Switching Spaces while LiveWall was in front could leave the menu bar empty; focus now goes to the app in front
  on the new Space.
- Volume sliders no longer jump while dragging, and the panel no longer redraws many times a second while a
  preview plays.

## [1.1.0] - 2026-10-13

### Added
- WebM and MKV files (VP9, H.264/HEVC, AV1 on M3 and newer) and MP4 files with codecs macOS can't play are
  converted to HEVC on import. Videos that show up black get a *Convert* button.
- Links to YouTube, Vimeo, TikTok and other supported sites are downloaded with yt-dlp (plus Deno for YouTube).
  LiveWall asks before downloading these tools from their official GitHub releases and can update them in
  Settings.
- *Optimize for This Mac*: re-encodes a video to HEVC at the size of the largest screen with a frame rate limit
  (24, 30 or 60 fps), from the wallpaper panel, the card menu or *Settings → Performance → Optimize All Videos*.
  New videos are optimized automatically (can be turned off); videos that are already light are left alone.
- A video paused for a configurable interval (10 s – 5 min, or never) is unloaded from memory; its exact paused
  frame stays on screen and playback resumes from the same spot.
- Lock screen frame: choose which frame of a video goes to the lock screen with the timeline slider and
  frame-by-frame buttons under the preview, then *Use Current Frame*.
- Turning the lock-screen option off restores the macOS wallpaper used before LiveWall.
- Safe hotkey assignment: combinations used by macOS (Spotlight, screenshots, Mission Control, input sources,
  including ones changed in System Settings) and common app commands (⌘ + key, ⌘Tab, ⌃⌘Q, …) can't be
  assigned; LiveWall reports what uses them.
- The DMG includes *Install LiveWall.command*, which copies the app to Applications, clears the download flag and
  starts it.
- The wallpaper panel shows the video's resolution, frame rate and codec.

### Changed
- Library previews play on their own instead of mirroring the desktop wallpaper; when the wallpaper is paused,
  previews stop on the same frame and show a *Paused* badge.
- GIFs are animated in the library and pause with the wallpaper.
- Images no longer show pause and sound controls (library, menu bar).
- The lock-screen option is named *Show the wallpaper on the lock screen* and also covers a separately chosen
  lock-screen wallpaper.
- The wallpaper panel preview has a margin and rounded corners; the panel scrolls when it doesn't fit.

### Fixed
- GIF wallpapers didn't always pause.
- Panel controls (hotkey scope, speed) could overflow the panel.
- The macOS wallpaper is no longer changed while the screen is locked or right after unlocking, and isn't set
  again when the correct frame is already in place.

### Performance
- Window coverage is checked only while a video or GIF is playing, less often and with timer tolerance; the power
  source is reported by the system instead of being polled.
- A paused GIF costs nothing: its animation is removed until it resumes.
- Closing the library window frees its interface, thumbnails, images and preview players.
- At most three thumbnails are generated at a time; the library index is saved once per burst of changes.
- Local videos start without extra buffering and keep a smaller frame buffer.

## [1.0.0] - 2026-10-13

### Added
- Videos (mp4, mov, m4v), images (png, jpg, heic, webp) and animated GIFs as desktop wallpaper on every display
  and Space.
- Adding files and folders, drag & drop, and pasting a link (direct file, or a page with `og:video` / `<video>` /
  `og:image`).
- Per-wallpaper fill mode (Fill, Fit, Stretch, Center), volume, mute and playback speed.
- Lock screen: a still frame of the desktop wallpaper or of a separately chosen one.
- Full-screen library with live Desktop and Lock Screen previews and a panel for every wallpaper.
- Folders with a built-in icon pack or custom images; password-locked folders; renaming of wallpapers and folders.
- Per-wallpaper toggle hotkeys for the desktop, the lock screen or both — also for wallpapers in locked folders.
- Global hotkeys, menu bar menu, Dock icon, launch at login.
- Smart pause: fullscreen apps, covered desktop, focused app, battery, Low Power Mode, heat, app rules, sleep and
  lock.
- Automatic rotation by timer.
- Eight color styles, custom styles with import/export, English and Russian interface.
- One-line installer, DMG and universal (Apple Silicon + Intel) builds published automatically for tagged
  releases.
