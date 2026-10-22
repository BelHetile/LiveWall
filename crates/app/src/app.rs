use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSApplication;
use objc2_foundation::NSTimer;
use objc2_local_authentication::LAContext;
use proto::{ApplyTarget, ErrorCode, FillMode, HotkeyAction, KeyCombo, Request, Response};
use serde::Serialize;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::download;
use crate::engine::{Engine, Playback};
use crate::hotkey;
use crate::i18n::{self, Strings};
use crate::import;
use crate::media::probe;
use crate::media::{optimize, plan, thumb};
use crate::menu::{Menu, MenuAction, MenuState};
use crate::model::{FolderId, Item, ItemId, ItemKind, Library, LockAssignment, Settings, apply_patch};
use crate::paths::{DirKind, Paths, Visibility};
use crate::platform::hotkeys::{Hotkeys, RegisterError};
use crate::platform::observe::Observers;
use crate::platform::spaces::{self, SurfaceKey};
use crate::platform::system::{self, LoginItemStatus};
use crate::platform::{auth, cancel_timer, detect, schedule_timer};
use crate::process;
use crate::rules;
use crate::store;
use crate::tools;

mod privacy;

pub use privacy::{visible_folders, visible_items};

const GEOMETRY_POLL_SECONDS: f64 = 2.0;
const ICON_PX: usize = 128;
const VOLUME_STEP: f32 = 0.1;
const IDLE_POLL_SECONDS: f64 = 10.0;
const UNLOCK_REASON: &str = "Unlock private wallpaper folders";
const LOCKED: &str = "locked";
const INVALID_ITEM_ID: &str = "invalid item id";
const INVALID_FOLDER_ID: &str = "invalid folder id";
const NO_SUCH_ITEM: &str = "no such item";
const NO_SUCH_FOLDER: &str = "no such folder";
const NO_OPTIMIZE_JOB: &str = "no optimize job for this item";
const SERIALIZATION_FAILED: &str = "serialization failed";

pub enum Event {
    Request(Request, oneshot::Sender<Response>),
    Quit,
    DisplayChanged,
    SpacesChanged,
    FrontmostChanged,
    DisplayAsleep(bool),
    LowPowerChanged,
    ThermalChanged,
    ScreensaverChanged(bool),
    SystemWake,
    OcclusionChanged,
    OcclusionSettled,
    WakeCheck { item: ItemId },
    GeometryTick,
    IdleTick,
    RotateTick,
    UnlockResult { success: bool, epoch: u64 },
    VideoLoaded { item: ItemId },
    GifTick { item: ItemId },
    StillReady { item: ItemId, path: PathBuf },
    StillFailed { path: String },
    DesktopVerify,
    ScreenLocked,
    ScreenUnlocked,
    VideoUnloadTick { item: ItemId },
    CoverTick { item: ItemId },
    TransitionDone { key: SurfaceKey },
    Hotkey(HotkeyAction),
    ItemHotkey(u32),
    Menu(MenuAction),
    ImportDone { report: import::Report, folder: Option<FolderId>, reply: oneshot::Sender<Response> },
    OptimizeDone { item: ItemId, result: Result<optimize::Optimized, String> },
    RotateDone { item: ItemId, result: Result<(String, probe::Probe), String>, reply: oneshot::Sender<Response> },
    DownloadImported { job: String, items: Vec<Item> },
    DownloadResolved,
    DownloadFailed { reason: String },
    Reply(Request, Response),
}

