#!/bin/bash
set -euo pipefail

usage() {
  cat <<'EOF'
Screenshots the dev app window, using the id written to target/dev/window.

  scripts/dev-shot.sh              write target/dev/shot.png
  scripts/dev-shot.sh out.png      write out.png instead
EOF
}

cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source scripts/dev-env.sh

OUT="$SHOT"
for arg in "$@"; do
  case "$arg" in
    -h|--help) usage; exit 0 ;;
    -*) echo "Unknown option: $arg" >&2; usage >&2; exit 64 ;;
    *) OUT="$arg" ;;
  esac
done

screen_permission_hint() {
  echo "screencapture failed — grant Screen Recording to your terminal app in" >&2
  echo "System Settings -> Privacy & Security -> Screen & System Audio Recording" >&2
}

if [[ ! -s "$WINDOW_FILE" ]]; then
  echo "no window id at $WINDOW_FILE; is the dev app running?" >&2
  exit 1
fi

num="$(tr -d '[:space:]' < "$WINDOW_FILE")"
if [[ ! "$num" =~ ^[0-9]+$ ]]; then
  echo "window file does not contain a number: $num" >&2
  exit 1
fi

if ! screencapture -l "$num" -x -o -t png "$OUT"; then
  screen_permission_hint
  exit 1
fi
if [[ ! -s "$OUT" ]]; then
  screen_permission_hint
  exit 1
fi

echo "$OUT"
sips -g pixelWidth -g pixelHeight "$OUT"
