#!/bin/bash
set -euo pipefail

cd "$(cd "$(dirname "$0")/.." && pwd)"

APP="build/LiveWall.app"
PLIST="Resources/Info.plist"
BIN="$(swift build -c release --show-bin-path)"

echo "==> Building lwctl"
swift build -c release --product lwctl

echo "==> Building LiveWall with embedded Info.plist"
swift build -c release --product LiveWall \
    -Xlinker -sectcreate -Xlinker __TEXT -Xlinker __info_plist -Xlinker "$PWD/$PLIST"

echo "==> Assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp "$BIN/LiveWall" "$APP/Contents/MacOS/LiveWall"
cp "$BIN/lwctl" "$APP/Contents/MacOS/lwctl"
cp "$PLIST" "$APP/Contents/Info.plist"

echo "==> Validating Info.plist"
plutil -lint "$APP/Contents/Info.plist"
test "$(plutil -extract LSUIElement raw "$APP/Contents/Info.plist")" = "true"

if ! otool -s __TEXT __info_plist "$APP/Contents/MacOS/LiveWall" | grep -q 'Contents of'; then
    echo "error: Info.plist is not embedded in the LiveWall binary" >&2
    exit 1
fi

echo "==> Signing ad-hoc"
codesign --force -s - "$APP"
codesign --verify --verbose=2 "$APP"

echo "==> Launch/kill smoke"
"$APP/Contents/MacOS/LiveWall" >/dev/null 2>&1 &
PID=$!
sleep 1
if ! kill -0 "$PID" 2>/dev/null; then
    echo "error: LiveWall did not stay running during smoke test" >&2
    exit 1
fi
kill "$PID"
wait "$PID" 2>/dev/null || true

echo "==> OK: $APP"
