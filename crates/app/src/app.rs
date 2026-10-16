use std::cell::RefCell;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::NSTimer;
use objc2_local_authentication::LAContext;
use proto::{ApplyTarget, ErrorCode, FillMode, HotkeyAction, KeyCombo, Request, Response};
use serde::Serialize;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::download;
use crate::engine::{Engine, Playback, resolve_item};
use crate::hotkey;
use crate::i18n::{self, Strings};
use crate::import;
use crate::media::optimize;
use crate::media::probe;
use crate::menu::{Menu, MenuAction, MenuState};
use crate::model::{FolderId, Item, ItemId, ItemKind, Library, LockAssignment, Settings, apply_patch};
use crate::paths::Paths;
use crate::platform::hotkeys::{Hotkeys, RegisterError};
use crate::platform::observe::Observers;
use crate::platform::spaces::{self, SurfaceKey};
use crate::platform::system::{self, LoginItemStatus};
use crate::platform::{auth, detect};
use crate::privacy;
use crate::process;
use crate::rules;
use crate::store;
use crate::tools;

const GEOMETRY_POLL_SECONDS: f64 = 2.0;
const VOLUME_STEP: f32 = 0.1;
const IDLE_POLL_SECONDS: f64 = 10.0;
const UNLOCK_REASON: &str = "Unlock private wallpaper folders";

pub enum Event {
    Request(Request, oneshot::Sender<Response>),
    Quit,
    DisplayChanged,
    SpacesChanged,
    FrontmostChanged,
    AudioChanged,
    AudioSettled,
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
    UnlockResult { success: bool, epoch: u64 },
    VideoLoaded { item: ItemId },
    GifTick { item: ItemId },
    StillReady { item: ItemId, path: PathBuf },
    StillFailed { path: String },
    DesktopVerify,
    ScreenLocked,
    ScreenUnlocked,
    VideoUnloadTick { item: ItemId },
    TransitionDone { key: SurfaceKey },
    Hotkey(HotkeyAction),
    Menu(MenuAction),
    ImportDone { report: import::Report, reply: oneshot::Sender<Response> },
    OptimizeDone { item: ItemId, result: Result<optimize::Optimized, String> },
    DownloadImported { job: String, items: Vec<Item> },
    Reply(Request, Response),
}

#[allow(clippy::struct_excessive_bools)]
pub struct App {
    paths: Paths,
    library: Library,
    settings: Settings,
    paused: bool,
    muted: bool,
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
    engine: Engine,
    nap: system::AppNap,
    observers: Observers,
    hotkeys: Hotkeys,
    hotkey_failures: std::collections::BTreeMap<HotkeyAction, String>,
    strings: Strings,
    menu: Menu,
    handle: tokio::runtime::Handle,
    optimize: Arc<optimize::Queue>,
    downloads: Arc<download::Queue>,
}

