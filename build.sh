#!/bin/bash
# Сборка LiveWall.app. Использование:  ./build.sh            -> build/LiveWall.app
#                                     ./build.sh --install  -> ещё и копирует в /Applications
set -euo pipefail
cd "$(dirname "$0")"

if ! command -v swift >/dev/null 2>&1; then
  echo "Не найден swift. Установите Command Line Tools:  xcode-select --install"
  exit 1
fi

ARCH="$(uname -m)"
swift build -c release --arch "$ARCH"
BIN="$(swift build -c release --arch "$ARCH" --show-bin-path 2>/dev/null)/LiveWall"
if [[ ! -f "$BIN" ]]; then
  BIN="$(find .build -type f -name LiveWall -perm -u+x 2>/dev/null | grep -i release | head -1)"
fi
if [[ -z "${BIN:-}" || ! -f "$BIN" ]]; then
  echo "Не удалось найти собранный бинарник LiveWall"
  exit 1
fi

APP="build/LiveWall.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/LiveWall"
cp Info.plist "$APP/Contents/Info.plist"
if [[ -d Resources/AppIcon.iconset ]]; then
  iconutil -c icns Resources/AppIcon.iconset -o "$APP/Contents/Resources/AppIcon.icns"
fi
codesign --force --deep -s - "$APP"

echo "Готово: $APP"

if [[ "${1:-}" == "--install" ]]; then
  pkill -x LiveWall 2>/dev/null || true
  # ждём, пока старая копия действительно закроется
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    pgrep -x LiveWall >/dev/null 2>&1 || break
    sleep 0.5
  done
  rm -rf /Applications/LiveWall.app
  cp -R "$APP" /Applications/LiveWall.app
  /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f /Applications/LiveWall.app >/dev/null 2>&1 || true
  sleep 1
  if open /Applications/LiveWall.app 2>/dev/null || { sleep 2; open /Applications/LiveWall.app; }; then
    echo "Установлено в /Applications и запущено."
  else
    echo "Установлено в /Applications. Запустите LiveWall вручную (Spotlight: LiveWall)."
  fi
fi