#[allow(clippy::struct_excessive_bools)]
pub struct App {
    mtm: MainThreadMarker,
    paths: Paths,
    library: Library,
    settings: Settings,
    paused: bool,
    display_asleep: bool,
    screen_locked: bool,
    screensaver: bool,
    low_power: bool,
    thermal_hot: bool,
    snapshot: rules::Snapshot,
    verdict: rules::Verdict,
    geometry_timer: Option<Retained<NSTimer>>,
    unlocked: bool,
    unlock_pending: Option<Retained<LAContext>>,
    unlock_epoch: u64,
    idle_timer: Option<Retained<NSTimer>>,
    rotate_timer: Option<Retained<NSTimer>>,
    engine: Engine,
    nap: system::AppNap,
    _observers: Observers,
    hotkeys: Hotkeys,
    hotkey_failures: std::collections::BTreeMap<HotkeyAction, String>,
    strings: Strings,
    menu: Menu,
    handle: tokio::runtime::Handle,
    optimize: Arc<optimize::Queue>,
    downloads: Arc<download::Queue>,
    preparing_downloads: usize,
    shown_status: Option<WallpaperStatus>,
    previewing: Option<ItemId>,
    rotating: std::collections::HashSet<ItemId>,
    item_hotkey_failures: std::collections::BTreeMap<ItemId, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Still,
    Playing,
    Paused,
    Unloaded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    On,
    AllMuted,
    ItemMuted,
    Preview { here: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WallpaperStatus {
    pub state: PlayState,
    pub reason: Option<PauseReason>,
    pub sound: Option<Sound>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PauseReason {
    key: &'static str,
    brief: &'static str,
    arg: Option<String>,
}

impl PauseReason {
    fn new(key: &'static str, brief: &'static str, arg: Option<String>) -> Self {
        return Self { key, brief, arg };
    }

    fn from_cause(cause: &rules::Cause) -> Self {
        let (key, brief) = match cause {
            rules::Cause::DisplayAsleep => ("Display asleep", "display asleep"),
            rules::Cause::ScreenLocked => ("Screen locked", "screen locked"),
            rules::Cause::Screensaver => ("Screen saver is running", "screen saver"),
            rules::Cause::Occluded => ("Desktop is hidden", "desktop hidden"),
            rules::Cause::Thermal => ("Mac is running hot", "Mac is hot"),
            rules::Cause::LowPower => ("Low Power Mode is on", "Low Power Mode"),
            rules::Cause::Fullscreen => ("An app is in fullscreen", "full screen"),
            rules::Cause::Coverage => ("Desktop covered by windows", "covered"),
            rules::Cause::Focus => ("Another app is focused", "app focused"),
            rules::Cause::App(bundle) => return Self::new("App rule: %@", "", Some(app_name(bundle))),
        };
        return Self::new(key, brief, None);
    }

    pub fn brief(&self) -> String {
        return self.arg.clone().unwrap_or_else(|| self.brief.to_string());
    }

    fn args(&self) -> Vec<i18n::Arg> {
        return self.arg.iter().cloned().map(i18n::Arg::Text).collect();
    }

    pub fn localized(&self) -> String {
        return i18n::trf(self.key, &self.args());
    }

    pub fn english(&self) -> String {
        return i18n::substitute(self.key, &self.args());
    }

    pub fn text(&self, strings: &Strings) -> String {
        return strings.format("Paused: %@", &[i18n::Arg::Text(strings.format(self.key, &self.args()))]);
    }
}

fn discard_media(paths: &Paths, item: &Item) {
    let file = std::path::Path::new(&item.path);
    let managed = [Visibility::Public, Visibility::Private]
        .into_iter()
        .any(|visibility| file.parent() == Some(paths.dir_for(DirKind::Wallpapers, visibility).as_path()));
    if managed {
        remove_quietly(file);
    }
    for visibility in [Visibility::Public, Visibility::Private] {
        remove_quietly(&thumb::file(&paths.dir_for(DirKind::Thumbs, visibility), item.id));
    }
}

fn remove_quietly(path: &std::path::Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => tracing::warn!(%error, path = %path.display(), "cannot delete removed wallpaper file"),
    }
}

fn app_name(bundle: &str) -> String {
    return objc2_app_kit::NSWorkspace::sharedWorkspace()
        .runningApplications()
        .to_vec()
        .into_iter()
        .find(|application| application.bundleIdentifier().is_some_and(|id| id.to_string() == bundle))
        .and_then(|application| application.localizedName())
        .map_or_else(|| bundle.to_string(), |name| name.to_string());
}

impl App {
    pub fn new(paths: &Paths, mtm: MainThreadMarker) -> (Self, Vec<store::Warning>) {
        let (library, library_warning) = store::load::<Library>(&paths.library_file());
        let (mut settings, settings_warning) = store::load::<Settings>(&paths.settings_file());
        settings.normalize();
        settings.launch_at_login = matches!(system::login_item_status(), LoginItemStatus::Enabled);
        let warnings = library_warning.into_iter().chain(settings_warning).collect();

        let language = i18n::select_language(settings.language, &system::preferred_languages());
        i18n::set_active(language);
        let strings = Strings::new(language);
        let (hotkeys, hotkey_failures) = register_all(&settings.hotkeys);
        let unlocked = test_unlocked();
        let menu = Menu::install(
            mtm,
            &MenuState {
                title: current_title(&library, unlocked, &settings.privacy),
                paused: false,
                status: None,
                muted: settings.muted,
                launch_at_login: settings.launch_at_login,
            },
            &strings,
        );

        let handle = tokio::runtime::Handle::current();
        let optimize = Arc::new(optimize::Queue::new(&handle));
        let downloads = Arc::new(download::Queue::new(handle.clone()));
        let mut app = Self {
            mtm,
            paused: false,
            display_asleep: false,
            screen_locked: false,
            screensaver: false,
            low_power: system::low_power_mode(),
            thermal_hot: system::thermal_hot(),
            snapshot: rules::Snapshot::default(),
            verdict: rules::Verdict::default(),
            geometry_timer: None,
            unlocked,
            unlock_pending: None,
            unlock_epoch: 0,
            idle_timer: None,
            rotate_timer: None,
            library,
            settings,
            engine: Engine::new(mtm, paths),
            nap: system::AppNap::new(),
            _observers: Observers::install(mtm),
            hotkeys,
            hotkey_failures,
            strings,
            menu,
            handle,
            optimize,
            downloads,
            preparing_downloads: 0,
            shown_status: None,
            previewing: None,
            rotating: std::collections::HashSet::new(),
            item_hotkey_failures: std::collections::BTreeMap::new(),
            paths: paths.clone(),
        };
        app.register_item_hotkeys();
        app.sync(false);
        app.refresh_rules();
        app.sync_idle_timer();
        app.restart_rotation();
        return (app, warnings);
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Quit => self.shutdown(),
            Event::Request(request, reply) => self.request(request, reply),
            Event::Hotkey(action) => self.run_hotkey(action),
            Event::ItemHotkey(id) => {
                if let Some(item) = self.hotkeys.item_for(id) {
                    self.toggle_item(item);
                }
            }
            Event::Menu(action) => self.run_menu(action),
            Event::ImportDone { report, folder, reply } => {
                let response = self.finish_import(&report, folder);
                let _ = reply.send(response);
            }
            Event::OptimizeDone { item, result } => self.finish_optimize(item, result),
            Event::RotateDone { item, result, reply } => {
                let response = self.finish_rotate(item, result);
                let _ = reply.send(response);
            }
            Event::DownloadImported { job, items } => self.finish_download_import(&job, items),
            Event::DownloadResolved => {
                self.preparing_downloads = self.preparing_downloads.saturating_sub(1);
                crate::ui::changed();
            }
            Event::DownloadFailed { reason } => crate::ui::download_failed(&reason),
            Event::Reply(request, response) => crate::ui::reply(&request, &response),
            Event::UnlockResult { success, epoch } => self.finish_unlock(success, epoch),
            Event::IdleTick => self.idle_tick(),
            Event::RotateTick => self.rotate_tick(),
            Event::VideoLoaded { item } => {
                let playback = self.playback();
                self.engine.on_video_loaded(item, &self.library, &playback);
                self.refresh_power();
            }
            Event::GifTick { item } => self.engine.on_gif_tick(item),
            Event::StillReady { item, path } => {
                self.engine.on_still_ready(item, &path.to_string_lossy(), &self.library);
                self.publish_status();
                crate::ui::changed();
            }
            Event::StillFailed { path } => self.engine.on_still_failed(&path),
            Event::DesktopVerify => self.engine.on_desktop_verify(),
            Event::CoverTick { item } => self.engine.check_cover(item),
            Event::VideoUnloadTick { item } => {
                let playback = self.playback();
                self.engine.on_unload_tick(item, &self.library, &playback);
                self.publish_status();
            }
            Event::TransitionDone { key } => {
                self.engine.on_transition_done(&key);
                let playback = self.playback();
                self.engine.refresh_playback(&self.library, &playback);
                self.refresh_power();
            }
            Event::WakeCheck { item } => self.engine.on_wake_check(item),
            Event::DisplayChanged => self.on_display_changed(),
            Event::SpacesChanged => {
                self.sync(true);
                self.refresh_rules();
            }
            Event::DisplayAsleep(asleep) => self.on_display_asleep(asleep),
            Event::LowPowerChanged => {
                self.low_power = system::low_power_mode();
                self.refresh_rules();
            }
            Event::ThermalChanged => {
                self.thermal_hot = system::thermal_hot();
                self.refresh_rules();
            }
            Event::ScreensaverChanged(active) => self.on_screensaver(active),
            Event::OcclusionChanged => {
                if self.engine.on_occlusion_changed() {
                    self.refresh_rules();
                }
            }
            Event::OcclusionSettled => {
                if self.engine.on_occlusion_settled() {
                    self.refresh_rules();
                }
            }
            Event::ScreenLocked => self.on_screen_locked(),
            Event::ScreenUnlocked => self.on_screen_unlocked(),
            Event::SystemWake => {
                self.refresh_rules();
                self.engine.on_wake();
            }
            Event::FrontmostChanged | Event::GeometryTick => self.refresh_rules(),
        }
        self.sync_menu();
    }

    fn on_display_changed(&mut self) {
        if self.settings.privacy.lock_on_display && system::online_display_count() > 1 {
            self.lock();
        }
        self.sync(false);
        self.refresh_rules();
    }

    fn on_display_asleep(&mut self, asleep: bool) {
        self.display_asleep = asleep;
        if asleep && self.settings.privacy.lock_on_screen_lock {
            self.lock();
        }
        self.refresh_rules();
        if !asleep {
            self.engine.on_wake();
        }
    }

    fn on_screensaver(&mut self, active: bool) {
        self.screensaver = active;
        if active && self.settings.privacy.lock_on_screen_lock {
            self.lock();
        }
        self.refresh_rules();
    }

    fn on_screen_locked(&mut self) {
        self.screen_locked = true;
        if self.settings.privacy.lock_on_screen_lock {
            self.lock();
        }
        self.engine.on_lock();
        self.refresh_rules();
    }

    fn on_screen_unlocked(&mut self) {
        self.screen_locked = false;
        self.engine.on_unlock(&self.library);
        self.refresh_rules();
        self.engine.on_wake();
    }

    fn request(&mut self, request: Request, reply: oneshot::Sender<Response>) {
        match request {
            Request::LibraryAdd { paths, folder } => self.start_import(paths, folder.as_deref(), reply),
            Request::Probe { path } => self.start_probe(path, reply),
            Request::Tools => self.start_tools(reply),
            Request::ToolsInstall { tool, consent } => self.start_tools_install(&tool, consent, reply),
            Request::Download { url, consent } => self.start_download(url, consent, reply),
            Request::Rotate { id, turns } => self.start_rotate(&id, turns, reply),
            other => {
                let response = self.dispatch(&other);
                let _ = reply.send(response);
            }
        }
    }

    fn reply_with(&self, reply: oneshot::Sender<Response>, work: impl Future<Output = Response> + Send + 'static) {
        self.handle.spawn(async move {
            let _ = reply.send(work.await);
        });
    }

    pub fn submit(&mut self, request: Request) {
        let (reply, receiver) = oneshot::channel();
        let echo = request.clone();
        self.request(request, reply);
        self.handle.spawn(async move {
            if let Ok(response) = receiver.await {
                post(Event::Reply(echo, response));
            }
        });
    }

    fn start_import(&mut self, paths: Vec<String>, folder: Option<&str>, reply: oneshot::Sender<Response>) {
        let folder = match folder.map(parse_folder).transpose() {
            Ok(folder) => folder,
            Err(response) => {
                let _ = reply.send(response);
                return;
            }
        };
        if folder.is_some_and(|id| !self.library.has_folder(id)) {
            let _ = reply.send(Response::err(ErrorCode::NotFound, NO_SUCH_FOLDER));
            return;
        }
        if !self.unlocked && self.library.is_locked_folder(folder) {
            let _ = reply.send(Response::err(ErrorCode::Locked, LOCKED));
            return;
        }
        let dest = self.paths.wallpapers_dir();
        let thumbs = self.paths.thumbs_dir();
        let policy = self.probe_policy();
        self.handle.spawn(async move {
            let report = import::run(&paths, &dest, &thumbs, policy, import::Transfer::Copy).await;
            post(Event::ImportDone { report, folder, reply });
        });
    }

    fn finish_import(&mut self, report: &import::Report, folder: Option<FolderId>) -> Response {
        let added = report.imported.len();
        let items: Vec<Item> = report.imported.iter().map(|imported| imported.item.clone()).collect();
        let ids: Vec<ItemId> = items.iter().map(|item| item.id).collect();
        self.add_items(items);
        if let Some(folder) = folder.filter(|id| self.library.has_folder(*id))
            && !ids.is_empty()
        {
            let _ = self.move_to_folder(&ids, Some(folder));
        }
        return ok(&AddReport { added, skipped: report.skipped, results: &report.results });
    }

    fn add_items(&mut self, mut items: Vec<Item>) {
        let av1: Vec<String> = if probe::av1_hardware() {
            Vec::new()
        } else {
            items.iter().filter(|item| item.codec.as_deref() == Some("av01")).map(|item| item.title.clone()).collect()
        };
        if !av1.is_empty() {
            crate::ui::av1_notice(&av1, self.settings.optimize_on_import);
        }
        let mut queue = Vec::new();
        for item in &mut items {
            item.volume = self.settings.volume;
            item.speed = self.settings.speed;
            if item.needs_optimize && self.settings.optimize_on_import {
                queue.push(item.id);
            }
        }
        self.library.items.extend(items);
        self.persist_library();
        self.reapply();
        for item in queue {
            let _ = self.on_optimize_requested(item);
        }
    }

    fn finish_download_import(&mut self, job: &str, items: Vec<Item>) {
        let done =
            items.first().map(|item| download::JobState::Done { item: item.id.to_string(), path: item.path.clone() });
        self.add_items(items);
        if let Some(state) = done {
            self.downloads.complete(job, state);
        }
    }

    fn start_download(&mut self, url: String, consent: bool, reply: oneshot::Sender<Response>) {
        let queue = Arc::clone(&self.downloads);
        let paths = self.paths.clone();
        let settings_consent = self.settings.tools.install_consent;
        let policy = self.probe_policy();
        let height = self.max_display_px();
        let av1_hw = probe::av1_hardware();
        self.preparing_downloads += 1;
        crate::ui::changed();
        self.reply_with(reply, async move {
            let resolved =
                resolve_download(&url, consent || settings_consent, &paths, policy, height, av1_hw, &queue).await;
            post(Event::DownloadResolved);
            return match resolved {
                Ok(id) => ok(&serde_json::json!({ "id": id, "state": "queued" })),
                Err((code, message)) => Response::err(code, message),
            };
        });
    }

    fn finish_optimize(&mut self, item: ItemId, result: Result<optimize::Optimized, String>) {
        match result {
            Ok(optimized) => {
                if let Some(entry) = self.library.items.iter_mut().find(|entry| entry.id == item) {
                    entry.path = optimized.path;
                    entry.kind = optimized.probe.kind;
                    entry.width = optimized.probe.width;
                    entry.height = optimized.probe.height;
                    entry.fps = optimized.probe.fps;
                    entry.codec = optimized.probe.codec;
                    entry.audio_codec = optimized.probe.audio_codec;
                    entry.hdr = optimized.probe.hdr;
                    entry.duration = optimized.probe.duration;
                    entry.playable = true;
                    entry.needs_optimize = false;
                    entry.optimize_reason = None;
                    entry.optimized = true;
                }
                self.engine.forget(item);
                tracing::info!(%item, "optimize complete");
            }
            Err(reason) => {
                let playable = self.library.items.iter().any(|entry| entry.id == item && entry.playable);
                if playable {
                    tracing::warn!(%item, %reason, "optimize failed; keeping original");
                } else {
                    tracing::warn!(%item, %reason, "optimize failed; removing unplayable item");
                    let _ = self.remove_items(&[item.to_string()]);
                }
            }
        }
        self.persist_library();
        self.reapply();
    }

    fn start_rotate(&mut self, id: &str, turns: u8, reply: oneshot::Sender<Response>) {
        let request = Request::Rotate { id: id.to_string(), turns };
        if let Some(denied) = self.deny(&request) {
            let _ = reply.send(denied);
            return;
        }
        let item = match parse_item(id) {
            Ok(item) => item,
            Err(response) => {
                let _ = reply.send(response);
                return;
            }
        };
        let Some(entry) = self.library.items.iter().find(|entry| entry.id == item) else {
            let _ = reply.send(Response::err(ErrorCode::NotFound, NO_SUCH_ITEM));
            return;
        };
        if !(1..=3).contains(&turns) {
            let _ = reply.send(Response::err(ErrorCode::Invalid, "turns must be 1, 2 or 3"));
            return;
        }
        if entry.kind == ItemKind::Gif {
            let _ = reply.send(Response::err(ErrorCode::Media, "GIFs can’t be rotated"));
            return;
        }
        if self.optimizing_item(item) || !self.rotating.insert(item) {
            let _ = reply.send(Response::err(ErrorCode::Busy, "this wallpaper is being converted"));
            return;
        }
        let (input, kind) = (PathBuf::from(&entry.path), entry.kind);
        self.handle.spawn(async move {
            let task = tokio::task::spawn_blocking(move || {
                let target = crate::media::orient::rotate_file(&input, kind, turns)?;
                let probed = probe::inspect(&target, kind);
                return Ok((target.to_string_lossy().into_owned(), probed));
            });
            let result = task.await.unwrap_or_else(|_| Err("rotation failed".to_string()));
            post(Event::RotateDone { item, result, reply });
        });
    }

    fn finish_rotate(&mut self, item: ItemId, result: Result<(String, probe::Probe), String>) -> Response {
        self.rotating.remove(&item);
        let (path, probed) = match result {
            Ok(done) => done,
            Err(reason) => {
                tracing::warn!(%item, %reason, "rotate failed");
                return Response::err(ErrorCode::Media, reason);
            }
        };
        let thumbs = self.library.items.iter().find(|entry| entry.id == item).map(|entry| {
            return self.paths.dir_for(DirKind::Thumbs, self.library.visibility(entry));
        });
        let Some(entry) = self.library.items.iter_mut().find(|entry| entry.id == item) else {
            return Response::err(ErrorCode::NotFound, NO_SUCH_ITEM);
        };
        entry.path.clone_from(&path);
        entry.width = probed.width;
        entry.height = probed.height;
        if let Some(thumbs) = thumbs {
            remove_quietly(&thumb::file(&thumbs, item));
        }
        let snapshot = entry.clone();
        self.engine.forget(item);
        self.persist_library();
        self.reapply();
        self.ensure_thumbnail(&snapshot);
        return ok(&serde_json::json!({ "item": item.to_string(), "path": path }));
    }

    fn optimizing_item(&self, item: ItemId) -> bool {
        return matches!(
            self.optimize.status(item),
            Some(optimize::JobState::Queued | optimize::JobState::Running { .. })
        );
    }

    pub fn is_rotating(&self, item: ItemId) -> bool {
        return self.rotating.contains(&item);
    }

    fn start_probe(&mut self, path: String, reply: oneshot::Sender<Response>) {
        let Some(kind) =
            std::path::Path::new(&path).extension().and_then(|value| value.to_str()).and_then(import::kind_for)
        else {
            let _ = reply.send(Response::err(ErrorCode::Media, "unsupported format"));
            return;
        };
        let policy = self.probe_policy();
        self.reply_with(reply, async move {
            let task = tokio::task::spawn_blocking(move || {
                let found = probe::inspect(std::path::Path::new(&path), kind);
                let verdict = probe::verdict(&found, policy);
                return (found, verdict);
            });
            return match task.await {
                Ok((found, verdict)) => ok(&serde_json::json!({ "probe": found, "verdict": verdict })),
                Err(_) => Response::err(ErrorCode::Internal, "probe failed"),
            };
        });
    }

    fn start_tools(&mut self, reply: oneshot::Sender<Response>) {
        let tools_dir = self.paths.tools_dir();
        self.reply_with(reply, async move {
            let report = tools::report(&tools_dir).await;
            return ok(&report);
        });
    }

    fn start_tools_install(&mut self, tool: &str, consent: bool, reply: oneshot::Sender<Response>) {
        let Some(kind) = tools::ToolKind::from_id(tool) else {
            let _ = reply.send(Response::err(ErrorCode::Invalid, "unknown tool"));
            return;
        };
        if !consent && !self.settings.tools.install_consent {
            let _ = reply.send(Response::err(ErrorCode::ConsentRequired, "tool installation requires consent"));
            return;
        }
        let tools_dir = self.paths.tools_dir();
        self.reply_with(reply, async move {
            return match tools::install(kind, &tools_dir).await {
                Ok(path) => ok(&serde_json::json!({ "tool": kind, "path": path.to_string_lossy() })),
                Err(message) => Response::err(ErrorCode::ToolMissing, message),
            };
        });
    }

    fn on_optimize_requested(&self, item: ItemId) -> Result<(), (ErrorCode, String)> {
        let Some(entry) = self.library.items.iter().find(|entry| entry.id == item) else {
            return Err((ErrorCode::NotFound, NO_SUCH_ITEM.to_string()));
        };
        if !entry.needs_optimize {
            return Err((ErrorCode::Media, "item does not need optimization".to_string()));
        }
        let tools_dir = self.paths.tools_dir();
        let container =
            std::path::Path::new(&entry.path).extension().and_then(|value| value.to_str()).map(str::to_ascii_lowercase);
        let route = if entry.kind == ItemKind::Gif {
            plan::Route::Apple
        } else {
            plan::route(container.as_deref(), entry.codec.as_deref(), probe::av1_hardware())
        };
        let ffmpeg = match route {
            plan::Route::Apple => None,
            plan::Route::Ffmpeg => match tools::locate(tools::ToolKind::Ffmpeg, &tools_dir) {
                Some(path) => Some(path),
                None => return Err((ErrorCode::ToolMissing, "ffmpeg is not installed".to_string())),
            },
        };
        let fallback = plan::Source {
            width: entry.width,
            height: entry.height,
            fps: entry.fps,
            bit_depth: None,
            hdr: entry.hdr,
            transfer: None,
            audio: plan::audio_plan(entry.audio_codec.as_deref(), false),
        };
        let job = optimize::Job {
            item,
            kind: entry.kind,
            input: PathBuf::from(&entry.path),
            thumbs_dir: self.paths.dir_for(DirKind::Thumbs, self.library.visibility(entry)),
            ffmpeg,
            fps_limit: self.settings.optimize_fps_limit,
            max_pixels: self.max_display_px(),
            duration: entry.duration,
            fallback,
        };
        if !self.optimize.enqueue(job) {
            return Err((ErrorCode::Busy, "optimize is already queued for this item".to_string()));
        }
        return Ok(());
    }

    fn start_optimize(&self, id: &str) -> Result<Response, Response> {
        let uuid = parse_item(id)?;
        return Ok(match self.on_optimize_requested(uuid) {
            Ok(()) => ok(&serde_json::json!({ "id": id, "state": "queued" })),
            Err((code, message)) => Response::err(code, message),
        });
    }

    fn optimize_status(&self, id: &str) -> Result<Response, Response> {
        let uuid = parse_item(id)?;
        if !self.unlocked && self.library.is_private_id(uuid) {
            return Ok(Response::err(ErrorCode::NotFound, NO_OPTIMIZE_JOB));
        }
        return Ok(match self.optimize.status(uuid) {
            Some(state) => ok(&state.report(uuid)),
            None => Response::err(ErrorCode::NotFound, NO_OPTIMIZE_JOB),
        });
    }

    fn download_status(&self, id: &str) -> Response {
        return match self.downloads.status(id) {
            Some(state) => ok(&state.report(id)),
            None => Response::err(ErrorCode::NotFound, "no such download job"),
        };
    }

    fn probe_policy(&self) -> probe::Policy {
        return probe::Policy { fps_limit: self.settings.optimize_fps_limit, max_pixels: self.max_display_px() };
    }

    fn max_display_px(&self) -> u32 {
        return self.engine.max_display_px();
    }

    pub fn run_hotkey(&mut self, action: HotkeyAction) {
        match action {
            HotkeyAction::Next => {
                let _ = self.select(Selection::Next, None);
            }
            HotkeyAction::Previous => {
                let _ = self.select(Selection::Previous, None);
            }
            HotkeyAction::Random => {
                let _ = self.select(Selection::Random, None);
            }
            HotkeyAction::TogglePause => {
                let paused = self.paused;
                let _ = self.set_paused(!paused);
            }
            HotkeyAction::ToggleMute => {
                let muted = self.settings.muted;
                let _ = self.set_muted(!muted);
            }
            HotkeyAction::VolumeUp => self.nudge_volume(VOLUME_STEP),
            HotkeyAction::VolumeDown => self.nudge_volume(-VOLUME_STEP),
            HotkeyAction::Lock => self.lock(),
        }
    }

    fn nudge_volume(&mut self, delta: f32) {
        let Some(id) = self.showing_item() else { return };
        let Some(entry) = self.library.items.iter().find(|entry| entry.id == id) else { return };
        let base = if entry.muted { 0.0 } else { entry.volume };
        let volume = (base + delta).clamp(0.0, 1.0);
        let _ = self.set_item(&id.to_string(), Some(volume), None, Some(volume <= 0.0), None);
    }

    fn showing_item(&self) -> Option<ItemId> {
        let (spaces, current) = self.desktops();
        let space = spaces.into_iter().nth(current)?;
        return self.library.resolve(Some(&space), self.reveals_desktop());
    }

    fn run_menu(&mut self, action: MenuAction) {
        match action {
            MenuAction::TogglePause => {
                let paused = self.paused;
                let _ = self.set_paused(!paused);
            }
            MenuAction::Next => {
                let _ = self.select(Selection::Next, None);
            }
            MenuAction::Previous => {
                let _ = self.select(Selection::Previous, None);
            }
            MenuAction::Random => {
                let _ = self.select(Selection::Random, None);
            }
            MenuAction::ToggleMute => {
                let muted = self.settings.muted;
                let _ = self.set_muted(!muted);
            }
            MenuAction::ToggleLaunchAtLogin => {
                let enabled = !self.settings.launch_at_login;
                let _ = self.set_login_item(enabled);
            }
            MenuAction::OpenLibrary => crate::ui::reopen(),
            MenuAction::Quit => self.shutdown(),
        }
    }

    fn shutdown(&mut self) {
        self.engine.restore_desktop();
        process::kill_all();
        crate::ui::dismiss_sheets();
        quit(self.mtm);
    }

    fn sync_menu(&self) {
        self.menu.update(
            &MenuState {
                title: current_title(&self.library, self.unlocked, &self.settings.privacy),
                paused: self.paused,
                status: self.pause_reason().map(|reason| reason.text(&self.strings)),
                muted: self.settings.muted,
                launch_at_login: self.settings.launch_at_login,
            },
            &self.strings,
        );
    }

    fn playback(&self) -> Playback {
        return Playback {
            paused: self.paused,
            muted: self.settings.muted,
            stop_after: self.settings.stop_after_seconds,
            verdict: self.verdict.clone(),
            reveal: self.reveals_desktop(),
            silenced: self.previewing,
        };
    }

    pub fn reveals_desktop(&self) -> bool {
        return privacy::reveals_desktop(self.unlocked, &self.settings.privacy);
    }

    pub fn set_previewing(&mut self, item: Option<ItemId>) {
        if self.previewing == item {
            return;
        }
        self.previewing = item;
        let playback = self.playback();
        self.engine.refresh_playback(&self.library, &playback);
        self.publish_status();
    }

    fn detect(&self) -> rules::Snapshot {
        let to_rect = |value: objc2_core_foundation::CGRect| {
            return rules::Rect::new(value.origin.x, value.origin.y, value.size.width, value.size.height);
        };
        let frame = to_rect(self.engine.display().frame);
        let usable = to_rect(self.engine.usable_frame());
        let signals = detect::Signals {
            display_asleep: self.display_asleep,
            screen_locked: self.screen_locked,
            screensaver: self.screensaver,
            occluded: self.engine.occluded(),
            thermal_hot: self.thermal_hot,
            low_power: self.low_power,
        };
        return detect::snapshot(frame, usable, signals);
    }

    fn refresh_rules(&mut self) {
        self.snapshot = self.detect();
        let config = rules::Config::from_settings(&self.settings);
        self.verdict = rules::evaluate(&self.snapshot, &config);
        let playback = self.playback();
        self.engine.refresh_playback(&self.library, &playback);
        self.refresh_power();
        self.publish_status();
    }

    fn refresh_power(&mut self) {
        if self.engine.any_playing() {
            self.nap.begin();
        } else {
            self.nap.end();
        }
        self.update_geometry_timer();
    }

    fn update_geometry_timer(&mut self) {
        let needed = rules::geometry_poll_needed(self.engine.any_playing(), &self.verdict, &self.settings);
        match (needed, self.geometry_timer.is_some()) {
            (true, false) => self.start_geometry_timer(),
            (false, true) => cancel_timer(self.geometry_timer.take()),
            _ => {}
        }
    }

    fn start_geometry_timer(&mut self) {
        self.geometry_timer = Some(schedule_timer(GEOMETRY_POLL_SECONDS, true, || Event::GeometryTick));
    }

    fn sync(&mut self, force_desktop: bool) {
        let playback = self.playback();
        self.engine.sync(&self.library, &playback);
        self.engine.refresh_desktop(&self.library, force_desktop);
        crate::ui::changed();
    }

    fn reapply(&mut self) {
        self.reapply_with(None);
    }

    fn reapply_with(&mut self, request: Option<&proto::Transition>) {
        let playback = self.playback();
        let transition = crate::engine::transition::resolve(&self.settings.transition, request);
        self.engine.reapply(&self.library, &playback, Some(&transition));
        self.engine.refresh_desktop(&self.library, false);
        self.refresh_power();
        self.publish_status();
    }

    fn persist_library(&self) {
        if let Err(error) = store::save(&self.paths.library_file(), &self.library) {
            tracing::warn!(%error, "cannot save library");
        }
        crate::ui::changed();
    }

    fn persist_settings(&self) {
        if let Err(error) = store::save(&self.paths.settings_file(), &self.settings) {
            tracing::warn!(%error, "cannot save settings");
        }
        crate::ui::changed();
    }

    pub fn library(&self) -> &Library {
        return &self.library;
    }

    pub fn settings(&self) -> &Settings {
        return &self.settings;
    }

    pub fn paths(&self) -> &Paths {
        return &self.paths;
    }

    pub fn desktops(&self) -> (Vec<String>, usize) {
        let topology = self.engine.spaces();
        let spaces: Vec<String> = spaces::plan(topology).into_iter().map(|spec| spec.key.space).collect();
        let current = spaces.iter().position(|space| *space == topology.current).unwrap_or(0);
        return (spaces, current);
    }

    pub fn optimize_progress(&self, item: ItemId) -> Option<f64> {
        return match self.optimize.status(item)? {
            optimize::JobState::Queued => Some(0.0),
            optimize::JobState::Running { percent, .. } => Some(percent),
            _ => None,
        };
    }

    pub fn preparing_downloads(&self) -> usize {
        return self.preparing_downloads;
    }

    pub fn download_progress(&self) -> Vec<download::JobReport> {
        return self
            .downloads
            .reports()
            .into_iter()
            .filter(|report| {
                matches!(
                    report.state,
                    download::Phase::Queued
                        | download::Phase::Downloading
                        | download::Phase::Merging
                        | download::Phase::Importing
                )
            })
            .collect();
    }

    pub fn ensure_thumbnail(&self, item: &Item) {
        let (id, kind, path, duration) = (item.id, item.kind, PathBuf::from(&item.path), item.duration);
        let thumbs = self.paths.dir_for(DirKind::Thumbs, self.library.visibility(item));
        self.handle.spawn(async move {
            if crate::media::thumb::ensure(id, kind, path, duration, thumbs).await.is_some() {
                crate::ui::changed();
            }
        });
    }

    fn current_item(&self) -> Option<ItemId> {
        return self.library.resolve(None, self.unlocked);
    }

    fn state(&self) -> StateReport {
        return StateReport { paused: self.paused, muted: self.settings.muted };
    }

    fn set_paused(&mut self, paused: bool) -> Response {
        self.paused = paused;
        self.reapply();
        return ok(&self.state());
    }

    fn set_muted(&mut self, muted: bool) -> Response {
        self.settings.muted = muted;
        self.persist_settings();
        self.reapply();
        return ok(&self.state());
    }

    pub fn apply(
        &mut self,
        item: Option<&str>,
        target: &ApplyTarget,
        everywhere: bool,
        transition: Option<&proto::Transition>,
    ) -> Response {
        let item = match item.map(parse_item).transpose() {
            Ok(item) => item,
            Err(response) => return response,
        };
        if let Some(id) = item
            && let Some(entry) = self.library.items.iter().find(|entry| entry.id == id)
            && !entry.playable
        {
            let reason =
                entry.optimize_reason.clone().unwrap_or_else(|| "media is not playable until optimized".to_string());
            return Response::err(ErrorCode::Media, reason);
        }
        self.apply_target(item, target);
        if everywhere {
            for assignment in self.library.placement.spaces.values_mut() {
                assignment.item = None;
            }
        }
        self.persist_library();
        self.reapply_with(transition);
        self.restart_rotation();
        return ok(&serde_json::json!({ "applied": item.map(|id| id.to_string()) }));
    }

    pub fn is_unlocked(&self) -> bool {
        return self.unlocked;
    }

    pub fn lock(&mut self) {
        self.unlock_epoch += 1;
        if let Some(context) = self.unlock_pending.take() {
            auth::cancel(&context);
        }
        if !self.unlocked {
            return;
        }
        self.unlocked = false;
        self.refresh_visibility();
        self.sync_idle_timer();
        self.sync_menu();
        crate::ui::changed();
    }

    pub fn request_unlock(&mut self) {
        if self.unlocked || self.unlock_pending.is_some() {
            return;
        }
        self.unlock_epoch += 1;
        let reason = self.strings.get(UNLOCK_REASON);
        self.unlock_pending = Some(auth::evaluate(&reason, self.unlock_epoch));
    }

    fn finish_unlock(&mut self, success: bool, epoch: u64) {
        if epoch != self.unlock_epoch || self.unlock_pending.take().is_none() || !success {
            return;
        }
        self.unlocked = true;
        self.reapply();
        self.sync_idle_timer();
        self.sync_menu();
        crate::ui::changed();
    }

    fn sync_idle_timer(&mut self) {
        let needed = self.unlocked && self.settings.privacy.lock_on_idle;
        match (needed, self.idle_timer.is_some()) {
            (true, false) => self.idle_timer = Some(schedule_timer(IDLE_POLL_SECONDS, true, || Event::IdleTick)),
            (false, true) => cancel_timer(self.idle_timer.take()),
            _ => {}
        }
    }

    fn idle_tick(&mut self) {
        let limit = f64::from(self.settings.privacy.idle_minutes) * 60.0;
        if self.unlocked && self.settings.privacy.lock_on_idle && system::idle_seconds() >= limit {
            self.lock();
        }
    }

    fn rotation_settings(&self) -> (bool, u32, bool) {
        return (self.settings.auto_rotate, self.settings.rotate_minutes, self.settings.shuffle);
    }

    fn restart_rotation(&mut self) {
        cancel_timer(self.rotate_timer.take());
        if self.settings.auto_rotate {
            let interval = f64::from(self.settings.rotate_minutes) * 60.0;
            self.rotate_timer = Some(schedule_timer(interval, true, || Event::RotateTick));
        }
    }

    fn rotate_tick(&mut self) {
        let eligible = self.selectable_ids().len();
        if !rules::should_rotate(self.settings.auto_rotate, self.paused, &self.verdict, eligible) {
            return;
        }
        if privacy::holds_private(&self.library, self.unlocked, &self.settings.privacy) {
            return;
        }
        let selection = if self.settings.shuffle { Selection::Random } else { Selection::Next };
        let _ = self.select(selection, None);
    }

    fn deny(&self, request: &Request) -> Option<Response> {
        if self.unlocked || !privacy::denies(&self.library, request) {
            return None;
        }
        return Some(Response::err(ErrorCode::Locked, LOCKED));
    }

    pub fn create_folder(&mut self, name: String, item: Option<ItemId>, private: Option<bool>) -> Uuid {
        let id = self.library.create_folder(name);
        if let Some(hidden) = private {
            self.library.set_folder_flags(id, Some(true), Some(hidden));
        }
        self.persist_library();
        if let Some(item) = item
            && (private.is_none() || self.unlocked)
        {
            let _ = self.move_to_folder(&[item], Some(id));
        }
        if private.is_some() {
            self.refresh_visibility();
        }
        return id;
    }

    pub fn set_lock(&mut self, lock: Option<(ItemId, Option<f32>)>) -> Response {
        let Some((item, time)) = lock else {
            self.library.placement.lock = None;
            self.persist_library();
            self.reapply();
            return ok(&serde_json::json!({ "lock": null }));
        };
        let Some(entry) = self.library.items.iter().find(|entry| entry.id == item) else {
            return Response::err(ErrorCode::NotFound, NO_SUCH_ITEM);
        };
        let time = time.unwrap_or(entry.still_time).max(0.0);
        self.library.placement.lock = Some(LockAssignment { item, time });
        self.persist_library();
        self.reapply();
        return ok(&serde_json::json!({ "lock": item.to_string(), "time": time }));
    }

    pub fn save_style(&mut self, style: crate::model::Style, reserved: &[&str]) -> String {
        let name = self.settings.appearance.add_style(style, reserved);
        self.settings.appearance.theme.clone_from(&name);
        self.persist_settings();
        return name;
    }

    pub fn import_styles(&mut self, text: &str, reserved: &[&str]) -> Response {
        let styles = match crate::model::Style::import(text) {
            Ok(styles) => styles,
            Err(message) => return Response::err(ErrorCode::Invalid, message),
        };
        let names: Vec<String> =
            styles.into_iter().map(|style| self.settings.appearance.add_style(style, reserved)).collect();
        if let Some(last) = names.last() {
            self.settings.appearance.theme.clone_from(last);
        }
        self.persist_settings();
        return ok(&serde_json::json!({ "imported": names }));
    }

    pub fn delete_style(&mut self, name: &str, fallback: &str) -> bool {
        let removed = self.settings.appearance.remove_style(name, fallback);
        if removed {
            self.persist_settings();
        }
        return removed;
    }

    pub fn set_folder_icon(&mut self, id: FolderId, icon: Option<&str>) -> Response {
        if let Some(value) = icon {
            match crate::model::Icon::parse(value) {
                None => return Response::err(ErrorCode::Invalid, "icon must be symbol:<name> or image:<file>.png"),
                Some(crate::model::Icon::Image(file)) if !self.paths.icons_dir().join(file).is_file() => {
                    return Response::err(ErrorCode::NotFound, "no such icon image");
                }
                Some(_) => {}
            }
        }
        if !self.library.set_folder_icon(id, icon.map(str::to_string)) {
            return Response::err(ErrorCode::NotFound, NO_SUCH_FOLDER);
        }
        self.persist_library();
        return ok(&serde_json::json!({ "id": id.to_string(), "icon": icon }));
    }

    pub fn import_icon(&self, path: &std::path::Path) -> Result<String, String> {
        let file = format!("{}.png", Uuid::new_v4());
        crate::media::orient::write_png(path, &self.paths.icons_dir().join(&file), ICON_PX)?;
        return Ok(format!("{}{file}", crate::model::IMAGE_PREFIX));
    }

    pub fn custom_icons(&self) -> Vec<String> {
        let mut files: Vec<(std::time::SystemTime, String)> = std::fs::read_dir(self.paths.icons_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let value = format!("{}{name}", crate::model::IMAGE_PREFIX);
                crate::model::Icon::parse(&value)?;
                let modified = entry.metadata().and_then(|meta| meta.modified()).ok()?;
                return Some((modified, value));
            })
            .collect();
        files.sort();
        return files.into_iter().map(|(_, value)| value).collect();
    }