impl App {
    pub fn new(paths: &Paths, mtm: MainThreadMarker) -> (Self, Vec<store::Warning>) {
        let (library, library_warning) = store::load::<Library>(&paths.library_file());
        let (mut settings, settings_warning) = store::load::<Settings>(&paths.settings_file());
        settings.normalize();
        settings.launch_at_login = matches!(system::login_item_status(), LoginItemStatus::Enabled);
        let warnings = library_warning.into_iter().chain(settings_warning).collect();

        let strings = Strings::new(i18n::select_language(settings.language, &system::preferred_languages()));
        let (hotkeys, hotkey_failures) = register_all(&settings.hotkeys);
        let muted = settings.muted;
        let unlocked = cfg!(debug_assertions) && std::env::var_os("LIVEWALL_TEST_UNLOCKED").is_some();
        let menu = Menu::install(
            mtm,
            &MenuState {
                title: current_title(&library, unlocked),
                paused: false,
                muted,
                launch_at_login: settings.launch_at_login,
            },
            &strings,
        );

        let handle = tokio::runtime::Handle::current();
        let optimize = Arc::new(optimize::Queue::new(&handle));
        let downloads = Arc::new(download::Queue::new(handle.clone()));
        let mut app = Self {
            muted,
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
            library,
            settings,
            engine: Engine::new(mtm, paths),
            nap: system::AppNap::new(),
            observers: Observers::install(mtm),
            hotkeys,
            hotkey_failures,
            strings,
            menu,
            handle,
            optimize,
            downloads,
            paths: paths.clone(),
        };
        app.sync(false);
        app.refresh_rules();
        app.sync_idle_timer();
        return (app, warnings);
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Quit => self.shutdown(),
            Event::VideoLoaded { item } => {
                let playback = self.playback();
                self.engine.on_video_loaded(item, &self.library, &playback);
                self.refresh_power();
            }
            Event::GifTick { item } => self.engine.on_gif_tick(item),
            Event::StillReady { item, path } => {
                self.engine.on_still_ready(item, &path.to_string_lossy(), &self.library);
                crate::ui::changed();
            }
            Event::StillFailed { path } => self.engine.on_still_failed(&path),
            Event::DesktopVerify => self.engine.on_desktop_verify(),
            Event::VideoUnloadTick { item } => {
                let playback = self.playback();
                self.engine.on_unload_tick(item, &self.library, &playback);
            }
            Event::TransitionDone { key } => {
                self.engine.on_transition_done(&key);
                let playback = self.playback();
                self.engine.refresh_playback(&self.library, &playback);
                self.refresh_power();
            }
            Event::WakeCheck { item } => self.engine.on_wake_check(item),
            Event::Hotkey(action) => self.run_hotkey(action),
            Event::Menu(action) => self.run_menu(action),
            Event::Request(request, reply) => self.request(request, reply),
            Event::ImportDone { report, reply } => {
                let response = self.finish_import(&report);
                let _ = reply.send(response);
            }
            Event::IdleTick => self.idle_tick(),
            Event::UnlockResult { success, epoch } => self.finish_unlock(success, epoch),
            Event::OptimizeDone { item, result } => self.finish_optimize(item, result),
            Event::DownloadImported { job, items } => self.finish_download_import(&job, items),
            Event::Reply(request, response) => crate::ui::reply(&request, &response),
            signal => self.handle_signal(&signal),
        }
        self.sync_menu();
    }

    fn handle_signal(&mut self, event: &Event) {
        match *event {
            Event::DisplayChanged => {
                if self.settings.privacy.lock_on_display && system::online_display_count() > 1 {
                    self.lock();
                }
                self.sync(false);
            }
            Event::SpacesChanged => self.sync(true),
            Event::AudioChanged => {
                self.observers.rebind_audio();
                self.observers.refresh_audio();
            }
            Event::AudioSettled => self.observers.settle_audio(),
            Event::DisplayAsleep(asleep) => {
                self.display_asleep = asleep;
                if asleep && self.settings.privacy.lock_on_screen_lock {
                    self.lock();
                }
            }
            Event::LowPowerChanged => self.low_power = system::low_power_mode(),
            Event::ThermalChanged => self.thermal_hot = system::thermal_hot(),
            Event::ScreensaverChanged(active) => {
                self.screensaver = active;
                if active && self.settings.privacy.lock_on_screen_lock {
                    self.lock();
                }
            }
            Event::OcclusionChanged if !self.engine.on_occlusion_changed() => return,
            Event::OcclusionSettled if !self.engine.on_occlusion_settled() => return,
            Event::ScreenLocked => {
                self.screen_locked = true;
                if self.settings.privacy.lock_on_screen_lock {
                    self.lock();
                }
                self.engine.on_lock();
            }
            Event::ScreenUnlocked => {
                self.screen_locked = false;
                self.engine.on_unlock(&self.library);
            }
            _ => {}
        }
        self.refresh_rules();
        if matches!(*event, Event::DisplayAsleep(false) | Event::SystemWake | Event::ScreenUnlocked) {
            self.engine.on_wake();
        }
    }

    fn request(&mut self, request: Request, reply: oneshot::Sender<Response>) {
        match request {
            Request::LibraryAdd { paths } => self.start_import(paths, reply),
            Request::Probe { path } => self.start_probe(path, reply),
            Request::Tools => self.start_tools(reply),
            Request::ToolsInstall { tool, consent } => self.start_tools_install(&tool, consent, reply),
            Request::Download { url, consent } => self.start_download(url, consent, reply),
            other => {
                let response = self.dispatch(&other);
                let _ = reply.send(response);
            }
        }
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

    fn start_import(&mut self, paths: Vec<String>, reply: oneshot::Sender<Response>) {
        let dest = self.paths.wallpapers_dir();
        let thumbs = self.paths.thumbs_dir();
        let policy = self.probe_policy();
        self.handle.spawn(async move {
            let report = import::run(&paths, &dest, &thumbs, policy, import::Transfer::Copy).await;
            post(Event::ImportDone { report, reply });
        });
    }

    fn finish_import(&mut self, report: &import::Report) -> Response {
        let added = report.imported.len();
        let items: Vec<Item> = report.imported.iter().map(|imported| imported.item.clone()).collect();
        self.add_items(items);
        return ok(&AddReport { added, skipped: report.skipped, results: &report.results });
    }

    fn add_items(&mut self, mut items: Vec<Item>) {
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
        self.handle.spawn(async move {
            let response =
                match resolve_download(&url, consent || settings_consent, &paths, policy, height, av1_hw, &queue).await
                {
                    Ok(id) => Response::ok(serde_json::json!({ "id": id, "state": "queued" })),
                    Err((code, message)) => Response::err(code, message),
                };
            let _ = reply.send(response);
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

    fn start_probe(&mut self, path: String, reply: oneshot::Sender<Response>) {
        let Some(kind) =
            std::path::Path::new(&path).extension().and_then(|value| value.to_str()).and_then(import::kind_for)
        else {
            let _ = reply.send(Response::err(ErrorCode::Media, "unsupported format"));
            return;
        };
        let policy = self.probe_policy();
        self.handle.spawn(async move {
            let task = tokio::task::spawn_blocking(move || {
                let found = probe::inspect(&path, kind);
                let verdict = probe::verdict(&found, policy);
                return (found, verdict);
            });
            let response = match task.await {
                Ok((found, verdict)) => Response::ok(serde_json::json!({ "probe": found, "verdict": verdict })),
                Err(_) => Response::err(ErrorCode::Internal, "probe failed"),
            };
            let _ = reply.send(response);
        });
    }

    fn start_tools(&mut self, reply: oneshot::Sender<Response>) {
        let tools_dir = self.paths.tools_dir();
        self.handle.spawn(async move {
            let report = tools::report(&tools_dir).await;
            let response = serde_json::to_value(&report)
                .map_or_else(|_| Response::err(ErrorCode::Internal, "serialization failed"), Response::ok);
            let _ = reply.send(response);
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
        self.handle.spawn(async move {
            let response = match tools::install(kind, &tools_dir).await {
                Ok(path) => Response::ok(serde_json::json!({ "tool": kind, "path": path.to_string_lossy() })),
                Err(message) => Response::err(ErrorCode::ToolMissing, message),
            };
            let _ = reply.send(response);
        });
    }

    fn on_optimize_requested(&self, item: ItemId) -> Result<(), (ErrorCode, String)> {
        let Some(entry) = self.library.items.iter().find(|entry| entry.id == item) else {
            return Err((ErrorCode::NotFound, "no such item".to_string()));
        };
        if !entry.needs_optimize {
            return Err((ErrorCode::Media, "item does not need optimization".to_string()));
        }
        let tools_dir = self.paths.tools_dir();
        let container =
            std::path::Path::new(&entry.path).extension().and_then(|value| value.to_str()).map(str::to_ascii_lowercase);
        let route = if entry.kind == ItemKind::Gif {
            optimize::Route::Apple
        } else {
            optimize::route(container.as_deref(), entry.codec.as_deref(), probe::av1_hardware())
        };
        let ffmpeg = match route {
            optimize::Route::Apple => None,
            optimize::Route::Ffmpeg => match tools::locate(tools::ToolKind::Ffmpeg, &tools_dir) {
                Some(path) => Some(path),
                None => return Err((ErrorCode::ToolMissing, "ffmpeg is not installed".to_string())),
            },
        };
        let fallback = optimize::Source {
            width: entry.width,
            height: entry.height,
            fps: entry.fps,
            bit_depth: None,
            hdr: entry.hdr,
            transfer: None,
            audio: optimize::audio_plan(entry.audio_codec.as_deref(), false),
        };
        let job = optimize::Job {
            item,
            kind: entry.kind,
            input: PathBuf::from(&entry.path),
            thumbs_dir: self.paths.thumbs_for(self.library.is_private(entry)),
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

    fn start_optimize(&self, id: &str) -> Response {
        let Ok(uuid) = id.parse::<Uuid>() else {
            return Response::err(ErrorCode::Invalid, "invalid item id");
        };
        return match self.on_optimize_requested(uuid) {
            Ok(()) => ok(&serde_json::json!({ "id": id, "state": "queued" })),
            Err((code, message)) => Response::err(code, message),
        };
    }

    fn optimize_status(&self, id: &str) -> Response {
        let Ok(uuid) = id.parse::<Uuid>() else {
            return Response::err(ErrorCode::Invalid, "invalid item id");
        };
        if !self.unlocked && self.library.is_private_id(uuid) {
            return Response::err(ErrorCode::NotFound, "no optimize job for this item");
        }
        return match self.optimize.status(uuid) {
            Some(state) => ok(&state.report(uuid)),
            None => Response::err(ErrorCode::NotFound, "no optimize job for this item"),
        };
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
                let muted = self.muted;
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
        return resolve_item(&self.library, &SurfaceKey::new(space), self.unlocked);
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
                let muted = self.muted;
                let _ = self.set_muted(!muted);
            }
            MenuAction::ToggleLaunchAtLogin => {
                let enabled = !self.settings.launch_at_login;
                let _ = self.set_login_item(enabled);
            }
            MenuAction::Quit => self.shutdown(),
        }
    }

    fn shutdown(&mut self) {
        self.engine.restore_desktop();
        process::kill_all();
        quit();
    }

    fn sync_menu(&self) {
        self.menu.update(
            &MenuState {
                title: current_title(&self.library, self.unlocked),
                paused: self.paused,
                muted: self.muted,
                launch_at_login: self.settings.launch_at_login,
            },
            &self.strings,
        );
    }

    fn playback(&self) -> Playback {
        return Playback {
            paused: self.paused,
            muted: self.muted,
            stop_after: self.settings.stop_after_seconds,
            verdict: self.verdict.clone(),
            unlocked: self.unlocked,
        };
    }

    fn detect(&self) -> rules::Snapshot {
        let display = self.engine.display();
        let frame = rules::Rect::new(
            display.frame.origin.x,
            display.frame.origin.y,
            display.frame.size.width,
            display.frame.size.height,
        );
        let signals = detect::Signals {
            display_asleep: self.display_asleep,
            screen_locked: self.screen_locked,
            screensaver: self.screensaver,
            occluded: self.engine.occluded(),
            thermal_hot: self.thermal_hot,
            low_power: self.low_power,
            other_app_audio: self.observers.audio_active(),
        };
        return detect::snapshot(frame, signals);
    }

    fn refresh_rules(&mut self) {
        self.snapshot = self.detect();
        let config = rules::Config::from_settings(&self.settings);
        self.verdict = rules::evaluate(&self.snapshot, &config);
        let playback = self.playback();
        self.engine.refresh_playback(&self.library, &playback);
        self.refresh_power();
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
            (false, true) => {
                if let Some(timer) = self.geometry_timer.take() {
                    timer.invalidate();
                }
            }
            _ => {}
        }
    }

    fn start_geometry_timer(&mut self) {
        let block = block2::RcBlock::new(|_timer: NonNull<NSTimer>| {
            post(Event::GeometryTick);
        });
        let timer =
            unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(GEOMETRY_POLL_SECONDS, true, &block) };
        crate::platform::apply_timer_tolerance(&timer, GEOMETRY_POLL_SECONDS);
        self.geometry_timer = Some(timer);
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

    pub fn download_progress(&self) -> Vec<download::JobReport> {
        return self
            .downloads
            .reports()
            .into_iter()
            .filter(|report| matches!(report.state, "queued" | "downloading" | "merging" | "importing"))
            .collect();
    }

    pub fn ensure_thumbnail(&self, item: &Item) {
        let (id, kind, path, duration) = (item.id, item.kind, item.path.clone(), item.duration);
        let thumbs = self.paths.thumbs_for(self.library.is_private(item));
        self.handle.spawn(async move {
            if crate::media::thumb::ensure(id, kind, path, duration, thumbs).await.is_some() {
                crate::ui::changed();
            }
        });
    }

    pub fn apply_dock(&self) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let policy = if self.settings.show_in_dock {
            NSApplicationActivationPolicy::Regular
        } else {
            NSApplicationActivationPolicy::Accessory
        };
        let _ = NSApplication::sharedApplication(mtm).setActivationPolicy(policy);
    }

    fn current_item(&self) -> Option<ItemId> {
        return self.library.resolve(None, self.unlocked);
    }

    fn state(&self) -> StateReport {
        return StateReport { paused: self.paused, muted: self.muted };
    }

    fn set_paused(&mut self, paused: bool) -> Response {
        self.paused = paused;
        self.reapply();
        return ok(&self.state());
    }

    fn set_muted(&mut self, muted: bool) -> Response {
        self.muted = muted;
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
        let item = match item {
            Some(value) => match value.parse::<Uuid>() {
                Ok(id) => Some(id),
                Err(_) => return Response::err(ErrorCode::Invalid, "invalid item id"),
            },
            None => None,
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
        let playback = self.playback();
        self.engine.hide_private(&self.library, &playback);
        self.sync_idle_timer();
        self.refresh_power();
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
            (true, false) => {
                let block = block2::RcBlock::new(|_timer: NonNull<NSTimer>| {
                    post(Event::IdleTick);
                });
                let timer =
                    unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(IDLE_POLL_SECONDS, true, &block) };
                crate::platform::apply_timer_tolerance(&timer, IDLE_POLL_SECONDS);
                self.idle_timer = Some(timer);
            }
            (false, true) => {
                if let Some(timer) = self.idle_timer.take() {
                    timer.invalidate();
                }
            }
            _ => {}
        }
    }

    fn idle_tick(&mut self) {
        let limit = f64::from(self.settings.privacy.idle_minutes) * 60.0;
        if self.unlocked && self.settings.privacy.lock_on_idle && system::idle_seconds() >= limit {
            self.lock();
        }
    }

    fn deny(&self, request: &Request) -> Option<Response> {
        if self.unlocked || !privacy::denies(&self.library, request) {
            return None;
        }
        return Some(Response::err(ErrorCode::Locked, "locked"));
    }

    fn library_view(&self) -> LibraryView<'_> {
        return LibraryView {
            items: privacy::visible_items(&self.library, self.unlocked),
            folders: privacy::visible_folders(&self.library, self.unlocked),
        };
    }

    fn optimizing(&self, item: ItemId) -> bool {
        return matches!(
            self.optimize.status(item),
            Some(optimize::JobState::Queued | optimize::JobState::Running { .. })
        );
    }

    fn refresh_visibility(&mut self) {
        if self.unlocked {
            self.reapply();
            return;
        }
        let playback = self.playback();
        self.engine.hide_private(&self.library, &playback);
        self.engine.refresh_desktop(&self.library, false);
        self.refresh_power();
    }

    fn relocate(&mut self, items: &[ItemId], to_private: bool) -> Result<(), Response> {
        if items.iter().any(|item| self.optimizing(*item)) {
            return Err(Response::err(ErrorCode::Busy, "optimize is in progress for this item"));
        }
        for item in items {
            self.engine.forget(*item);
        }
        let lock = self.library.placement.lock;
        if let Err(error) = privacy::relocate_many(&self.paths, &mut self.library.items, items, to_private, lock) {
            self.reapply();
            return Err(Response::err(ErrorCode::Internal, format!("cannot move media: {error}")));
        }
        return Ok(());
    }

    fn move_to_folder(&mut self, ids: &[ItemId], folder: Option<FolderId>) -> Response {
        let to_private = self.library.is_locked_folder(folder);
        let changing: Vec<ItemId> = self
            .library
            .items
            .iter()
            .filter(|item| ids.contains(&item.id) && self.library.is_private(item) != to_private)
            .map(|item| item.id)
            .collect();
        if let Err(response) = self.relocate(&changing, to_private) {
            return response;
        }
        let moved = self.library.move_items(ids, folder);
        self.persist_library();
        self.refresh_visibility();
        return ok(&serde_json::json!({ "moved": moved }));
    }

    fn delete_folder(&mut self, id: FolderId) -> Response {
        if !self.library.has_folder(id) {
            return Response::err(ErrorCode::NotFound, "no such folder");
        }
        if self.library.is_locked_folder(Some(id))
            && let Err(response) = self.relocate(&self.library.folder_items(id), false)
        {
            return response;
        }
        let Some(moved) = self.library.delete_folder(id) else {
            return Response::err(ErrorCode::NotFound, "no such folder");
        };
        self.persist_library();
        self.refresh_visibility();
        return ok(&serde_json::json!({ "deleted": true, "moved": moved }));
    }

    pub fn set_folder_lock(&mut self, id: FolderId, locked: Option<bool>, hidden: Option<bool>) -> Response {
        let Some(folder) = self.library.folders.iter().find(|folder| folder.id == id) else {
            return Response::err(ErrorCode::NotFound, "no such folder");
        };
        let (was_locked, was_hidden) = (folder.locked, folder.hidden);
        let unlocking = locked == Some(false);
        let restyling = hidden.is_some_and(|hidden| hidden != was_hidden);
        if was_locked && !self.unlocked && (unlocking || restyling) {
            return Response::err(ErrorCode::Locked, "locked");
        }
        if let Some(to_private) = locked.filter(|locked| *locked != was_locked) {
            let members = self.library.folder_items(id);
            if let Err(response) = self.relocate(&members, to_private) {
                return response;
            }
        }
        self.library.set_folder_flags(id, locked, hidden);
        self.persist_library();
        self.refresh_visibility();
        return ok(&serde_json::json!({ "id": id.to_string(), "locked": locked, "hidden": hidden }));
    }

    pub fn create_folder(&mut self, name: String, item: Option<ItemId>) -> Uuid {
        let id = self.library.create_folder(name);
        if let Some(item) = item {
            self.library.move_items(&[item], Some(id));
        }
        self.persist_library();
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
            return Response::err(ErrorCode::NotFound, "no such item");
        };
        let time = time.unwrap_or(entry.still_time).max(0.0);
        self.library.placement.lock = Some(LockAssignment { item, time });
        self.persist_library();
        self.reapply();
        return ok(&serde_json::json!({ "lock": item.to_string(), "time": time }));
    }

    pub fn reset_item(&mut self, id: ItemId) -> Response {
        let (volume, speed) = (self.settings.volume, self.settings.speed);
        let Some(entry) = self.library.items.iter_mut().find(|entry| entry.id == id) else {
            return Response::err(ErrorCode::NotFound, "no such item");
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

    #[allow(clippy::too_many_lines)]
    pub fn dispatch(&mut self, request: &Request) -> Response {
        if let Some(denied) = self.deny(request) {
            return denied;
        }
        match request {
            Request::Status => {
                return ok(&StatusReport {
                    version: env!("CARGO_PKG_VERSION"),
                    paused: self.paused,
                    muted: self.muted,
                    current: self.current_item().map(|id| id.to_string()),
                    surfaces: self.engine.windows().len(),
                    playing: self.engine.playing_count(),
                    max_pixels: self.max_display_px(),
                    app_nap_token: self.nap.active(),
                    paused_by: self.paused_by(),
                    locked: !self.unlocked,
                });
            }
            Request::Pause => return self.set_paused(true),
            Request::Resume => return self.set_paused(false),
            Request::TogglePause => return self.set_paused(!self.paused),
            Request::Mute => return self.set_muted(true),
            Request::Unmute => return self.set_muted(false),
            Request::Current => return ok(&self.current_report()),
            Request::LibraryList => return ok(&self.library_view()),
            Request::LibraryRemove { ids } => {
                let removed = self.remove_items(ids);
                self.persist_library();
                self.reapply();
                return ok(&serde_json::json!({ "removed": removed }));
            }
            Request::LibraryRename { id, title } => {
                let Ok(id) = id.parse::<Uuid>() else {
                    return Response::err(ErrorCode::Invalid, "invalid item id");
                };
                if !self.library.rename_item(id, title.clone()) {
                    return Response::err(ErrorCode::NotFound, "no such item");
                }
                self.persist_library();
                return ok(&serde_json::json!({ "renamed": true }));
            }
            Request::LibraryMove { ids, folder } => {
                let folder = match folder {
                    Some(value) => match value.parse::<Uuid>() {
                        Ok(id) if self.library.has_folder(id) => Some(id),
                        Ok(_) => return Response::err(ErrorCode::NotFound, "no such folder"),
                        Err(_) => return Response::err(ErrorCode::Invalid, "invalid folder id"),
                    },
                    None => None,
                };
                let ids: Vec<Uuid> = ids.iter().filter_map(|value| value.parse::<Uuid>().ok()).collect();
                return self.move_to_folder(&ids, folder);
            }
            Request::FolderCreate { name } => {
                let id = self.create_folder(name.clone(), None);
                return ok(&serde_json::json!({ "id": id.to_string(), "name": name }));
            }
            Request::FolderRename { id, name } => {
                let Ok(id) = id.parse::<Uuid>() else {
                    return Response::err(ErrorCode::Invalid, "invalid folder id");
                };
                if !self.library.rename_folder(id, name.clone()) {
                    return Response::err(ErrorCode::NotFound, "no such folder");
                }
                self.persist_library();
                return ok(&serde_json::json!({ "id": id.to_string(), "name": name }));
            }
            Request::FolderDelete { id } => {
                let Ok(id) = id.parse::<Uuid>() else {
                    return Response::err(ErrorCode::Invalid, "invalid folder id");
                };
                return self.delete_folder(id);
            }
            Request::FolderLock { id, locked, hidden } => {
                let Ok(id) = id.parse::<Uuid>() else {
                    return Response::err(ErrorCode::Invalid, "invalid folder id");
                };
                return self.set_folder_lock(id, *locked, *hidden);
            }
            Request::Lock => {
                self.lock();
                return ok(&serde_json::json!({ "locked": true }));
            }
            Request::Apply { item, target, transition } => {
                return self.apply(item.as_deref(), target, false, transition.as_ref());
            }
            Request::Next { transition } => return self.select(Selection::Next, transition.as_ref()),
            Request::Previous { transition } => return self.select(Selection::Previous, transition.as_ref()),
            Request::Random { transition } => return self.select(Selection::Random, transition.as_ref()),
            Request::SetGeometry { item, mode, geometry } => {
                let Ok(id) = item.parse::<Uuid>() else {
                    return Response::err(ErrorCode::Invalid, "invalid item id");
                };
                if let Some(entry) = self.library.items.iter_mut().find(|entry| entry.id == id) {
                    entry.mode = *mode;
                    entry.modes.insert(*mode, *geometry);
                    self.persist_library();
                    self.reapply();
                    return ok(&serde_json::json!({ "item": id.to_string() }));
                }
                return Response::err(ErrorCode::NotFound, "no such item");
            }
            Request::SetItem { item, volume, speed, muted, still_time } => {
                return self.set_item(item, *volume, *speed, *muted, *still_time);
            }
            Request::Spaces => {
                return ok(&serde_json::json!({
                    "separate_spaces": self.engine.separate_spaces(),
                    "topology": self.engine.spaces(),
                }));
            }
            Request::Windows => {
                return ok(&serde_json::json!({
                    "windows": self.engine.windows(),
                    "nodes": self.engine.node_summary(),
                }));
            }
            Request::Desktop => return ok(&self.engine.desktop_report()),
            Request::Rules => return ok(&self.rules_report()),
            Request::SettingsGet => return ok(&self.settings),
            Request::SettingsSet { patch } => {
                let previous = i18n::select_language(self.settings.language, &system::preferred_languages());
                let (muted, dock) = (self.settings.muted, self.settings.show_in_dock);
                if let Err(message) = apply_patch(&mut self.settings, patch) {
                    return Response::err(ErrorCode::Invalid, message);
                }
                self.persist_settings();
                let language = i18n::select_language(self.settings.language, &system::preferred_languages());
                if language != previous {
                    self.strings = Strings::new(language);
                }
                if self.settings.muted != muted {
                    self.muted = self.settings.muted;
                }
                if self.settings.show_in_dock != dock {
                    self.apply_dock();
                }
                self.refresh_rules();
                self.sync_idle_timer();
                return ok(&self.settings);
            }
            Request::HotkeysGet => return ok(&self.hotkeys_report()),
            Request::HotkeysSet { action, combo } => return self.set_hotkey(*action, combo.clone()),
            Request::LoginItemGet => return ok(&serde_json::json!({ "status": system::login_item_status() })),
            Request::LoginItemSet { enabled } => return self.set_login_item(*enabled),
            Request::Menu => return ok(&self.menu.report()),
            Request::Optimize { id } => return self.start_optimize(id),
            Request::OptimizeStatus { id } => return self.optimize_status(id),
            Request::OptimizeCancel { id } => {
                let Ok(uuid) = id.parse::<Uuid>() else {
                    return Response::err(ErrorCode::Invalid, "invalid item id");
                };
                self.optimize.cancel(uuid);
                return ok(&serde_json::json!({ "id": id, "state": "cancelled" }));
            }
            Request::DownloadStatus { id } => return self.download_status(id),
            Request::DownloadCancel { id } => {
                self.downloads.cancel(id);
                return ok(&serde_json::json!({ "id": id, "state": "cancelled" }));
            }
            Request::Downloads => return ok(&self.downloads.reports()),
            Request::Quit => return Response::ok(serde_json::json!({})),
            Request::LibraryAdd { .. }
            | Request::Tools
            | Request::ToolsInstall { .. }
            | Request::Probe { .. }
            | Request::Download { .. } => {
                return Response::err(ErrorCode::Internal, "request is handled asynchronously");
            }
        }
    }

    pub fn set_item(
        &mut self,
        item: &str,
        volume: Option<f32>,
        speed: Option<f32>,
        muted: Option<bool>,
        still_time: Option<f32>,
    ) -> Response {
        let Ok(id) = item.parse::<Uuid>() else {
            return Response::err(ErrorCode::Invalid, "invalid item id");
        };
        let Some(entry) = self.library.items.iter_mut().find(|entry| entry.id == id) else {
            return Response::err(ErrorCode::NotFound, "no such item");
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
            let _ = self.bind_hotkey(action, None);
        }
        for action in HotkeyAction::ALL {
            let _ = self.bind_hotkey(action, defaults.get(action).cloned());
        }
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
    }

    fn bind_hotkey(&mut self, action: HotkeyAction, combo: Option<KeyCombo>) -> Response {
        let Some(combo) = combo else {
            self.hotkeys.unregister(action);
            self.settings.hotkeys.set(action, None);
            self.hotkey_failures.remove(&action);
            self.persist_settings();
            return ok(&self.hotkeys_report());
        };
        if let Err(error) = hotkey::validate(&combo, action, &self.settings.hotkeys) {
            return Response::err(ErrorCode::Invalid, validation_message(&self.strings, &error, &combo));
        }
        match self.hotkeys.register(action, &combo) {
            Ok(()) => {
                self.settings.hotkeys.set(action, Some(combo));
                self.hotkey_failures.remove(&action);
                self.persist_settings();
                return ok(&self.hotkeys_report());
            }
            Err(RegisterError::TakenByOtherApp) => {
                let display = hotkey::display(&combo);
                let message = self.strings.format(
                    "%@ is already used by another app. Press another combination.",
                    &[crate::i18n::Arg::Text(display)],
                );
                return Response::err(ErrorCode::Invalid, message);
            }
            Err(error) => return Response::err(ErrorCode::Invalid, register_message(error)),
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
            muted: self.muted,
            rate,
            ready,
            surfaces: self.engine.windows().len(),
        };
    }

    fn remove_items(&mut self, ids: &[String]) -> usize {
        let parsed: Vec<Uuid> = ids.iter().filter_map(|value| value.parse::<Uuid>().ok()).collect();
        let before = self.library.items.len();
        self.library.items.retain(|item| !parsed.contains(&item.id));
        self.library.placement.default.item = clear_if_removed(self.library.placement.default.item, &parsed);
        for assignment in self.library.placement.spaces.values_mut() {
            assignment.item = clear_if_removed(assignment.item, &parsed);
        }
        self.library.placement.lock = self.library.placement.lock.filter(|lock| !parsed.contains(&lock.item));
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

    fn select(&mut self, selection: Selection, transition: Option<&proto::Transition>) -> Response {
        let ids: Vec<ItemId> = self
            .library
            .items
            .iter()
            .filter(|item| self.unlocked || !self.library.is_private(item))
            .map(|item| item.id)
            .collect();
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
        return ok(&serde_json::json!({ "item": ids[index].to_string() }));
    }
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
        _ => error.to_string(),
    };
}

fn current_title(library: &Library, unlocked: bool) -> Option<String> {
    let id = library.resolve(None, unlocked)?;
    return library.items.iter().find(|item| item.id == id).map(|item| item.title.clone());
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

#[derive(Serialize)]
struct LibraryView<'a> {
    items: Vec<&'a Item>,
    folders: Vec<&'a crate::model::Folder>,
}

fn ok<T: Serialize>(value: &T) -> Response {
    match serde_json::to_value(value) {
        Ok(data) => Response::ok(data),
        Err(_) => Response::err(ErrorCode::Internal, "serialization failed"),
    }
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
        return Err((ErrorCode::ConsentRequired, format!("{} is not installed; consent required", kind.id())));
    }
    return tools::install(kind, tools_dir).await.map_err(|error| (ErrorCode::ToolMissing, error));
}

static TERMINATING: AtomicBool = AtomicBool::new(false);

pub fn terminating() -> bool {
    return TERMINATING.load(Ordering::SeqCst);
}

fn quit() {
    TERMINATING.store(true, Ordering::SeqCst);
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        app.terminate(None);
    }
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
