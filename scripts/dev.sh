#!/bin/bash
set -euo pipefail

usage() {
  cat <<'EOF'
Watches crates/**/*.rs, Cargo.toml/lock and resources/*.lproj, rebuilds
target/debug/LiveWall + lwctl and relaunches the dev app on every change.

  scripts/dev.sh            watch, rebuild and relaunch until ctrl-c
  scripts/dev.sh --fresh    wipe target/dev/data first, then seed once
  scripts/dev.sh --once     build and launch once, leave the app running

The app runs with its own data dir and control socket under target/dev.
LW_UI_LANG=ru (or en) overrides the system language for the dev app only.
EOF
}

cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source scripts/dev-env.sh

FRESH=0
ONCE=0
for arg in "$@"; do
  case "$arg" in
    --fresh) FRESH=1 ;;
    --once) ONCE=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $arg" >&2; usage >&2; exit 64 ;;
  esac
done

case "$DEV_DATA" in
  "$PWD"/*) : ;;
  *) echo "Refusing to run: dev data dir escapes the repo: $DEV_DATA" >&2; exit 1 ;;
esac

STAMP="$DEV_DIR/stamp"
DEBOUNCE="$DEV_DIR/debounce"

mkdir -p "$DEV_DIR"
if [[ "$FRESH" == 1 ]]; then
  rm -rf "$DEV_DATA"
fi
mkdir -p "$DEV_DATA"
touch "$STAMP"

changed_since() {
  local ref="$1"
  if [[ Cargo.toml -nt "$ref" ]] || [[ Cargo.lock -nt "$ref" ]]; then
    return 0
  fi
  if [[ -n "$(find crates -name '*.rs' -newer "$ref" -print -quit 2>/dev/null)" ]]; then
    return 0
  fi
  if [[ -n "$(find resources -newer "$ref" -print -quit 2>/dev/null)" ]]; then
    return 0
  fi
  return 1
}

stop_app() {
  [[ -f "$PIDFILE" ]] || return 0
  local pid
  pid="$(cat "$PIDFILE" 2>/dev/null || true)"
  [[ -n "$pid" ]] || return 0
  kill "$pid" 2>/dev/null || true
  local waited=0
  while kill -0 "$pid" 2>/dev/null && (( waited < 50 )); do
    sleep 0.1
    waited=$((waited + 1))
  done
  if kill -0 "$pid" 2>/dev/null; then
    kill -9 "$pid" 2>/dev/null || true
  fi
}

relaunch() {
  stop_app
  rm -f "$SOCKET" "$WINDOW_FILE"
  dev_env
  : > "$APP_LOG"
  local args=()
  if [[ -n "${LW_UI_LANG:-}" ]]; then
    args=(-AppleLanguages "($LW_UI_LANG)")
  fi
  nohup "$BIN" ${args[@]+"${args[@]}"} >>"$APP_LOG" 2>&1 &
  local pid="$!"
  echo "$pid" > "$PIDFILE"
  local waited=0
  while [[ ! -s "$WINDOW_FILE" ]]; do
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "dev app died during start; last 20 lines of app.log:" >&2
      tail -n 20 "$APP_LOG" >&2 || true
      return 1
    fi
    if (( waited >= 100 )); then
      echo "timed out waiting for $WINDOW_FILE; app is still running" >&2
      return 1
    fi
    sleep 0.1
    waited=$((waited + 1))
  done
  echo "window $(tr -d '[:space:]' < "$WINDOW_FILE")"
  return 0
}

cleanup() {
  stop_app
  exit 0
}
trap cleanup INT TERM

echo "==> dev loop; watching for changes (ctrl-c to stop)"

FIRST=1
SEED="$FRESH"
while :; do
  changed=0
  if [[ "$FIRST" == 1 ]]; then
    changed=1
  elif changed_since "$STAMP"; then
    touch "$DEBOUNCE"
    sleep 0.3
    while changed_since "$DEBOUNCE"; do
      touch "$DEBOUNCE"
      sleep 0.3
    done
    changed=1
  fi

  if [[ "$changed" == 1 ]]; then
    FIRST=0
    touch "$STAMP"
    start="$(date +%s)"
    if cargo build --bin LiveWall --bin lwctl --locked >"$BUILD_LOG" 2>&1; then
      echo "built in $(( $(date +%s) - start ))s"
      if relaunch; then
        if [[ "$SEED" == 1 ]]; then
          scripts/dev-seed.sh || echo "seed failed (continuing)" >&2
          SEED=0
        fi
        if [[ "$ONCE" == 1 ]]; then
          exit 0
        fi
      elif [[ "$ONCE" == 1 ]]; then
        exit 1
      fi
    else
      errors="$(grep -c '^error' "$BUILD_LOG" 2>/dev/null || true)"
      [[ -n "$errors" ]] || errors=0
      echo "build FAILED ($errors errors) — see target/dev/build.log" >&2
      grep -A12 -m1 '^error' "$BUILD_LOG" >&2 || true
      if [[ "$ONCE" == 1 ]]; then
        exit 1
      fi
    fi
  fi

  sleep 0.5
done
