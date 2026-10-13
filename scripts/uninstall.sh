#!/bin/bash
# Removes LiveWall. Your wallpapers and settings are kept unless you pass --all.
#
#   curl -fsSL https://raw.githubusercontent.com/BelHetile/LiveWall/main/scripts/uninstall.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/BelHetile/LiveWall/main/scripts/uninstall.sh | bash -s -- --all
#
set -euo pipefail

APP="/Applications/LiveWall.app"
DATA="$HOME/Library/Application Support/LiveWall"
BUNDLE_ID="io.github.belhetile.livewall"

if pgrep -x LiveWall >/dev/null 2>&1; then
  osascript -e "tell application id \"$BUNDLE_ID\" to quit" >/dev/null 2>&1 || pkill -x LiveWall 2>/dev/null || true
  sleep 1
fi

if [[ -e "$APP" ]]; then
  if [[ -w "$APP" ]]; then rm -rf "$APP"; else sudo rm -rf "$APP"; fi
  echo "Removed $APP"
else
  echo "LiveWall.app is not in /Applications."
fi

if [[ "${1:-}" == "--all" ]]; then
  rm -rf "$DATA"
  defaults delete "$BUNDLE_ID" >/dev/null 2>&1 || true
  defaults delete com.local.livewall >/dev/null 2>&1 || true
  echo "Removed your wallpapers and settings."
else
  echo "Your wallpapers and settings are still in: $DATA"
  echo "Run with --all to remove them too."
fi
