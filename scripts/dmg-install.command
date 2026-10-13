#!/bin/bash
# LiveWall — установщик из образа диска / installer from the disk image.
# Копирует LiveWall.app в «Программы», снимает карантин macOS и запускает приложение.
# Copies LiveWall.app to Applications, clears the macOS quarantine flag and launches it.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
SRC="$HERE/LiveWall.app"
APP="/Applications/LiveWall.app"

say() { printf '\033[1;35m==>\033[0m %s\n' "$1"; }
fail() { printf '\033[1;31mError:\033[0m %s\n' "$1" >&2; read -r -p "Press Return to close… " _ || true; exit 1; }

[[ -d "$SRC" ]] || fail "LiveWall.app is not next to this script."
major="$(sw_vers -productVersion | cut -d. -f1)"
(( major >= 13 )) || fail "LiveWall needs macOS 13 Ventura or newer (you have $(sw_vers -productVersion))."

if pgrep -x LiveWall >/dev/null 2>&1; then
  say "Quitting the running LiveWall…"
  osascript -e 'tell application id "io.github.belhetile.livewall" to quit' >/dev/null 2>&1 || pkill -x LiveWall || true
  sleep 1
fi

say "Installing LiveWall to /Applications…"
SUDO=""
if [[ ! -w /Applications ]] || { [[ -e "$APP" ]] && [[ ! -w "$APP" ]]; }; then
  SUDO="sudo"
  echo "    (your Mac password is needed to write to /Applications)"
fi
$SUDO rm -rf "$APP"
$SUDO ditto "$SRC" "$APP"
# файл скачан из интернета — без этого macOS спросит про «неизвестного разработчика»
$SUDO xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true

say "Launching LiveWall…"
open "$APP"
say "Done! You can close this window and eject the LiveWall disk."
