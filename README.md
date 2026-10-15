# LiveWall

Native macOS animated-wallpaper engine, rewritten in pure Rust. One `.app` owns
the whole stack: per-display and per-Space wallpaper surfaces, a menu-bar item,
the IPC server, and `lwctl`. Video, GIF and image wallpapers with hardware
decode; the desktop picture is taken over and restored on quit or after a crash.
The GUI is deferred — `lwctl` drives every feature until it exists.

## Requirements

- macOS 15 or newer on Apple Silicon (arm64). Intel and older macOS are unsupported.
- Rust with the 2024 edition (CommandLineTools only; no Swift, no Xcode).
- Optional, for transcoding on import and for downloads: Homebrew `ffmpeg`,
  `yt-dlp`, `deno`. Plain `mp4`/`mov` import and playback need none of them.

## Build

    scripts/bundle.sh

Builds a release binary and assembles `build/LiveWall.app` (`lwctl` lives in
`Contents/MacOS`), then ad-hoc codesigns it.

## Run

    build/LiveWall.app/Contents/MacOS/LiveWall

## Test

    cargo test                     # unit tests for the pure modules
    scripts/fixtures.sh            # ffmpeg-generated media into target/fixtures
    LW_E2E=1 cargo test -p livewall --test e2e_desktop
    LW_E2E=1 cargo test -p livewall --test e2e_hotkeys
    LW_E2E=1 cargo test -p livewall --test e2e_media
    LW_E2E=1 cargo test -p livewall --test e2e_ipc

`LW_E2E=1` enables the end-to-end suites. They need a logged-in GUI session and
manipulate the real desktop picture (they restore it on exit). `e2e_media`
additionally needs `scripts/fixtures.sh` to have run.

## lwctl

`lwctl [--json] <command>` talks to a running LiveWall over a Unix socket.
Exit codes: 0 ok, 1 error, 2 app not running, 64 usage.

    pause  resume  toggle-pause  mute  unmute  next  previous  random
    status  current  quit  displays  spaces  windows  desktop  rules
    library {list,add,remove,rename,move}
    folder {create,rename,delete}
    apply [--item ID] [--target default|display|space] [--target-id ID]
    set-geometry ID --mode fill|fit|stretch|center|custom [--x N --y N --zoom N]
    set-item ID [--volume N --speed N --muted BOOL --still-time N]
    settings {get,set JSON}
    hotkeys {get,set ACTION [COMBO]}
    login-item {get,set BOOL}
    menu  tools  tools-install TOOL [--consent]
    probe PATH
    download URL [--consent] [--no-wait]  download-status ID  download-cancel ID  downloads
    optimize ID  optimize-status ID  optimize-cancel ID

## Transitions

Every wallpaper change animates on the GPU (awww-style). `apply`, `next`,
`previous` and `random` accept:

    -t, --transition KIND   none simple fade left right top bottom wipe wave
                            grow center any outer random        (default fade)
    --duration SECONDS      default 1.0
    --bezier A,B,C,D        default 0.54,0,0.34,0.99
    --angle DEG             wipe/wave direction; 0 = right-to-left, 90 = top-to-bottom; default 45
    --pos X,Y | ALIAS       circle origin: fractions, pixels or center/top-right/...
    --invert-y              flip the y axis of --pos
    --wave PERIOD,AMPLITUDE default 20,20

    lwctl next -t wipe --angle 90 --duration 0.8
    lwctl settings set '{"transition":{"kind":"grow","pos":"bottom-left"}}'

The global default lives in `settings.json` under `transition`; per-request flags
override it. Topology changes, startup and crash recovery switch instantly.

## Data

`~/Library/Application Support/LiveWall/`:

    library.json      items, folders, placement
    settings.json     settings, hotkeys, language, tool consent
    swap-state.json   captured original desktop pictures (for restore/crash recovery)
    Wallpapers/       imported media, as Wallpapers/<uuid>.<ext>
    Thumbs/           <item-id>.jpg thumbnails
    Stills/           <hash>.png still frames
    Incoming/         download staging (wiped on launch)
    Tools/            optional ffmpeg/yt-dlp/deno overrides
    control.sock      IPC socket (mode 0600; parent directory 0700)

Environment overrides: `LIVEWALL_DATA_DIR`, `LIVEWALL_SOCKET`, `LIVEWALL_BREW`.