    pub fn remove_icon(&mut self, icon: &str) {
        let Some(crate::model::Icon::Image(file)) = crate::model::Icon::parse(icon) else {
            return;
        };
        remove_quietly(&self.paths.icons_dir().join(file));
        if self.library.drop_icon(icon) > 0 {
            self.persist_library();
        } else {
            crate::ui::changed();
        }
    }

    pub fn reset_item(&mut self, id: ItemId) -> Response {
        let (volume, speed) = (self.settings.volume, self.settings.speed);
        let Some(entry) = self.library.items.iter_mut().find(|entry| entry.id == id) else {
            return Response::err(ErrorCode::NotFound, NO_SUCH_ITEM);
        };
        entry.volume = volume;
        entry.speed = speed;
        entry.muted = true;
        entry.mode = FillMode::Fill;
        entry.modes.clear();
        self.persist_library();
        self.reapply();
        return ok(&serde_json::json!({ "item": id.to_string() }));
    }

    pub fn dispatch(&mut self, request: &Request) -> Response {
        if let Some(denied) = self.deny(request) {
            return denied;
        }
        return self.route(request).unwrap_or_else(|response| response);
    }

    #[allow(clippy::too_many_lines)]
    fn route(&mut self, request: &Request) -> Result<Response, Response> {
        return Ok(match request {
            Request::Status => ok(&StatusReport {
                version: env!("CARGO_PKG_VERSION"),
                paused: self.paused,
                muted: self.settings.muted,
                current: self.current_item().map(|id| id.to_string()),
                surfaces: self.engine.windows().len(),
                playing: self.engine.playing_count(),
                max_pixels: self.max_display_px(),
                app_nap_token: self.nap.active(),
                paused_by: self.paused_by(),
                pause_reason: self.pause_reason().map(|reason| reason.english()),
                locked: !self.unlocked,
            }),
            Request::Pause => self.set_paused(true),
            Request::Resume => self.set_paused(false),
            Request::TogglePause => self.set_paused(!self.paused),
            Request::Mute => self.set_muted(true),
            Request::Unmute => self.set_muted(false),
            Request::Current => ok(&self.current_report()),
            Request::LibraryList => ok(&self.library_view()),
            Request::LibraryRemove { ids } => {
                let removed = self.remove_items(ids);
                self.persist_library();
                self.reapply();
                ok(&serde_json::json!({ "removed": removed }))
            }
            Request::LibraryRename { id, title } => {
                let id = parse_item(id)?;
                if !self.library.rename_item(id, title.clone()) {
                    return Err(Response::err(ErrorCode::NotFound, NO_SUCH_ITEM));
                }
                self.persist_library();
                ok(&serde_json::json!({ "renamed": true }))
            }
            Request::LibraryMove { ids, folder } => {
                let folder = match folder {
                    Some(value) => {
                        let id = parse_folder(value)?;
                        if !self.library.has_folder(id) {
                            return Err(Response::err(ErrorCode::NotFound, NO_SUCH_FOLDER));
                        }
                        Some(id)
                    }
                    None => None,
                };
                let ids: Vec<Uuid> = ids.iter().filter_map(|value| value.parse::<Uuid>().ok()).collect();
                self.move_to_folder(&ids, folder)
            }
            Request::FolderCreate { name } => {
                let id = self.create_folder(name.clone(), None, None);
                ok(&serde_json::json!({ "id": id.to_string(), "name": name }))
            }
            Request::FolderRename { id, name } => {
                let id = parse_folder(id)?;
                if !self.library.rename_folder(id, name.clone()) {
                    return Err(Response::err(ErrorCode::NotFound, NO_SUCH_FOLDER));
                }
                self.persist_library();
                ok(&serde_json::json!({ "id": id.to_string(), "name": name }))
            }
            Request::FolderDelete { id } => self.delete_folder(parse_folder(id)?),
            Request::FolderLock { id, locked, hidden } => self.set_folder_lock(parse_folder(id)?, *locked, *hidden),
            Request::Lock => {
                self.lock();
                ok(&serde_json::json!({ "locked": true }))
            }
            Request::Apply { item, target, transition } => {
                self.apply(item.as_deref(), target, false, transition.as_ref())
            }
            Request::Next { transition } => self.select(Selection::Next, transition.as_ref()),
            Request::Previous { transition } => self.select(Selection::Previous, transition.as_ref()),
            Request::Random { transition } => self.select(Selection::Random, transition.as_ref()),
            Request::SetGeometry { item, mode, geometry } => {
                let id = parse_item(item)?;
                let Some(entry) = self.library.items.iter_mut().find(|entry| entry.id == id) else {
                    return Err(Response::err(ErrorCode::NotFound, NO_SUCH_ITEM));
                };
                entry.mode = *mode;
                entry.modes.insert(*mode, *geometry);
                self.persist_library();
                self.reapply();
                ok(&serde_json::json!({ "item": id.to_string() }))
            }
            Request::SetItem { item, volume, speed, muted, still_time } => {
                self.set_item(item, *volume, *speed, *muted, *still_time)
            }
            Request::Spaces => ok(&serde_json::json!({
                "separate_spaces": self.engine.separate_spaces(),
                "topology": self.engine.spaces(),
            })),
            Request::Windows => ok(&serde_json::json!({
                "windows": self.engine.windows(),
                "nodes": self.engine.node_summary(),
            })),
            Request::Desktop => ok(&self.engine.desktop_report()),
            Request::Rules => ok(&self.rules_report()),
            Request::SettingsGet => ok(&self.settings),
            Request::SettingsSet { patch } => {
                let previous = i18n::select_language(self.settings.language, &system::preferred_languages());
                let rotation = self.rotation_settings();
                if let Err(message) = apply_patch(&mut self.settings, patch) {
                    return Err(Response::err(ErrorCode::Invalid, message));
                }
                self.persist_settings();
                let language = i18n::select_language(self.settings.language, &system::preferred_languages());
                if language != previous {
                    i18n::set_active(language);
                    self.strings = Strings::new(language);
                    self.sync_menu();
                    crate::ui::relocalize();
                }
                if self.rotation_settings() != rotation {
                    self.restart_rotation();
                }
                self.refresh_rules();
                self.sync_idle_timer();
                ok(&self.settings)
            }
            Request::HotkeysGet => ok(&self.hotkeys_report()),
            Request::HotkeysSet { action, combo } => self.set_hotkey(*action, combo.clone()),
            Request::LoginItemGet => ok(&serde_json::json!({ "status": system::login_item_status() })),
            Request::LoginItemSet { enabled } => self.set_login_item(*enabled),
            Request::Menu => ok(&self.menu.report()),
            Request::Optimize { id } => self.start_optimize(id)?,
            Request::OptimizeStatus { id } => self.optimize_status(id)?,
            Request::OptimizeCancel { id } => {
                self.optimize.cancel(parse_item(id)?);
                ok(&serde_json::json!({ "id": id, "state": "cancelled" }))
            }
            Request::DownloadStatus { id } => self.download_status(id),
            Request::DownloadCancel { id } => {
                self.downloads.cancel(id);
                ok(&serde_json::json!({ "id": id, "state": "cancelled" }))
            }
            Request::Downloads => ok(&self.downloads.reports()),
            Request::Quit => ok(&serde_json::json!({})),
            Request::LibraryAdd { .. }
            | Request::Tools
            | Request::ToolsInstall { .. }
            | Request::Probe { .. }
            | Request::Download { .. }
            | Request::Rotate { .. } => Response::err(ErrorCode::Internal, "request is handled asynchronously"),
            Request::FolderIcon { id, icon } => self.set_folder_icon(parse_folder(id)?, icon.as_deref()),
            Request::ItemHotkey { item, combo, scope } => {
                self.set_item_hotkey(parse_item(item)?, combo.as_deref(), *scope)
            }
        });
    }

