#!/bin/bash
set -euo pipefail

cd "$(cd "$(dirname "$0")/.." && pwd)"

OUT="Tests/Fixtures"

if ! command -v ffmpeg >/dev/null 2>&1; then
    echo "error: ffmpeg is required to generate fixtures" >&2
    exit 1
fi

mkdir -p "$OUT"

COMMON=(-fflags +bitexact -flags +bitexact -map_metadata -1 -threads 1)

ffmpeg -y -v error \
    -f lavfi -i "smptebars=size=1920x1080:rate=30" \
    -f lavfi -i "sine=frequency=440:sample_rate=48000" \
    -t 2 \
    -c:v libx265 -pix_fmt yuv420p -preset medium \
    -x265-params pools=1:frame-threads=1:no-info=1 \
    -tag:v hvc1 \
    -c:a aac -b:a 64k \
    -movflags +faststart \
    "${COMMON[@]}" \
    "$OUT/clip-hevc-1080p30.mp4"

ffmpeg -y -v error \
    -f lavfi -i "smptebars=size=1280x720:rate=60" \
    -t 2 \
    -c:v libx264 -pix_fmt yuv420p -preset medium \
    -x264-params sliced-threads=0:threads=1 \
    -movflags +faststart \
    "${COMMON[@]}" \
    "$OUT/clip-h264-720p60.mp4"

ffmpeg -y -v error \
    -f lavfi -i "smptebars=size=1920x1080:rate=30" \
    -t 2 \
    -c:v libsvtav1 -pix_fmt yuv420p -preset 12 -crf 40 \
    -svtav1-params lp=1 \
    -movflags +faststart \
    "${COMMON[@]}" \
    "$OUT/clip-av1-1080p.mp4"

ffmpeg -y -v error \
    -f lavfi -i "testsrc2=size=160x120:rate=10" \
    -frames:v 8 -loop 0 \
    "$OUT/anim.gif"

ffmpeg -y -v error \
    -f lavfi -i "testsrc2=size=320x240:rate=1" \
    -frames:v 1 \
    "$OUT/still.png"

ffmpeg -y -v error \
    -f lavfi -i "sine=frequency=440:sample_rate=48000" \
    -t 2 \
    -c:a aac -b:a 64k \
    "${COMMON[@]}" \
    "$OUT/audio.m4a"

head -c 4096 "$OUT/clip-h264-720p60.mp4" > "$OUT/corrupt.mp4"
: > "$OUT/zero-bytes.mp4"

echo "==> Generated fixtures in $OUT"
ls -l "$OUT"
