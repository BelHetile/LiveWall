#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

version="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
if [[ -z "$version" ]]; then
    echo "bundle.sh: could not read workspace version" >&2
    exit 1
fi

cargo build --release --bin LiveWall --bin lwctl

app="build/LiveWall.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

cp target/release/LiveWall "$app/Contents/MacOS/LiveWall"
cp target/release/lwctl "$app/Contents/MacOS/lwctl"
cp resources/Info.plist "$app/Contents/Info.plist"

iconutil -c icns resources/AppIcon.iconset -o "$app/Contents/Resources/AppIcon.icns"

cp -R resources/en.lproj "$app/Contents/Resources/en.lproj"
cp -R resources/ru.lproj "$app/Contents/Resources/ru.lproj"

plutil -replace CFBundleShortVersionString -string "$version" "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"

codesign --force --sign - "$app/Contents/MacOS/lwctl"
codesign --force --sign - "$app"
codesign --verify --verbose=2 "$app"

echo "bundle.sh: built $app (version $version)"