    pub fn set_item(
        &mut self,
        item: &str,
        volume: Option<f32>,
        speed: Option<f32>,
        muted: Option<bool>,
        still_time: Option<f32>,
    ) -> Response {
        let id = match parse_item(item) {
            Ok(id) => id,
            Err(response) => return response,
        };
        let Some(entry) = self.library.items.iter_mut().find(|entry| entry.id == id) else {
            return Response::err(ErrorCode::NotFound, NO_SUCH_ITEM);
        };
        if let Some(volume) = volume {
            entry.volume = volume.clamp(0.0, 1.0);
        }
        if let Some(speed) = speed {
            entry.speed = speed.clamp(0.25, 4.0);
        }
        if let Some(muted) = muted {
            entry.muted = muted;
        }
        if let Some(still_time) = still_time {
            entry.still_time = still_time.max(0.0);
        }
        self.persist_library();
        self.reapply();
        return ok(&serde_json::json!({ "item": id.to_string() }));
    }

    pub fn set_hotkey(&mut self, action: HotkeyAction, combo: Option<String>) -> Response {
        let Some(text) = combo else {
            return self.bind_hotkey(action, None);
        };
        return match hotkey::parse(&text) {
            Ok(combo) => self.bind_hotkey(action, Some(combo)),
            Err(error) => Response::err(ErrorCode::Invalid, error.to_string()),
        };
    }

