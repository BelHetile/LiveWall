#!/bin/bash
set -euo pipefail

cd "$(cd "$(dirname "$0")/.." && pwd)"

PLUGIN_PATH="/Library/Developer/CommandLineTools/usr/lib/swift/host/plugins/testing"
FIXTURES=(
    clip-hevc-1080p30.mp4
    clip-h264-720p60.mp4
    clip-av1-1080p.mp4
    anim.gif
    still.png
    audio.m4a
    corrupt.mp4
    zero-bytes.mp4
)

stage() { printf '\n==> %s\n' "$1"; }
fail() { printf 'error: %s\n' "$1" >&2; exit 1; }

stage "1/5 debug build"
swift build

stage "2/5 tests"
if swift test; then
    echo "test path: plain swift test"
else
    echo "plain swift test failed; retrying with Testing plugin path" >&2
    if swift test -Xswiftc -plugin-path -Xswiftc "$PLUGIN_PATH"; then
        echo "test path: flagged (TestingMacros plugin path)"
    else
        fail "tests failed on both plain and flagged invocations"
    fi
fi

stage "3/5 release build"
swift build -c release

stage "4/5 app assembly smoke"
./scripts/build.sh

stage "5/5 fixture presence"
for name in "${FIXTURES[@]}"; do
    path="Tests/Fixtures/$name"
    [ -f "$path" ] || fail "missing fixture: $path"
    if [ "$name" = "zero-bytes.mp4" ]; then
        [ -s "$path" ] && fail "zero-bytes.mp4 must be empty (rejection fixture)"
    else
        [ -s "$path" ] || fail "empty fixture: $path"
    fi
done
echo "all ${#FIXTURES[@]} fixtures present (zero-bytes.mp4 intentionally empty)"

printf '\n==> check.sh: OK\n'
