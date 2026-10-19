#!/bin/bash
set -euo pipefail

usage() {
  cat <<'EOF'
Seeds the running dev app (scripts/dev.sh) with fixtures and folders.

  scripts/dev-seed.sh          seed once; prints "already seeded" if not empty

Reads and writes through target/debug/lwctl on target/dev/data/control.sock.
Generates target/fixtures/ via scripts/fixtures.sh when the fixtures are missing.
EOF
}

cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source scripts/dev-env.sh

for arg in "$@"; do
  case "$arg" in
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $arg" >&2; usage >&2; exit 64 ;;
  esac
done

command -v jq >/dev/null 2>&1 || { echo "jq not found (brew install jq)" >&2; exit 1; }
[[ -x "$CTL" ]] || { echo "lwctl not built: $CTL (run scripts/dev.sh first)" >&2; exit 2; }

export LIVEWALL_SOCKET="$SOCKET"

deadline=$((SECONDS + 10))
while ! "$CTL" status >/dev/null 2>&1; do
  if (( SECONDS >= deadline )); then
    echo "dev app is not running (no reply on $SOCKET)" >&2
    exit 2
  fi
  sleep 0.5
done

if [[ "$("$CTL" --json library list | jq -r '.data.items | length')" -gt 0 ]]; then
  echo "already seeded"
  exit 0
fi

FIXTURES="$PWD/target/fixtures"
if [[ ! -f "$FIXTURES/clip-h264-720p60.mp4" ]]; then
  if ! command -v ffmpeg >/dev/null 2>&1; then
    echo "fixtures are missing and ffmpeg is not installed (brew install ffmpeg)" >&2
    exit 1
  fi
  echo "==> generating fixtures"
  scripts/fixtures.sh
fi

SEED_DIR="$DEV_DIR/seed"
rm -rf "$SEED_DIR"
mkdir -p "$SEED_DIR"

seed_copy() {
  cp "$FIXTURES/$1" "$SEED_DIR/$2"
}

seed_copy clip-h264-720p60.mp4 "очень длинное название обоев для проверки усечения текста в сетке.mp4"
seed_copy clip-h264-720p60.mp4 "Обои для работы 2024 финал.mp4"
seed_copy clip-h264-720p60.mp4 "222222222222222222222222222222222222222222.mp4"
seed_copy clip-hevc-1080p30.mp4 "clip-hevc-1080p30.mp4"
seed_copy clip-hevc-1080p30.mp4 "трёхсекундный hevc ролик.mp4"
seed_copy portrait.mp4 "вертикальное видео 1080x1920.mp4"
seed_copy clip-4k-hevc-60.mp4 "4k hevc 60fps stress test.mp4"
seed_copy anim.gif "анимация.gif"
seed_copy anim.gif "animation with a deliberately long file name.gif"
seed_copy still.png "still-1920x1200.png"
seed_copy still.png "Картинка для проверки усечения.png"
if [[ -f "$FIXTURES/clip-vp9.webm" ]]; then
  seed_copy clip-vp9.webm "vp9 sample.webm"
fi
if [[ -f "$FIXTURES/clip-av1-1080p.mp4" ]]; then
  seed_copy clip-av1-1080p.mp4 "av1 1080p sample.mp4"
fi

create_folder() {
  "$CTL" --json folder create "$1" | jq -r '.data.id'
}

add_to() {
  local folder="$1"
  shift
  if [[ -n "$folder" ]]; then
    "$CTL" --json library add --folder "$folder" "$@" | jq -r '.data.results[] | select(.status=="imported") | .id'
  else
    "$CTL" --json library add "$@" | jq -r '.data.results[] | select(.status=="imported") | .id'
  fi
}

echo "==> creating folders"
nature="$(create_folder "Nature")"
rabota="$(create_folder "Работа")"
longname="$(create_folder "A folder with a deliberately very long name that must truncate")"
private="$(create_folder "Private")"

echo "==> adding library items"
nature_ids="$(add_to "$nature" \
  "$SEED_DIR/очень длинное название обоев для проверки усечения текста в сетке.mp4" \
  "$SEED_DIR/Обои для работы 2024 финал.mp4" \
  "$SEED_DIR/анимация.gif" \
  "$SEED_DIR/still-1920x1200.png")"
rabota_ids="$(add_to "$rabota" \
  "$SEED_DIR/вертикальное видео 1080x1920.mp4" \
  "$SEED_DIR/Картинка для проверки усечения.png" \
  "$SEED_DIR/clip-hevc-1080p30.mp4")"
longname_ids="$(add_to "$longname" \
  "$SEED_DIR/4k hevc 60fps stress test.mp4" \
  "$SEED_DIR/трёхсекундный hevc ролик.mp4")"
private_ids="$(add_to "" \
  "$SEED_DIR/animation with a deliberately long file name.gif" \
  "$SEED_DIR/222222222222222222222222222222222222222222.mp4")"
unfiled_ids="$(add_to "" "$SEED_DIR/vp9 sample.webm" "$SEED_DIR/av1 1080p sample.mp4" 2>/dev/null || true)"

if [[ -n "$private_ids" ]]; then
  private_list=()
  while IFS= read -r id; do
    [[ -n "$id" ]] || continue
    private_list+=("$id")
  done <<< "$private_ids"
  "$CTL" --json library move "${private_list[@]}" --folder "$private" >/dev/null
fi

if [[ -n "$nature_ids" ]]; then
  first="$(printf '%s\n' "$nature_ids" | sed -n '1p')"
  "$CTL" --json library rename "$first" --title "Очень длинное название обоев, которое должно усечься в сетке" >/dev/null
fi

echo "==> styling folders"
"$CTL" --json folder style "$nature" --icon symbol:leaf --color green >/dev/null
"$CTL" --json folder style "$rabota" --icon symbol:briefcase --color blue >/dev/null

echo "==> locking Private"
"$CTL" --json folder lock "$private" --locked true >/dev/null

echo "seeded: 4 folders, $(printf '%s\n' "$nature_ids" "$rabota_ids" "$longname_ids" "$private_ids" "$unfiled_ids" | grep -c .) items"
