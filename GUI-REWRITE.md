# GUI rewrite: working brief

Untracked scratch file for session continuity. Do not commit it. Delete it when the rewrite lands.

## Goal
Rewrite the sloppy GUI code added since baseline `562123b` as native macOS AppKit (Rust + objc2), not patch it.
Backend is trusted. Keep baseline-era logic. Rewrite ~30-40% of the GUI.

## Git
- Baseline: `562123b` (feat(library): folder icons and colors over IPC and lwctl).
- `v2` HEAD `d6e3a68` = old WIP snapshot of the whole previous GUI (amended, local only, nothing pushed). Reference for old code: `git show d6e3a68:crates/app/src/ui/<file>`.
- Working tree on top of it (uncommitted): dev isolation gates, `scripts/dev*.sh`, the new native `ui/` skeleton.
- END OF PROJECT: `git reset --soft 562123b`, then commit. Conventional, subject-only, no body, no co-author. `Archive.zip` stays untracked.

## Hard rules (from the user)
- No comments or docstrings in code. Explicit `return`. Rust 2024 unsafe rules.
- Native only: system colours/controls/materials. NO custom colours, shadows, animations, appearance enhancers. No hard-coded control heights. No NSVisualEffectView in the sidebar.
- Delete the custom colour-style system entirely (style presets, `lwctl style`, proto `Style*`, Appearance editor). Keep folder colours (Finder tag colours/hex): `style/tags.rs`, `style/color.rs`, `FolderStyle`.
- Target macOS 26 natively; keep macOS 15 working only where cheap, guarded by `objc2::available!` / `AnyClass::get`. Unverifiable on this Mac (macOS 27).
- en/ru localization must stay: strings that remain keep their ru entry; every new string needs en key plus ru entry. Language is resolved once at launch from the system language (unchanged). Live in-app switching would be a NEW feature: ask the user first.
- i18n test fails on unused ru keys. At the end: delete code first, run the test, remove exactly the ru keys it reports.
- One cargo at a time (8 GB RAM). Never kill by name; kill by PID.
- Confirm before significant actions; ask before assuming.

## Dev loop (running)
- `scripts/dev.sh [--fresh|--once]`: polls `crates/**/*.rs`, builds (debug) into `target/dev/build.log`, relaunches the isolated dev app on success. A failed build leaves the old window running.
- Isolation env (`scripts/dev-env.sh`): `LIVEWALL_DATA_DIR`, `LIVEWALL_SOCKET`, `LW_UI_NO_DESKTOP=1` (gates in `crates/app/src/dev.rs`: no desktop takeover, no wallpaper windows, no hotkeys, no status item, no login item, no focus steal, separate frame autosave), `LIVEWALL_TEST_UNLOCKED=1`. Dev data is in `target/dev/data`.
- `scripts/dev-shot.sh` -> `target/dev/shot.png` (screencapture -l, works while covered). The first capture right after a relaunch can catch a half-sized window; retake it.
- `scripts/dev-seed.sh`: 12 fixture items, 4 folders. Known bug: the final "lock Private" step fails with "optimize is in progress"; needs a retry loop.
- Stop the watcher: find the PID with `ps -o ppid= -p $(cat target/dev/app.pid)`, then `kill` it. Its trap kills the app.
- When the watcher runs, agents must NOT run cargo; they save, wait for `target/dev/build.log` to update, and read it.
- Existing env hooks: `LW_UI_SCOPE`, `LW_UI_FOLDER`, `LW_UI_SEARCH`, `LW_UI_SELECT`, `LW_UI_SETTINGS` (tab index), `LW_UI_SIZE`, `LW_UI_LAYOUT` (large|small|list), `LW_UI_LANG` (ru|en, dev.sh passes `-AppleLanguages`; default follows the system).

## Working with agents
- Executor agents are good at running commands and following precise specs, weak at design. Give exact specs (files, APIs, what to delete, done-when). Design decisions stay with the main agent and the user.
- Disjoint files per agent. The main agent reviews diffs and screenshots.

