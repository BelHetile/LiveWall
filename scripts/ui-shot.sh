#!/usr/bin/env bash
set -euo pipefail

OUT="${1:?usage: ui-shot.sh <out.png> [ENV=VAL...]}"
shift
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cargo build --quiet --manifest-path "$ROOT/Cargo.toml" -p livewall --bin lwui
WINDOW_FILE="$(mktemp -t lwui-window)"
rm -f "$WINDOW_FILE"
env LW_UI_WINDOW_FILE="$WINDOW_FILE" "$@" "$ROOT/target/debug/lwui" &
PID=$!
trap 'kill -TERM "$PID" 2>/dev/null || true; rm -f "$WINDOW_FILE"' EXIT
for _ in $(seq 1 100); do
    [[ -s "$WINDOW_FILE" ]] && break
    kill -0 "$PID" 2>/dev/null || exit 1
    sleep 0.1
done
sleep "${LW_UI_SETTLE:-2}"
screencapture -x -o -l "$(cat "$WINDOW_FILE")" "$OUT"
echo "$OUT"