    pub fn restore_hotkeys(&mut self) {
        let defaults = proto::Hotkeys::default();
        for action in HotkeyAction::ALL {
            let _ = self.apply_binding(action, None);
        }
        for action in HotkeyAction::ALL {
            let _ = self.apply_binding(action, defaults.get(action).cloned());
        }
        self.persist_settings();
    }

    pub fn record_hotkey(&mut self, recording: bool) {
        for action in HotkeyAction::ALL {
            if recording {
                self.hotkeys.unregister(action);
            } else if let Some(combo) = self.settings.hotkeys.get(action)
                && let Err(error) = self.hotkeys.register(action, combo)
            {
                self.hotkey_failures.insert(action, register_message(error));
            }
        }
        if recording {
            for item in self.hotkeys.registered_items() {
                self.hotkeys.unregister_item(item);
            }
        } else {
            self.register_item_hotkeys();
        }
    }

    fn register_item_hotkeys(&mut self) {
        self.item_hotkey_failures.clear();
        let bound: Vec<(ItemId, KeyCombo)> = self
            .library
            .items
            .iter()
            .filter_map(|item| item.hotkey.as_ref().map(|hotkey| (item.id, hotkey.combo.clone())))
            .collect();
        for (item, combo) in bound {
            if let Err(error) = self.hotkeys.register_item(item, &combo) {
                self.item_hotkey_failures.insert(item, register_message(error));
            }
        }
    }

