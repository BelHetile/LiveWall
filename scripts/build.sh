#!/bin/bash
# Builds LiveWall.app from source.
#
#   scripts/build.sh               build for this Mac            -> build/LiveWall.app
#   scripts/build.sh --install     build, copy to /Applications and launch
#   scripts/build.sh --universal   Apple Silicon + Intel binary  (used for releases)
#
# LIVEWALL_VERSION=1.2.3 scripts/build.sh   sets the version shown in About.
set -euo pipefail
SCRIPT="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
# run from the repository root, wherever the script is called from
cd "$(dirname "$SCRIPT")/.."

INSTALL=0
UNIVERSAL=0
for arg in "$@"; do
  case "$arg" in
    --install) INSTALL=1 ;;
    --universal) UNIVERSAL=1 ;;
    -h|--help) sed -n '2,9p' "$SCRIPT"; exit 0 ;;
    *) echo "Unknown option: $arg"; exit 1 ;;
  esac
done

if ! command -v swift >/dev/null 2>&1; then
  echo "Swift not found. Install the Command Line Tools first:  xcode-select --install"
  exit 1
fi

OUT="build"
STAGE="$OUT/bin"
rm -rf "$STAGE"
mkdir -p "$STAGE"

# Finds the freshly built binary that contains the given architecture.
find_binary() {
  local arch="$1" path
  path="$(swift build -c release --arch "$arch" --show-bin-path 2>/dev/null)/LiveWall"
  if [[ -f "$path" ]] && lipo -archs "$path" 2>/dev/null | grep -qw "$arch"; then
    echo "$path"; return
  fi
  while IFS= read -r candidate; do
    if lipo -archs "$candidate" 2>/dev/null | grep -qw "$arch"; then
      echo "$candidate"; return
    fi
  done < <(find .build -type f -name LiveWall -perm -u+x 2>/dev/null | grep -i release)
}

build_arch() {
  local arch="$1" bin
  echo "==> Building for $arch"
  swift build -c release --arch "$arch"
  bin="$(find_binary "$arch")"
  if [[ -z "$bin" ]]; then
    echo "Could not find the built LiveWall binary for $arch"
    exit 1
  fi
  # copy right away: some build systems reuse one output folder for every architecture
  cp "$bin" "$STAGE/LiveWall-$arch"
}

if [[ "$UNIVERSAL" == 1 ]]; then
  build_arch arm64
  build_arch x86_64
  lipo -create "$STAGE/LiveWall-arm64" "$STAGE/LiveWall-x86_64" -output "$STAGE/LiveWall"
else
  ARCH="$(uname -m)"
  build_arch "$ARCH"
  cp "$STAGE/LiveWall-$ARCH" "$STAGE/LiveWall"
fi

APP="$OUT/LiveWall.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$STAGE/LiveWall" "$APP/Contents/MacOS/LiveWall"
cp Resources/Info.plist "$APP/Contents/Info.plist"
if [[ -n "${LIVEWALL_VERSION:-}" ]]; then
  plutil -replace CFBundleShortVersionString -string "$LIVEWALL_VERSION" "$APP/Contents/Info.plist"
fi
if [[ -d Resources/AppIcon.iconset ]]; then
  iconutil -c icns Resources/AppIcon.iconset -o "$APP/Contents/Resources/AppIcon.icns"
fi
codesign --force --deep -s - "$APP"

echo "==> Built $APP ($(lipo -archs "$APP/Contents/MacOS/LiveWall"))"

if [[ "$INSTALL" == 1 ]]; then
  pkill -x LiveWall 2>/dev/null || true
  # wait until the running copy has really quit
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    pgrep -x LiveWall >/dev/null 2>&1 || break
    sleep 0.5
  done
  rm -rf /Applications/LiveWall.app
  cp -R "$APP" /Applications/LiveWall.app
  /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f /Applications/LiveWall.app >/dev/null 2>&1 || true
  sleep 1
  if open /Applications/LiveWall.app 2>/dev/null || { sleep 2; open /Applications/LiveWall.app; }; then
    echo "==> Installed to /Applications and launched."
  else
    echo "==> Installed to /Applications. Launch LiveWall from Spotlight."
  fi
fi