## Native geometry spec (Apple docs, verified)
- Window: Titled|Closable|Miniaturizable|Resizable|FullSizeContentView, toolbarStyle Unified, title hidden.
- `NSSplitViewController`: sidebar item (system width, no thickness overrides), content item (defaults), inspector item (system default 270, fixed, collapsible).
- Sidebar: `NSTableView` `.sourceList`, system row size (never `rowSizeStyle = .custom`), group rows, system selection.
- Library grid: `NSCollectionView` + flow layout (decided). List: `NSTableView` `.inset`, swapped with the grid.
- Inspector: `NSScrollView` + `NSStackView`/`NSGridView`, Auto Layout, system control sizes; macOS 26 `prefersCompactControlSizeMetrics` guarded.
- Toolbar: toggleSidebar, sidebarTrackingSeparator, scope, flexible space, sort, layout group, search, add, settings, inspectorTrackingSeparator, toggleInspector. Every item needs a menu equivalent.

## Status
DONE
- Dev isolation gates and the dev loop scripts.
- Native skeleton: new `ui/{mod,windows,panes,toolbar,menus,dialogs}.rs`. Old files deleted: swatches, controls, theme, metrics, widgets, picker, sidebar, wall, inspector, settings, folders, actions.
- `ui/view.rs` (Library model) and `ui/state.rs` kept, currently unused.
- Public `ui::*` API is stubbed as no-ops, so the library, folders and inspector do not work yet. Settings is an empty window.
- Toolbar rebuilt with the baseline button set, title hidden. Pills and "Nothing playing" are removed for now.

NEXT (steer one screen at a time, screenshot after each)
1. Sidebar: source list with All Wallpapers, Folders, counts, icons/colours, inline rename, drop targets, new-folder and lock. Reference: `git show d6e3a68:crates/app/src/ui/sidebar.rs` and `folders.rs`, only for behaviour. Needs `ui::changed()` and a real `Ui` state with Library and Filter.
2. Grid: `NSCollectionView` over `view::Library`.
3. List: `NSTableView` `.inset`.
4. Inspector (reference: `git show 562123b:crates/app/src/ui/inspector.rs`).
5. Scope/sort/kind/layout/search/add actions wired, menus.
6. Settings window: `NSTabViewController` toolbar tabs with native rows. Port general, playback, shortcuts, privacy pages from `d6e3a68:crates/app/src/ui/settings.rs`.
7. Folder icon/colour popover (`NSPopover`, tag-colour grid + `NSColorWell`, keep `Request::FolderStyle`).
8. Backend style removal: proto `Style*`, ctl `StyleCommand`, `app/appearance.rs`, `Appearance.{style,theme,accent,canvas}`, `styles.json`, `style/{presets,document,mod}.rs`, `e2e_ipc` `style_round_trip`, README/README.ru/CHANGELOG. Keep `Tag`, `Rgba`; move `SystemColor` out of `keys.rs`.
9. i18n cleanup (see rules), then `cargo fmt`, clippy `-D warnings`, `cargo test --workspace --lib`.
10. Squash: `git reset --soft 562123b` and one commit.

DEFERRED / OPEN
- "Now Showing" Desktop/Lock pills and playback status: user wants to see the toolbar without them first. Then decide: inspector plus menu bar only, or a toolbar menu.
- Relaunch UI-state file (selection/scope/folder/search/panes) for the dev loop: build after the new `Ui` struct exists.
- Fix the seed lock step.
- Scope popup needs real entries from library desktops.
- Backend determinism: user wants the backend testable and rock solid; review once the GUI settles.
- Possible new feature: live language switching (ask).

## Product decisions (user)
- One display only. Customise per workspace (Space / "desktop"), never per display. GUI must not mention displays.
- Backend still has per-screen code (`engine/display.rs`, `NSScreen`, per-screen surfaces). Dropping it is a backend change: ask first.
- Toolbar is hardcoded on purpose for review (previews, subtitle): grep `HARDCODED PLACEHOLDER`.

## Findings worth remembering
- Baseline already had a light theming (`palette.rs`); HEAD's heavy system replaced it.
- Dead menu handlers at old HEAD: `toggleSidebar:` / `toggleInspector:` (now native via `NSSplitViewController`).
- Old bugs: marker glyph regressed from `lock.fill` to `lock`; deprecated `setAllowedFileTypes` in dialogs.
- `define_class!` method bodies cannot use `return` (tail expressions required).
- Sidebar tracking separator and inspector tracking separator keep toolbar items aligned to pane edges.