    pub fn item_hotkey_failure(&self, item: ItemId) -> Option<&str> {
        return self.item_hotkey_failures.get(&item).map(String::as_str);
    }

    pub fn set_item_hotkey(&mut self, item: ItemId, combo: Option<&str>, scope: proto::HotkeyScope) -> Response {
        if !self.library.items.iter().any(|entry| entry.id == item) {
            return Response::err(ErrorCode::NotFound, NO_SUCH_ITEM);
        }
        let Some(text) = combo else {
            self.hotkeys.unregister_item(item);
            self.item_hotkey_failures.remove(&item);
            if let Some(entry) = self.library.items.iter_mut().find(|entry| entry.id == item) {
                entry.hotkey = None;
            }
            self.persist_library();
            return ok(&serde_json::json!({ "item": item.to_string(), "combo": null }));
        };
        let combo = match hotkey::parse(text) {
            Ok(combo) => combo,
            Err(error) => return Response::err(ErrorCode::Invalid, error.to_string()),
        };
        if let Err(error) = hotkey::validate_item(&combo, &self.settings.hotkeys) {
            return Response::err(ErrorCode::Invalid, validation_message(&self.strings, &error, &combo));
        }
        let previous = self
            .library
            .items
            .iter()
            .find(|entry| {
                entry.id != item && entry.hotkey.as_ref().is_some_and(|bound| hotkey::same(&bound.combo, &combo))
            })
            .map(|entry| (entry.id, entry.title.clone()));
        if let Some((other, _)) = &previous {
            self.hotkeys.unregister_item(*other);
        }
        match self.hotkeys.register_item(item, &combo) {
            Ok(()) => {}
            Err(RegisterError::TakenByOtherApp) => {
                if let Some((other, _)) = &previous {
                    self.register_item_hotkeys_for(*other);
                }
                let message = self.strings.format(
                    "%@ is already used by another app. Press another combination.",
                    &[i18n::Arg::Text(hotkey::display(&combo))],
                );
                return Response::err(ErrorCode::Invalid, message);
            }
            Err(error) => return Response::err(ErrorCode::Invalid, register_message(error)),
        }
        if let Some((other, _)) = &previous
            && let Some(entry) = self.library.items.iter_mut().find(|entry| entry.id == *other)
        {
            entry.hotkey = None;
        }
        self.item_hotkey_failures.remove(&item);
        if let Some(entry) = self.library.items.iter_mut().find(|entry| entry.id == item) {
            entry.hotkey = Some(crate::model::ItemHotkey { combo: combo.clone(), scope });
        }
        self.persist_library();
        let moved = previous.map(|(_, title)| {
            return self.strings.format(
                "%@ was moved from “%@”.",
                &[i18n::Arg::Text(hotkey::display(&combo)), i18n::Arg::Text(title)],
            );
        });
        return ok(&serde_json::json!({
            "item": item.to_string(),
            "combo": hotkey::format(&combo),
            "display": hotkey::display(&combo),
            "scope": scope,
            "moved": moved,
        }));
    }

    fn register_item_hotkeys_for(&mut self, item: ItemId) {
        let combo = self.library.items.iter().find(|entry| entry.id == item).and_then(|entry| entry.hotkey.clone());
        if let Some(bound) = combo
            && let Err(error) = self.hotkeys.register_item(item, &bound.combo)
        {
            self.item_hotkey_failures.insert(item, register_message(error));
        }
    }

    pub fn toggle_item(&mut self, item: ItemId) {
        let Some(entry) = self.library.items.iter().find(|entry| entry.id == item && entry.playable) else {
            return;
        };
        let scope = entry.hotkey.as_ref().map_or(proto::HotkeyScope::Desktop, |bound| bound.scope);
        let back = self.showing_raw() == Some(item);
        let plan = hotkey_toggle(
            scope,
            back,
            self.library.placement.lock.map(|lock| lock.item) == Some(item),
            self.library.placement.shortcut_back,
            self.library.placement.shortcut_lock_back,
        );
        if let Some(desktop) = plan.desktop {
            let target = self.hotkey_target();
            if !back {
                self.library.placement.shortcut_back = self.showing_raw().filter(|current| *current != item);
            }
            let next = desktop.resolve(item).filter(|id| self.library.items.iter().any(|entry| entry.id == *id));
            if !back || next.is_some() {
                self.apply_target(next, &target);
                self.persist_library();
                self.reapply();
                self.restart_rotation();
            }
        }
        if let Some(lock) = plan.lock {
            match lock {
                LockMove::Show => {
                    if self.library.placement.lock.map(|lock| lock.item) != Some(item) {
                        self.library.placement.shortcut_lock_back = self.library.placement.lock;
                    }
                    let _ = self.set_lock(Some((item, None)));
                }
                LockMove::Restore(previous) => {
                    let previous = previous.filter(|lock| self.library.items.iter().any(|entry| entry.id == lock.item));
                    self.library.placement.shortcut_lock_back = Some(LockAssignment { item, time: 0.0 });
                    match previous {
                        Some(lock) => {
                            let _ = self.set_lock(Some((lock.item, Some(lock.time))));
                        }
                        None => {
                            let _ = self.set_lock(None);
                        }
                    }
                }
            }
        }
    }

    fn showing_raw(&self) -> Option<ItemId> {
        let (spaces, current) = self.desktops();
        let space = spaces.into_iter().nth(current);
        return self.library.resolve(space.as_deref(), true);
    }

    fn hotkey_target(&self) -> ApplyTarget {
        let (spaces, current) = self.desktops();
        return spaces
            .into_iter()
            .nth(current)
            .filter(|space| {
                self.library.placement.spaces.get(space).is_some_and(|assignment| assignment.item.is_some())
            })
            .map_or(ApplyTarget::Default, |space| ApplyTarget::Space { space });
    }

    fn bind_hotkey(&mut self, action: HotkeyAction, combo: Option<KeyCombo>) -> Response {
        if let Err(response) = self.apply_binding(action, combo) {
            return response;
        }
        self.persist_settings();
        return ok(&self.hotkeys_report());
    }

