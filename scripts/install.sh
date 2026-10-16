#!/bin/bash
set -euo pipefail

REPO="BelHetile/LiveWall"
APP="/Applications/LiveWall.app"
ASSET="LiveWall-macOS.zip"
BUNDLE_ID="io.github.belhetile.livewall"

say() { printf '\033[1;35m==>\033[0m %s\n' "$1"; }
fail() {
    printf '\033[1;31mError:\033[0m %s\n' "$1" >&2
    exit 1
}

running() { pgrep -x -u "$USER" LiveWall >/dev/null 2>&1; }

quit_running() {
    running || return 0
    say "Quitting the running LiveWall…"
    osascript -e "tell application id \"$BUNDLE_ID\" to quit" >/dev/null 2>&1 || true
    for _ in {1..20}; do
        running || return 0
        sleep 0.5
    done
    pkill -x -u "$USER" LiveWall 2>/dev/null || true
    for _ in {1..10}; do
        running || return 0
        sleep 0.5
    done
    fail "LiveWall did not quit. Quit it from the menu bar and run the installer again."
}

[[ "$(uname -s)" == "Darwin" ]] || fail "LiveWall runs on macOS only."
[[ "$(uname -m)" == "arm64" ]] || fail "LiveWall needs a Mac with Apple Silicon."
major="$(sw_vers -productVersion | cut -d. -f1)"
((major >= 15)) || fail "LiveWall needs macOS 15 Sequoia or newer (you have $(sw_vers -productVersion))."

say "Looking up the latest LiveWall release…"
release="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest")" || true
asset_url() {
    printf '%s\n' "$release" |
        grep -o "\"browser_download_url\": *\"[^\"]*/$1\"" |
        head -1 | sed -E 's/.*"(https[^"]+)"/\1/' || true
}
url="$(asset_url "$ASSET")"
sums_url="$(asset_url SHA256SUMS.txt)"
[[ -n "$url" ]] || fail "Could not find a release to download. See https://github.com/$REPO/releases"
[[ -n "$sums_url" ]] || fail "The release has no SHA256SUMS.txt."

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

say "Downloading ${ASSET}…"
curl -fL --progress-bar "$url" -o "$tmp/$ASSET"

say "Verifying the download…"
curl -fsSL "$sums_url" -o "$tmp/SHA256SUMS.txt" || fail "Could not download SHA256SUMS.txt."
grep -F " $ASSET" "$tmp/SHA256SUMS.txt" >"$tmp/$ASSET.sha256" || fail "SHA256SUMS.txt has no entry for $ASSET."
(cd "$tmp" && shasum -a 256 -c "$ASSET.sha256" >/dev/null) || fail "The download failed its integrity check."

ditto -x -k "$tmp/$ASSET" "$tmp"
[[ -d "$tmp/LiveWall.app" ]] || fail "The download does not contain LiveWall.app."

quit_running

say "Installing to /Applications…"
SUDO=""
if [[ ! -w /Applications ]] || { [[ -e "$APP" ]] && [[ ! -w "$APP" ]]; }; then
    SUDO="sudo"
    echo "    (your password is needed to write to /Applications)"
fi
$SUDO rm -rf "$APP"
$SUDO ditto "$tmp/LiveWall.app" "$APP"
$SUDO xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true

say "Launching LiveWall…"
open "$APP"
say "Done! LiveWall is in your Applications folder, the Dock and the menu bar."
say "Command line: $APP/Contents/MacOS/lwctl --help"
