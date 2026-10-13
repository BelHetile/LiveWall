#!/bin/bash
# LiveWall installer — downloads the latest release and puts it into /Applications.
#
#   curl -fsSL https://raw.githubusercontent.com/BelHetile/LiveWall/main/install.sh | bash
#
set -euo pipefail

REPO="BelHetile/LiveWall"
APP="/Applications/LiveWall.app"
ASSET="LiveWall-macOS.zip"

say() { printf '\033[1;35m==>\033[0m %s\n' "$1"; }
fail() { printf '\033[1;31mError:\033[0m %s\n' "$1" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || fail "LiveWall runs on macOS only."
major="$(sw_vers -productVersion | cut -d. -f1)"
(( major >= 13 )) || fail "LiveWall needs macOS 13 Ventura or newer (you have $(sw_vers -productVersion))."

say "Looking up the latest LiveWall release…"
url="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
  | grep -o "\"browser_download_url\": *\"[^\"]*/$ASSET\"" \
  | head -1 | sed -E 's/.*"(https[^"]+)"/\1/')" || true
[[ -n "${url:-}" ]] || fail "Could not find a release to download. See https://github.com/$REPO/releases"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

say "Downloading $(basename "$url")…"
curl -fL --progress-bar "$url" -o "$tmp/$ASSET"
ditto -x -k "$tmp/$ASSET" "$tmp"
[[ -d "$tmp/LiveWall.app" ]] || fail "The download does not contain LiveWall.app."

if pgrep -x LiveWall >/dev/null 2>&1; then
  say "Quitting the running LiveWall…"
  osascript -e 'tell application id "io.github.belhetile.livewall" to quit' >/dev/null 2>&1 || pkill -x LiveWall || true
  sleep 1
fi

say "Installing to /Applications…"
SUDO=""
if [[ ! -w /Applications ]] || { [[ -e "$APP" ]] && [[ ! -w "$APP" ]]; }; then
  SUDO="sudo"
  echo "    (your password is needed to write to /Applications)"
fi
$SUDO rm -rf "$APP"
$SUDO ditto "$tmp/LiveWall.app" "$APP"
# downloaded with curl, so macOS does not quarantine it; clear the flag just in case
$SUDO xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true

say "Launching LiveWall…"
open "$APP"
say "Done! LiveWall is in your Applications folder, the Dock and the menu bar."