    fn apply_binding(&mut self, action: HotkeyAction, combo: Option<KeyCombo>) -> Result<(), Response> {
        let Some(combo) = combo else {
            self.hotkeys.unregister(action);
            self.settings.hotkeys.set(action, None);
            self.hotkey_failures.remove(&action);
            return Ok(());
        };
        if let Err(error) = hotkey::validate(&combo, action, &self.settings.hotkeys) {
            return Err(Response::err(ErrorCode::Invalid, validation_message(&self.strings, &error, &combo)));
        }
        if let Some(entry) = self
            .library
            .items
            .iter()
            .find(|entry| entry.hotkey.as_ref().is_some_and(|bound| hotkey::same(&bound.combo, &combo)))
        {
            let message = self.strings.format(
                "%@ is already used by %@. Press another combination.",
                &[i18n::Arg::Text(hotkey::display(&combo)), i18n::Arg::Text(format!("“{}”", entry.title))],
            );
            return Err(Response::err(ErrorCode::Invalid, message));
        }
        match self.hotkeys.register(action, &combo) {
            Ok(()) => {
                self.settings.hotkeys.set(action, Some(combo));
                self.hotkey_failures.remove(&action);
                return Ok(());
            }
            Err(RegisterError::TakenByOtherApp) => {
                let display = hotkey::display(&combo);
                let message = self.strings.format(
                    "%@ is already used by another app. Press another combination.",
                    &[crate::i18n::Arg::Text(display)],
                );
                return Err(Response::err(ErrorCode::Invalid, message));
            }
            Err(error) => return Err(Response::err(ErrorCode::Invalid, register_message(error))),
        }
    }

    fn hotkeys_report(&self) -> Vec<HotkeyEntry> {
        return HotkeyAction::ALL
            .into_iter()
            .map(|action| {
                let combo = self.settings.hotkeys.get(action);
                return HotkeyEntry {
                    action,
                    combo: combo.cloned(),
                    display: combo.map(hotkey::display),
                    registered: self.hotkeys.is_registered(action),
                    conflict: self.hotkey_failures.get(&action).cloned(),
                };
            })
            .collect();
    }

    fn set_login_item(&mut self, enabled: bool) -> Response {
        let status = match system::set_login_item(enabled) {
            Ok(status) => status,
            Err(message) => return Response::err(ErrorCode::Internal, message),
        };
        if matches!(status, LoginItemStatus::RequiresApproval) {
            system::open_login_items_settings();
        }
        self.settings.launch_at_login = matches!(status, LoginItemStatus::Enabled);
        self.persist_settings();
        return ok(&serde_json::json!({ "status": status }));
    }

    fn paused_by(&self) -> Vec<String> {
        let mut by = Vec::new();
        if self.paused {
            by.push("user".to_string());
        }
        if let Some(cause) = &self.verdict.cause {
            by.push(format!("rule:{}", cause.label()));
        }
        return by;
    }

    pub fn pause_reason(&self) -> Option<PauseReason> {
        let animated = self
            .showing_item()
            .and_then(|id| self.library.items.iter().find(|item| item.id == id))
            .is_some_and(|item| item.kind != ItemKind::Image);
        if !animated {
            return None;
        }
        if self.paused {
            return Some(PauseReason::new("Paused by you", "by you", None));
        }
        if self.verdict.action == rules::Action::KeepRunning {
            return None;
        }
        return self.verdict.cause.as_ref().map(PauseReason::from_cause);
    }

    pub fn wallpaper_status(&self) -> Option<WallpaperStatus> {
        let id = self.showing_item()?;
        let item = self.library.items.iter().find(|item| item.id == id)?;
        let preview = self.previewing.map(|previewing| Sound::Preview { here: previewing == id });
        if item.kind == ItemKind::Image {
            return Some(WallpaperStatus { state: PlayState::Still, reason: None, sound: preview });
        }
        let reason = self.pause_reason();
        let state = if self.engine.unloaded(id) {
            PlayState::Unloaded
        } else if reason.is_some() {
            PlayState::Paused
        } else {
            PlayState::Playing
        };
        let sound = if self.settings.muted {
            Sound::AllMuted
        } else if item.muted || item.volume <= 0.0 {
            Sound::ItemMuted
        } else {
            Sound::On
        };
        let sound = preview.or((item.kind == ItemKind::Video && item.audio_codec.is_some()).then_some(sound));
        return Some(WallpaperStatus { state, reason, sound });
    }

    fn publish_status(&mut self) {
        let status = self.wallpaper_status();
        if status != self.shown_status {
            self.shown_status = status;
            self.sync_menu();
            crate::ui::changed();
        }
    }

    fn rules_report(&self) -> RulesReport<'_> {
        return RulesReport {
            snapshot: &self.snapshot,
            action: &self.verdict,
            geometry_polling: self.geometry_timer.is_some(),
            app_nap_token: self.nap.active(),
        };
    }

    fn current_report(&self) -> CurrentReport {
        let item = self.current_item().and_then(|id| self.library.items.iter().find(|entry| entry.id == id));
        let (rate, ready) = item.map_or((0.0, false), |entry| self.engine.rate_ready(entry.id));
        return CurrentReport {
            item: item.map(|entry| entry.id.to_string()),
            title: item.map(|entry| entry.title.clone()),
            kind: item.map(|entry| entry.kind),
            mode: item.map(|entry| entry.mode),
            paused: self.paused,
            muted: self.settings.muted,
            rate,
            ready,
            surfaces: self.engine.windows().len(),
        };
    }

    fn remove_items(&mut self, ids: &[String]) -> usize {
        let parsed: Vec<Uuid> = ids.iter().filter_map(|value| value.parse::<Uuid>().ok()).collect();
        for item in &parsed {
            self.hotkeys.unregister_item(*item);
            self.item_hotkey_failures.remove(item);
        }
        let before = self.library.items.len();
        for item in self.library.items.iter().filter(|item| parsed.contains(&item.id)) {
            self.optimize.cancel(item.id);
            discard_media(&self.paths, item);
        }
        self.library.items.retain(|item| !parsed.contains(&item.id));
        self.library.placement.default.item = clear_if_removed(self.library.placement.default.item, &parsed);
        for assignment in self.library.placement.spaces.values_mut() {
            assignment.item = clear_if_removed(assignment.item, &parsed);
        }
        self.library.placement.lock = self.library.placement.lock.filter(|lock| !parsed.contains(&lock.item));
        self.library.placement.shortcut_back = clear_if_removed(self.library.placement.shortcut_back, &parsed);
        self.library.placement.shortcut_lock_back =
            self.library.placement.shortcut_lock_back.filter(|lock| !parsed.contains(&lock.item));
        self.library.forget_safe(&parsed);
        return before - self.library.items.len();
    }

    fn apply_target(&mut self, item: Option<ItemId>, target: &ApplyTarget) {
        match target {
            ApplyTarget::Default => {
                self.library.placement.default.item = item;
                self.library.remember(None, item);
            }
            ApplyTarget::Space { space } => {
                let assignment = self.library.placement.spaces.entry(space.clone()).or_default();
                assignment.item = item;
                self.library.remember(Some(space), item);
            }
        }
    }

    fn selectable_ids(&self) -> Vec<ItemId> {
        return self
            .library
            .items
            .iter()
            .filter(|item| item.playable && (self.unlocked || !self.library.is_private(item)))
            .map(|item| item.id)
            .collect();
    }

    fn select(&mut self, selection: Selection, transition: Option<&proto::Transition>) -> Response {
        let ids = self.selectable_ids();
        if ids.is_empty() {
            return Response::err(ErrorCode::NotFound, "library is empty");
        }
        let current = self.current_item().and_then(|id| ids.iter().position(|existing| *existing == id));
        let index = match selection {
            Selection::Next => current.map_or(0, |index| (index + 1) % ids.len()),
            Selection::Previous => current.map_or(ids.len() - 1, |index| (index + ids.len() - 1) % ids.len()),
            Selection::Random => random_index(ids.len(), current),
        };
        self.library.placement.default.item = Some(ids[index]);
        self.library.remember(None, Some(ids[index]));
        self.persist_library();
        self.reapply_with(transition);
        self.restart_rotation();
        return ok(&serde_json::json!({ "item": ids[index].to_string() }));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesktopMove {
    Show,
    Back(Option<ItemId>),
}

impl DesktopMove {
    fn resolve(self, item: ItemId) -> Option<ItemId> {
        return match self {
            DesktopMove::Show => Some(item),
            DesktopMove::Back(previous) => previous,
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum LockMove {
    Show,
    Restore(Option<LockAssignment>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Toggle {
    desktop: Option<DesktopMove>,
    lock: Option<LockMove>,
}

fn hotkey_toggle(
    scope: proto::HotkeyScope,
    on_desktop: bool,
    on_lock: bool,
    desktop_back: Option<ItemId>,
    lock_back: Option<LockAssignment>,
) -> Toggle {
    let desktop = || if on_desktop { DesktopMove::Back(desktop_back) } else { DesktopMove::Show };
    let restore = || LockMove::Restore(lock_back);
    return match scope {
        proto::HotkeyScope::Desktop => Toggle { desktop: Some(desktop()), lock: None },
        proto::HotkeyScope::Lock => {
            Toggle { desktop: None, lock: Some(if on_lock { restore() } else { LockMove::Show }) }
        }
        proto::HotkeyScope::Both => Toggle {
            desktop: Some(desktop()),
            lock: match (on_desktop, on_lock) {
                (false, _) => Some(LockMove::Show),
                (true, true) => Some(restore()),
                (true, false) => None,
            },
        },
    };
}

#[derive(Clone, Copy)]
enum Selection {
    Next,
    Previous,
    Random,
}

fn clear_if_removed(item: Option<ItemId>, removed: &[Uuid]) -> Option<ItemId> {
    return item.filter(|id| !removed.contains(id));
}

fn register_all(hotkeys: &proto::Hotkeys) -> (Hotkeys, std::collections::BTreeMap<HotkeyAction, String>) {
    let mut registry = Hotkeys::install();
    let mut failures = std::collections::BTreeMap::new();
    for action in HotkeyAction::ALL {
        let Some(combo) = hotkeys.get(action) else { continue };
        if let Err(error) = registry.register(action, combo) {
            failures.insert(action, register_message(error));
        }
    }
    return (registry, failures);
}

fn register_message(error: RegisterError) -> String {
    return match error {
        RegisterError::TakenByOtherApp => "taken by another app".to_string(),
        RegisterError::Failed(code) => format!("registration failed ({code})"),
    };
}

fn validation_message(strings: &Strings, error: &hotkey::ValidationError, combo: &KeyCombo) -> String {
    return match error {
        hotkey::ValidationError::Reserved(owner) => strings.format(
            "%@ is already used by %@. Press another combination.",
            &[crate::i18n::Arg::Text(hotkey::display(combo)), crate::i18n::Arg::Text(strings.get(owner))],
        ),
        hotkey::ValidationError::Duplicate(action) => strings.format(
            "%@ is already used by %@. Press another combination.",
            &[
                crate::i18n::Arg::Text(hotkey::display(combo)),
                crate::i18n::Arg::Text(strings.get(hotkey::title(*action))),
            ],
        ),
        hotkey::ValidationError::MissingModifier => strings.get("Needs at least one of ⌘ ⌃ ⌥, or an F-key"),
        hotkey::ValidationError::UnknownKey(_) => error.to_string(),
    };
}

fn current_title(library: &Library, unlocked: bool, privacy: &crate::model::Privacy) -> Option<String> {
    return privacy::menu_title(library, unlocked, privacy).map(|title| match title {
        privacy::Title::Name(name) => name,
        privacy::Title::Private => i18n::tr("Private wallpaper"),
    });
}

#[allow(clippy::cast_possible_truncation)]
fn random_index(len: usize, current: Option<usize>) -> usize {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.subsec_nanos());
    let mut index = usize::try_from(seed).unwrap_or(0) % len;
    if Some(index) == current && len > 1 {
        index = (index + 1) % len;
    }
    return index;
}

#[derive(Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct StatusReport {
    version: &'static str,
    paused: bool,
    muted: bool,
    current: Option<String>,
    surfaces: usize,
    playing: usize,
    max_pixels: u32,
    app_nap_token: bool,
    paused_by: Vec<String>,
    pause_reason: Option<String>,
    locked: bool,
}

#[derive(Serialize)]
struct RulesReport<'a> {
    snapshot: &'a rules::Snapshot,
    action: &'a rules::Verdict,
    geometry_polling: bool,
    app_nap_token: bool,
}

#[derive(Serialize)]
struct StateReport {
    paused: bool,
    muted: bool,
}

#[derive(Serialize)]
struct HotkeyEntry {
    action: HotkeyAction,
    combo: Option<KeyCombo>,
    display: Option<String>,
    registered: bool,
    conflict: Option<String>,
}

#[derive(Serialize)]
struct AddReport<'a> {
    added: usize,
    skipped: usize,
    results: &'a [import::PathResult],
}

#[derive(Serialize)]
struct CurrentReport {
    item: Option<String>,
    title: Option<String>,
    kind: Option<ItemKind>,
    mode: Option<FillMode>,
    paused: bool,
    muted: bool,
    rate: f64,
    ready: bool,
    surfaces: usize,
}

fn ok<T: Serialize>(value: &T) -> Response {
    return match serde_json::to_value(value) {
        Ok(data) => Response::ok(data),
        Err(_) => Response::err(ErrorCode::Internal, SERIALIZATION_FAILED),
    };
}

fn parse_id(value: &str, message: &'static str) -> Result<Uuid, Response> {
    return value.parse::<Uuid>().map_err(|_| Response::err(ErrorCode::Invalid, message));
}

fn parse_item(value: &str) -> Result<ItemId, Response> {
    return parse_id(value, INVALID_ITEM_ID);
}

fn parse_folder(value: &str) -> Result<FolderId, Response> {
    return parse_id(value, INVALID_FOLDER_ID);
}

#[cfg(feature = "test-support")]
fn test_unlocked() -> bool {
    return std::env::var_os("LIVEWALL_TEST_UNLOCKED").is_some();
}

#[cfg(not(feature = "test-support"))]
fn test_unlocked() -> bool {
    return false;
}

async fn resolve_download(
    url: &str,
    consent: bool,
    paths: &Paths,
    policy: probe::Policy,
    height: u32,
    av1_hw: bool,
    queue: &download::Queue,
) -> Result<String, (ErrorCode, String)> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err((ErrorCode::Invalid, "invalid url".to_string()));
    }
    let tools_dir = paths.tools_dir();
    let yt_dlp = require_tool(tools::ToolKind::YtDlp, &tools_dir, consent).await?;
    let ffmpeg = require_tool(tools::ToolKind::Ffmpeg, &tools_dir, consent).await?;
    let deno = match tools::locate(tools::ToolKind::Deno, &tools_dir) {
        Some(path) => Some(path),
        None if consent => tools::install(tools::ToolKind::Deno, &tools_dir).await.ok(),
        None => None,
    };
    let spec = download::Spec {
        url: url.to_string(),
        yt_dlp,
        ffmpeg,
        deno,
        incoming: paths.incoming_dir(),
        wallpapers: paths.wallpapers_dir(),
        thumbs: paths.thumbs_dir(),
        policy,
        height,
        av1_hw,
    };
    return Ok(queue.enqueue(spec));
}

