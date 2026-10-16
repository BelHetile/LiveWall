# Contributing to LiveWall

Bug reports, ideas and pull requests are welcome.

## Reporting a bug

Open an [issue](https://github.com/BelHetile/LiveWall/issues/new/choose) and include:

- your macOS version and Mac model,
- the LiveWall version (LiveWall → About LiveWall),
- what you did, what you expected and what happened,
- for a specific wallpaper: its format (mp4/mov/webm/gif/…) and, if possible, a link to it,
- optionally the log: `log show --last 10m --predicate 'subsystem == "io.github.belhetile.livewall"'`.

## Building

You need an Apple Silicon Mac with macOS 15 or newer, the Command Line Tools (`xcode-select --install`) and
[rustup](https://rustup.rs). `rust-toolchain.toml` pins the Rust version; rustup installs it on the first build.

```bash
./scripts/build.sh             # build/LiveWall.app
./scripts/build.sh --install   # also copy to /Applications and launch
```

The workspace has three crates:

- `crates/app` — the app (`LiveWall`): wallpaper engine, library window, menu bar item, IPC server.
- `crates/ctl` — `lwctl`, the command-line client.
- `crates/proto` — the IPC types shared by both.

## Tests

```bash
cargo test --workspace --lib                       # unit tests
scripts/fixtures.sh                                # test media into target/fixtures (needs ffmpeg)
LW_E2E=1 cargo test -p livewall --test e2e_desktop
LW_E2E=1 cargo test -p livewall --test e2e_hotkeys
LW_E2E=1 cargo test -p livewall --test e2e_ipc
LW_E2E=1 cargo test -p livewall --test e2e_media
```

The end-to-end suites need a logged-in desktop session, run the app with a temporary data directory and change
the real desktop picture (restored when they finish). `e2e_media` needs `scripts/fixtures.sh` first.

To run a development build without touching your own library:

```bash
LIVEWALL_DATA_DIR=/tmp/lw LIVEWALL_SOCKET=/tmp/lw.sock cargo run -p livewall --bin LiveWall
```

## Pull requests

- Keep changes focused: one feature or fix per pull request.
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace --lib`
  must pass; GitHub Actions runs them on every pull request.
- Rust 2024 edition, `objc2` bindings for Apple frameworks; no Swift or Objective-C sources.
- Interface text shown by the menu bar item and in messages goes through `resources/*.lproj`; add the Russian
  translation to `resources/ru.lproj/Localizable.strings`, and plural forms to `Localizable.stringsdict`.
- Mention user-visible changes in `CHANGELOG.md` under an *Unreleased* heading.

## Releasing (maintainers)

1. Bump `version` in `Cargo.toml` and move the *Unreleased* notes in `CHANGELOG.md` under a new heading, e.g.
   `## [2.1.0] - 2026-11-01`.
2. Tag and push (`git tag v2.1.0 && git push origin v2.1.0`), or run **Actions → Release → Run workflow** with
   the version.
3. GitHub Actions builds the app and publishes `LiveWall-macOS.zip`, `LiveWall.dmg` and `SHA256SUMS.txt`. The
   one-line installer always picks up the newest release.
