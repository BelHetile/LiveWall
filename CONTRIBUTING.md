# Contributing to LiveWall

Thanks for helping! Bug reports, ideas and pull requests are all welcome.

## Reporting a bug

Open an [issue](https://github.com/BelHetile/LiveWall/issues/new/choose) and include:

- your macOS version and Mac model (Apple Silicon or Intel),
- the LiveWall version (LiveWall → About LiveWall),
- what you did, what you expected and what happened,
- for a specific wallpaper: its format (mp4/mov/gif/…) and, if possible, a link to it.

## Building

```bash
xcode-select --install   # once
./build.sh               # build/LiveWall.app
./build.sh --install     # also copy to /Applications and launch
```

The project is a plain Swift package (`Package.swift`, sources in `Sources/LiveWall`), so you can also open the
folder in Xcode.

## Pull requests

- Keep changes focused; one feature or fix per pull request.
- Match the existing style (SwiftUI for the interface, AppKit where macOS needs it).
- New interface text goes through `L("…")` / `LF("…", …)`; add the Russian translation to
  `Sources/LiveWall/RussianStrings.swift`.
- Make sure `./build.sh` succeeds — every pull request is also built by GitHub Actions.
- Mention user-visible changes in `CHANGELOG.md` under an *Unreleased* heading.

## Releasing (maintainers)

1. Move the *Unreleased* notes in `CHANGELOG.md` under a new version heading, e.g. `## [1.1.0] - 2026-11-01`.
2. Tag and push: `git tag v1.1.0 && git push origin v1.1.0`.
3. GitHub Actions builds the universal app and publishes the release with `LiveWall-macOS.zip`, `LiveWall.dmg`
   and checksums. The one-line installer always picks up the newest release.