async fn require_tool(
    kind: tools::ToolKind,
    tools_dir: &std::path::Path,
    consent: bool,
) -> Result<PathBuf, (ErrorCode, String)> {
    if let Some(path) = tools::locate(kind, tools_dir) {
        return Ok(path);
    }
    if !consent {
        return Err((ErrorCode::ConsentRequired, format!("{kind} is not installed; consent required")));
    }
    return tools::install(kind, tools_dir).await.map_err(|error| (ErrorCode::ToolMissing, error));
}

static TERMINATING: AtomicBool = AtomicBool::new(false);

pub fn terminating() -> bool {
    return TERMINATING.load(Ordering::SeqCst);
}

fn quit(mtm: MainThreadMarker) {
    TERMINATING.store(true, Ordering::SeqCst);
    NSApplication::sharedApplication(mtm).terminate(None);
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

pub fn install(app: App) {
    APP.with(|cell| {
        *cell.borrow_mut() = Some(app);
    });
}

pub fn with<R>(body: impl FnOnce(&mut App) -> R) -> Option<R> {
    return APP.with(|cell| cell.try_borrow_mut().ok().and_then(|mut slot| slot.as_mut().map(body)));
}

pub fn post(event: Event) {
    dispatch2::DispatchQueue::main().exec_async(move || {
        APP.with(|cell| {
            if let Some(app) = cell.borrow_mut().as_mut() {
                app.handle(event);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use proto::HotkeyScope;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn desktop_shortcut_switches_to_the_wallpaper_and_back() {
        let previous = Uuid::new_v4();
        let item = Uuid::new_v4();
        let first = hotkey_toggle(HotkeyScope::Desktop, false, false, None, None);
        assert_eq!(first, Toggle { desktop: Some(DesktopMove::Show), lock: None });
        assert_eq!(first.desktop.map(|step| step.resolve(item)), Some(Some(item)));
        let second = hotkey_toggle(HotkeyScope::Desktop, true, false, Some(previous), None);
        assert_eq!(second.desktop.map(|step| step.resolve(item)), Some(Some(previous)));
        let nowhere = hotkey_toggle(HotkeyScope::Desktop, true, false, None, None);
        assert_eq!(nowhere.desktop.map(|step| step.resolve(item)), Some(None));
    }

    #[test]
    fn lock_screen_shortcut_restores_the_previous_lock_screen() {
        let previous = LockAssignment { item: Uuid::new_v4(), time: 3.0 };
        assert_eq!(hotkey_toggle(HotkeyScope::Lock, false, false, None, None).lock, Some(LockMove::Show));
        assert_eq!(
            hotkey_toggle(HotkeyScope::Lock, false, true, None, Some(previous)).lock,
            Some(LockMove::Restore(Some(previous)))
        );
        assert_eq!(hotkey_toggle(HotkeyScope::Lock, false, true, None, None).lock, Some(LockMove::Restore(None)));
        assert_eq!(hotkey_toggle(HotkeyScope::Lock, true, true, None, None).desktop, None);
    }

    #[test]
    fn the_second_press_goes_back_after_a_restart() {
        let (before, shortcut) = (Uuid::new_v4(), Uuid::new_v4());
        let earlier_lock = LockAssignment { item: before, time: 2.0 };
        let mut library = Library::default();
        library.placement.default.item = Some(shortcut);
        library.placement.lock = Some(LockAssignment { item: shortcut, time: 0.0 });
        library.placement.shortcut_back = Some(before);
        library.placement.shortcut_lock_back = Some(earlier_lock);
        let saved = serde_json::to_string(&library).expect("save");
        let restarted: Library = serde_json::from_str(&saved).expect("load");
        let toggle = hotkey_toggle(
            HotkeyScope::Both,
            true,
            true,
            restarted.placement.shortcut_back,
            restarted.placement.shortcut_lock_back,
        );
        assert_eq!(toggle.desktop.map(|step| step.resolve(shortcut)), Some(Some(before)));
        assert_eq!(toggle.lock, Some(LockMove::Restore(Some(earlier_lock))));
        let empty = serde_json::to_string(&Library::default()).expect("save");
        assert!(!empty.contains("shortcut"), "{empty}");
        let older: Library = serde_json::from_str(r#"{"placement":{"default":{"item":null}}}"#).expect("older");
        assert!(older.placement.shortcut_back.is_none() && older.placement.shortcut_lock_back.is_none());
    }

    #[test]
    fn both_lets_the_desktop_decide_the_direction() {
        let toggle = hotkey_toggle(HotkeyScope::Both, false, true, None, None);
        assert_eq!(toggle, Toggle { desktop: Some(DesktopMove::Show), lock: Some(LockMove::Show) });
        let back = hotkey_toggle(HotkeyScope::Both, true, true, None, None);
        assert_eq!(back, Toggle { desktop: Some(DesktopMove::Back(None)), lock: Some(LockMove::Restore(None)) });
        let mixed = hotkey_toggle(HotkeyScope::Both, true, false, None, None);
        assert_eq!(mixed.lock, None, "a lock screen showing something else is left alone");
    }
}
