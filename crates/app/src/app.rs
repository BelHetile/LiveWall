use std::cell::RefCell;
use std::path::PathBuf;

use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;
use proto::{ApplyTarget, ErrorCode, FillMode, Request, Response};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::engine::{Engine, Playback};
use crate::import;
use crate::model::{Item, ItemId, ItemKind, Library, Settings};
use crate::paths::Paths;
use crate::platform::observe::Observers;
use crate::store;

#[allow(clippy::large_enum_variant)]
pub enum Event {
    Request(Request, oneshot::Sender<Response>),
    Quit,
    ScreensChanged,
    SpacesChanged,
    VideoLoaded { item: ItemId },
    GifTick { item: ItemId },
    StillReady { item: ItemId, path: PathBuf },
    DesktopVerify,
    ScreenLocked,
    ScreenUnlocked,
    VideoUnloadTick { item: ItemId },
}

pub struct App {
    paths: Paths,
    library: Library,
    settings: Settings,
    paused: bool,
    muted: bool,
    engine: Engine,
    _observers: Observers,
}

impl App {
    pub fn new(paths: &Paths, mtm: MainThreadMarker) -> (Self, Vec<store::Warning>) {
        let (library, library_warning) = store::load::<Library>(&paths.library_file());
        let (mut settings, settings_warning) = store::load::<Settings>(&paths.settings_file());
        settings.normalize();
        let warnings = library_warning.into_iter().chain(settings_warning).collect();
        let mut app = Self {
            muted: settings.muted,
            paused: false,
            library,
            settings,
            engine: Engine::new(mtm, paths),
            _observers: Observers::install(mtm),
            paths: paths.clone(),
        };
        app.sync(false);
        return (app, warnings);
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Quit => {
                self.engine.restore_desktop();
                quit();
            }
            Event::ScreensChanged => self.sync(false),
            Event::SpacesChanged => self.sync(true),
            Event::VideoLoaded { item } => {
                let playback = self.playback();
                self.engine.on_video_loaded(item, &self.library, &playback);
            }
            Event::GifTick { item } => self.engine.on_gif_tick(item),
            Event::StillReady { item, path } => {
                self.engine.on_still_ready(item, &path.to_string_lossy(), &self.library);
            }
            Event::DesktopVerify => self.engine.on_desktop_verify(),
            Event::ScreenLocked => self.engine.on_lock(),
            Event::ScreenUnlocked => self.engine.on_unlock(&self.library),
            Event::VideoUnloadTick { item } => {
                let playback = self.playback();
                self.engine.on_unload_tick(item, &self.library, &playback);
            }
            Event::Request(request, reply) => {
                let response = self.dispatch(&request);
                let _ = reply.send(response);
            }
        }
    }

    fn playback(&self) -> Playback {
        return Playback { paused: self.paused, muted: self.muted, stop_after: self.settings.stop_after_seconds };
    }

    fn sync(&mut self, force_desktop: bool) {
        let playback = self.playback();
        self.engine.sync(&self.library, &playback);
        self.engine.refresh_desktop(&self.library, force_desktop);
    }

    fn reapply(&mut self) {
        let playback = self.playback();
        self.engine.reapply(&self.library, &playback);
        self.engine.refresh_desktop(&self.library, false);
    }

    fn persist_library(&self) {
        if let Err(error) = store::save(&self.paths.library_file(), &self.library) {
            tracing::warn!(%error, "cannot save library");
        }
    }

    fn persist_settings(&self) {
        if let Err(error) = store::save(&self.paths.settings_file(), &self.settings) {
            tracing::warn!(%error, "cannot save settings");
        }
    }

    fn current_item(&self) -> Option<ItemId> {
        return self.library.placement.default.item;
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

    #[allow(clippy::too_many_lines)]
    fn dispatch(&mut self, request: &Request) -> Response {
        match request {
            Request::Status => {
                return ok(&StatusReport {
                    version: env!("CARGO_PKG_VERSION"),
                    paused: self.paused,
                    muted: self.muted,
                    current: self.current_item().map(|id| id.to_string()),
                    surfaces: self.engine.windows().len(),
                });
            }
            Request::Pause => return self.set_paused(true),
            Request::Resume => return self.set_paused(false),
            Request::TogglePause => return self.set_paused(!self.paused),
            Request::Mute => return self.set_muted(true),
            Request::Unmute => return self.set_muted(false),
            Request::Current => return ok(&self.current_report()),
            Request::LibraryList => {
                return ok(&LibraryView { items: &self.library.items, folders: &self.library.folders });
            }
            Request::LibraryAdd { paths } => {
                let report = import::add(&self.paths, &mut self.library, paths);
                self.persist_library();
                self.reapply();
                return ok(&AddReport { added: report.added.len(), skipped: report.skipped });
            }
            Request::LibraryRemove { ids } => {
                let removed = self.remove_items(ids);
                self.persist_library();
                self.reapply();
                return ok(&serde_json::json!({ "removed": removed }));
            }
            Request::LibraryRename { id, title } => {
                if let Ok(id) = id.parse::<Uuid>()
                    && let Some(item) = self.library.items.iter_mut().find(|item| item.id == id)
                {
                    item.title.clone_from(title);
                    self.persist_library();
                    return ok(&serde_json::json!({ "renamed": true }));
                }
                return Response::err(ErrorCode::NotFound, "no such item");
            }
            Request::LibraryMove { ids, folder } => {
                let folder = folder.as_ref().and_then(|value| value.parse::<Uuid>().ok());
                for id in ids.iter().filter_map(|value| value.parse::<Uuid>().ok()) {
                    if let Some(item) = self.library.items.iter_mut().find(|item| item.id == id) {
                        item.folder = folder;
                    }
                }
                self.persist_library();
                return ok(&serde_json::json!({ "moved": ids.len() }));
            }
            Request::Apply { item, target } => {
                let item = match item {
                    Some(value) => match value.parse::<Uuid>() {
                        Ok(id) => Some(id),
                        Err(_) => return Response::err(ErrorCode::Invalid, "invalid item id"),
                    },
                    None => None,
                };
                self.apply_target(item, target);
                self.persist_library();
                self.reapply();
                return ok(&serde_json::json!({ "applied": item.map(|id| id.to_string()) }));
            }
            Request::Next => return self.select(Selection::Next),
            Request::Previous => return self.select(Selection::Previous),
            Request::Random => return self.select(Selection::Random),
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
            Request::Displays => return ok(&self.displays_report()),
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
            Request::SettingsGet => return ok(&self.settings),
            Request::SettingsSet { patch } => {
                if let Err(message) = apply_patch(&mut self.settings, patch) {
                    return Response::err(ErrorCode::Invalid, message);
                }
                self.persist_settings();
                return ok(&self.settings);
            }
            Request::Quit => return Response::ok(serde_json::json!({})),
            _ => return Response::err(ErrorCode::NotImplemented, "command not implemented yet"),
        }
    }

    fn set_item(
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

    fn displays_report(&self) -> Vec<DisplayReport> {
        return self
            .engine
            .displays()
            .iter()
            .map(|display| DisplayReport {
                uuid: display.uuid.clone(),
                frame: [display.frame.origin.x, display.frame.origin.y, display.frame.size.width, display.frame.size.height],
                scale: display.scale,
            })
            .collect();
    }

    fn remove_items(&mut self, ids: &[String]) -> usize {
        let parsed: Vec<Uuid> = ids.iter().filter_map(|value| value.parse::<Uuid>().ok()).collect();
        let before = self.library.items.len();
        self.library.items.retain(|item| !parsed.contains(&item.id));
        self.library.placement.default.item = clear_if_removed(self.library.placement.default.item, &parsed);
        for assignment in self.library.placement.displays.values_mut() {
            assignment.item = clear_if_removed(assignment.item, &parsed);
        }
        for assignment in self.library.placement.spaces.values_mut() {
            assignment.item = clear_if_removed(assignment.item, &parsed);
        }
        return before - self.library.items.len();
    }

    fn apply_target(&mut self, item: Option<ItemId>, target: &ApplyTarget) {
        match target {
            ApplyTarget::Default => self.library.placement.default.item = item,
            ApplyTarget::Display { display } => {
                let assignment = self.library.placement.displays.entry(display.clone()).or_default();
                assignment.item = item;
            }
            ApplyTarget::Space { space } => {
                let assignment = self.library.placement.spaces.entry(space.clone()).or_default();
                assignment.item = item;
            }
        }
    }

    fn select(&mut self, selection: Selection) -> Response {
        let ids: Vec<ItemId> = self.library.items.iter().map(|item| item.id).collect();
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
        self.persist_library();
        self.reapply();
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

fn apply_patch(settings: &mut Settings, patch: &Value) -> Result<(), String> {
    let mut base = serde_json::to_value(&*settings).map_err(|error| error.to_string())?;
    if let (Some(base), Some(patch)) = (base.as_object_mut(), patch.as_object()) {
        for (key, value) in patch {
            base.insert(key.clone(), value.clone());
        }
    }
    *settings = serde_json::from_value(base).map_err(|error| error.to_string())?;
    settings.normalize();
    return Ok(());
}

#[derive(Serialize)]
struct StatusReport {
    version: &'static str,
    paused: bool,
    muted: bool,
    current: Option<String>,
    surfaces: usize,
}

#[derive(Serialize)]
struct StateReport {
    paused: bool,
    muted: bool,
}

#[derive(Serialize)]
struct AddReport {
    added: usize,
    skipped: usize,
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
struct DisplayReport {
    uuid: String,
    frame: [f64; 4],
    scale: f64,
}

#[derive(Serialize)]
struct LibraryView<'a> {
    items: &'a [Item],
    folders: &'a [crate::model::Folder],
}

fn ok<T: Serialize>(value: &T) -> Response {
    match serde_json::to_value(value) {
        Ok(data) => Response::ok(data),
        Err(_) => Response::err(ErrorCode::Internal, "serialization failed"),
    }
}

fn quit() {
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

pub fn post(event: Event) {
    dispatch2::DispatchQueue::main().exec_async(move || {
        APP.with(|cell| {
            if let Some(app) = cell.borrow_mut().as_mut() {
                app.handle(event);
            }
        });
    });
}
