#!/usr/bin/env bash
set -euo pipefail

OUT="${1:?usage: ui-shot.sh <out.png> [ENV=VAL...]}"
shift
for pair in "$@"; do
    export "$pair"
done

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FIXTURES="${LW_UI_FIXTURES:-$HOME/Documents/livewall-design/design/fixtures/Library}"
BASE="${LW_UI_TMP:-${TMPDIR:-/tmp}}"

cargo build --quiet --manifest-path "$ROOT/Cargo.toml" -p livewall -p lwctl --bins

WORK="$(mktemp -d "$BASE/lw.XXXXXX")"
DATA="$WORK/data"
SOCK="$WORK/s"
WINDOW_FILE="$WORK/window"
mkdir -p "$DATA"
if [[ -n "${LW_UI_SETTINGS_JSON:-}" ]]; then
    printf '%s' "$LW_UI_SETTINGS_JSON" >"$DATA/settings.json"
fi

desktop_count() {
    osascript -e 'tell application "System Events" to count of desktops'
}

desktop_picture() {
    osascript -e "tell application \"System Events\" to get picture of desktop $1"
}

BEFORE=()
for index in $(seq 1 "$(desktop_count)"); do
    BEFORE+=("$(desktop_picture "$index")")
done

PID=""
finish() {
    if [[ -n "$PID" ]] && kill -0 "$PID" 2>/dev/null; then
        kill -TERM "$PID" 2>/dev/null || true
        for _ in $(seq 1 100); do
            kill -0 "$PID" 2>/dev/null || break
            sleep 0.1
        done
    fi
    for index in $(seq 1 "${#BEFORE[@]}"); do
        if [[ "$(desktop_picture "$index")" != "${BEFORE[$((index - 1))]}" ]]; then
            osascript -e "tell application \"System Events\" to set picture of desktop $index to \"${BEFORE[$((index - 1))]}\""
            echo "ui-shot: restored desktop $index picture" >&2
        fi
    done
    rm -rf "$WORK"
}
trap finish EXIT

ctl() {
    LIVEWALL_SOCKET="$SOCK" "$ROOT/target/debug/lwctl" --json "$@"
}

LIVEWALL_DATA_DIR="$DATA" LIVEWALL_SOCKET="$SOCK" LW_UI_WINDOW_FILE="$WINDOW_FILE" \
    "$ROOT/target/debug/LiveWall" &
PID=$!

for _ in $(seq 1 100); do
    [[ -s "$WINDOW_FILE" ]] && break
    kill -0 "$PID" 2>/dev/null || exit 1
    sleep 0.1
done

media() {
    find "$1" -maxdepth 1 -type f \( -iname '*.mp4' -o -iname '*.mov' -o -iname '*.png' -o -iname '*.jpg' -o -iname '*.gif' \) | sort
}

import() {
    local files=()
    while IFS= read -r file; do
        files+=("$file")
    done < <(media "$1")
    [[ ${#files[@]} -gt 0 ]] || return 0
    ctl library add "${files[@]}" | jq -r '.data.results[] | select(.status == "imported") | .id'
}

import "$FIXTURES" >/dev/null
for dir in "$FIXTURES"/*/; do
    name="$(basename "$dir")"
    folder="$(ctl folder create "$name" | jq -r '.data.id')"
    ids=()
    while IFS= read -r id; do
        ids+=("$id")
    done < <(import "$dir")
    [[ ${#ids[@]} -gt 0 ]] && ctl library move "${ids[@]}" --folder "$folder" >/dev/null
done

if [[ -z "${LW_UI_NO_APPLY:-}" ]]; then
    wallpaper="$(ctl library list | jq -r '.data.items[] | select(.title == "Perlin Fog") | .id' | head -n 1)"
    [[ -n "$wallpaper" ]] && ctl apply --item "$wallpaper" >/dev/null
fi

sleep "${LW_UI_SETTLE:-2}"
screencapture -x -o -l "$(cat "$WINDOW_FILE")" "$OUT"
echo "$OUT"
