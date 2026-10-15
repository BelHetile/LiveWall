#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="$root/target/fixtures"
mkdir -p "$out"

if ! command -v ffmpeg >/dev/null 2>&1; then
    echo "fixtures.sh: ffmpeg not found (brew install ffmpeg)" >&2
    exit 1
fi

encoders="$(ffmpeg -hide_banner -encoders 2>/dev/null || true)"
hevc="libx265"
if grep -q hevc_videotoolbox <<<"$encoders"; then
    hevc="hevc_videotoolbox"
fi

ffmpeg -y -loglevel error \
    -f lavfi -i testsrc2=size=1920x1080:rate=30 \
    -f lavfi -i sine=frequency=440:sample_rate=48000 \
    -t 3 -c:v "$hevc" -b:v 1500k -pix_fmt yuv420p -tag:v hvc1 \
    -c:a aac -b:a 48k -shortest "$out/clip-hevc-1080p30.mp4"

ffmpeg -y -loglevel error \
    -f lavfi -i testsrc2=size=1280x720:rate=60 \
    -f lavfi -i sine=frequency=660:sample_rate=48000 \
    -t 3 -c:v libx264 -b:v 1500k -pix_fmt yuv420p \
    -c:a aac -b:a 48k -shortest "$out/clip-h264-720p60.mp4"

ffmpeg -y -loglevel error \
    -f lavfi -i testsrc2=size=1080x1920:rate=30 \
    -t 3 -c:v libx264 -b:v 1500k -pix_fmt yuv420p "$out/portrait.mp4"

frames="$(mktemp -d)"
trap 'rm -rf "$frames"' EXIT
ffmpeg -y -loglevel error -f lavfi -i testsrc2=size=320x240:rate=10 -frames:v 8 "$frames/f%02d.png"

list="$frames/list.txt"
: >"$list"
delays=(0.05 0.1 0.2 0.1 0.4 0.1 0.2 0.3)
index=0
for delay in "${delays[@]}"; do
    index=$((index + 1))
    printf "file 'f%02d.png'\nduration %s\n" "$index" "$delay" >>"$list"
done
printf "file 'f%02d.png'\n" "$index" >>"$list"

ffmpeg -y -loglevel error -f concat -safe 0 -i "$list" -vf "scale=320:240:flags=neighbor" -loop 0 "$out/anim.gif"

ffmpeg -y -loglevel error -f lavfi -i testsrc2=size=1920x1200:rate=1 -frames:v 1 "$out/still.png"

ls -l "$out"
echo "fixtures.sh: wrote fixtures to $out"
