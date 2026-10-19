#!/bin/bash

_DEV_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

DEV_DIR="$_DEV_ROOT/target/dev"
DEV_DATA="$DEV_DIR/data"
SOCKET="$DEV_DATA/control.sock"
PIDFILE="$DEV_DIR/app.pid"
BUILD_LOG="$DEV_DIR/build.log"
APP_LOG="$DEV_DIR/app.log"
WINDOW_FILE="$DEV_DIR/window"
SHOT="$DEV_DIR/shot.png"
BIN="$_DEV_ROOT/target/debug/LiveWall"
CTL="$_DEV_ROOT/target/debug/lwctl"

if (( ${#SOCKET} >= 100 )); then
    echo "dev-env: socket path is ${#SOCKET} chars, must stay under 100: $SOCKET" >&2
    exit 1
fi

dev_env() {
    export LIVEWALL_DATA_DIR="$DEV_DATA"
    export LIVEWALL_SOCKET="$SOCKET"
    export LW_UI_NO_DESKTOP=1
    export LW_UI_WINDOW_FILE="$WINDOW_FILE"
    export LIVEWALL_TEST_UNLOCKED=1
}
