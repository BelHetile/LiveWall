#!/bin/bash
set -euo pipefail

usage() {
  cat <<'EOF'
Builds LiveWall.app from source.

  scripts/build.sh             build for this Mac          -> build/LiveWall.app
  scripts/build.sh --install   build, copy to /Applications and launch

LIVEWALL_VERSION=2.1.0 scripts/build.sh   sets the version shown in About.
LIVEWALL_BUILD=42 scripts/build.sh        sets the build number (default: commit count).
EOF
}

cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

INSTALL=0
for arg in "$@"; do
  case "$arg" in
    --install) INSTALL=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $arg" >&2; usage >&2; exit 64 ;;
  esac
done

[[ "$(uname -m)" == "arm64" ]] || { echo "LiveWall builds for Apple Silicon (arm64) only." >&2; exit 1; }
command -v cargo >/dev/null 2>&1 || { echo "cargo not found. Install Rust first: https://rustup.rs" >&2; exit 1; }
command -v iconutil >/dev/null 2>&1 || { echo "iconutil not found. Install the Command Line Tools: xcode-select --install" >&2; exit 1; }

VERSION="${LIVEWALL_VERSION:-$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')}"
VERSION="${VERSION#v}"
[[ -n "$VERSION" ]] || { echo "Could not read the version from Cargo.toml." >&2; exit 1; }
if [[ -n "${LIVEWALL_BUILD:-}" ]]; then
  BUILD="$LIVEWALL_BUILD"
elif git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  BUILD="$(git rev-list --count HEAD)"
else
  BUILD="1"
fi

echo "==> Building LiveWall $VERSION ($BUILD)"
export MACOSX_DEPLOYMENT_TARGET=15.0
cargo build --release --locked --bin LiveWall --bin lwctl
TARGET="$(cargo metadata --format-version 1 --no-deps | sed -E 's/.*"target_directory":"([^"]+)".*/\1/')/release"

APP="build/LiveWall.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$TARGET/LiveWall" "$TARGET/lwctl" "$APP/Contents/MacOS/"
cp resources/Info.plist "$APP/Contents/Info.plist"
cp -R resources/en.lproj resources/ru.lproj "$APP/Contents/Resources/"
iconutil -c icns resources/AppIcon.iconset -o "$APP/Contents/Resources/AppIcon.icns"
plutil -replace CFBundleShortVersionString -string "$VERSION" "$APP/Contents/Info.plist"
plutil -replace CFBundleVersion -string "$BUILD" "$APP/Contents/Info.plist"
plutil -lint -s "$APP/Contents/Info.plist"

codesign --force --sign - "$APP/Contents/MacOS/lwctl"
codesign --force --sign - "$APP"
codesign --verify --strict "$APP"
echo "==> Built $APP ($(lipo -archs "$APP/Contents/MacOS/LiveWall"))"

[[ "$INSTALL" == 1 ]] || exit 0

if pgrep -x -u "$USER" LiveWall >/dev/null 2>&1; then
  osascript -e 'tell application id "io.github.belhetile.livewall" to quit' >/dev/null 2>&1 || true
  for _ in {1..20}; do
    pgrep -x -u "$USER" LiveWall >/dev/null 2>&1 || break
    sleep 0.5
  done
fi
if pgrep -x -u "$USER" LiveWall >/dev/null 2>&1; then
  echo "LiveWall is still running; quit it and run the script again." >&2
  exit 1
fi
rm -rf /Applications/LiveWall.app
ditto "$APP" /Applications/LiveWall.app
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f /Applications/LiveWall.app >/dev/null 2>&1 || true
open /Applications/LiveWall.app
echo "==> Installed to /Applications and launched."
