pub mod desktop;
pub mod gif;
pub mod image;
pub mod still;
pub mod transition;
pub mod video;
pub mod window;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSScreen;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGWindowLevelForKey, CGWindowLevelKey};
use objc2_foundation::{NSNumber, NSTimer};
use objc2_quartz_core::CALayer;

use crate::engine::desktop::{Desktop, Mode, Target};
use crate::engine::window::WallpaperWindow;
use crate::geometry::{self, Size};
use crate::model::{ItemId, ItemKind, Library};
use crate::paths::Paths;
use crate::platform::spaces::{self, SurfaceKey};
use crate::rules;

pub struct Display {
    pub uuid: String,
    pub id: u32,
    pub frame: CGRect,
    pub scale: f64,
}

pub const OCCLUSION_SETTLE_SECONDS: f64 = 1.0;
pub const WAKE_SETTLE_SECONDS: f64 = 3.0;
pub const WAKE_RECHECK_SECONDS: f64 = 2.0;
pub const WAKE_PROGRESS_EPSILON: f64 = 0.1;
pub const REBUILD_LIMIT: usize = 2;
pub const REBUILD_WINDOW: Duration = Duration::from_mins(10);

#[derive(Clone, Copy)]
struct WakeCheck {
    baseline: f64,
    attempts: u8,
}

struct Surface {
    window: WallpaperWindow,
    spec: spaces::SurfaceSpec,
    frame: CGRect,
    active: bool,
    item: Option<ItemId>,
}

struct Node {
    kind: ItemKind,
    body: NodeBody,
}

enum NodeBody {
    Video(video::VideoNode),
    Gif(gif::GifNode),
    Image(image::ImageNode),
}

impl Node {
    fn attach(&mut self, key: &SurfaceKey, root: &CALayer) {
        match &mut self.body {
            NodeBody::Video(node) => node.attach(key, root),
            NodeBody::Gif(node) => node.attach(key, root),
            NodeBody::Image(node) => node.attach(key, root),
        }
    }

    fn detach(&mut self, key: &SurfaceKey) {
        match &mut self.body {
            NodeBody::Video(node) => node.detach(key),
            NodeBody::Gif(node) => node.detach(key),
            NodeBody::Image(node) => node.detach(key),
        }
    }

    fn set_frame(&self, key: &SurfaceKey, frame: CGRect) {
        match &self.body {
            NodeBody::Video(node) => node.set_frame(key, frame),
            NodeBody::Gif(node) => node.set_frame(key, frame),
            NodeBody::Image(node) => node.set_frame(key, frame),
        }
    }

    fn size(&self) -> (f64, f64) {
        return match &self.body {
            NodeBody::Video(node) => node.size,
            NodeBody::Gif(node) => node.size,
            NodeBody::Image(node) => node.size,
        };
    }

    fn keys(&self) -> Vec<SurfaceKey> {
        return match &self.body {
            NodeBody::Video(node) => node.keys(),
            NodeBody::Gif(node) => node.keys(),
            NodeBody::Image(node) => node.keys(),
        };
    }

    fn layer_count(&self) -> usize {
        return match &self.body {
            NodeBody::Video(node) => node.layer_count(),
            NodeBody::Gif(node) => node.layer_count(),
            NodeBody::Image(node) => node.layer_count(),
        };
    }

    fn layer(&self, key: &SurfaceKey) -> Option<Retained<CALayer>> {
        return match &self.body {
            NodeBody::Video(node) => node.layer(key),
            NodeBody::Gif(node) => node.layer(key),
            NodeBody::Image(node) => node.layer(key),
        };
    }
}

struct ActiveTransition {
    old: ItemId,
    layer: Retained<CALayer>,
    timer: Retained<NSTimer>,
}

pub struct Engine {
    mtm: MainThreadMarker,
    cgs: spaces::Cgs,
    topology: spaces::Topology,
    display: Display,
    surfaces: HashMap<SurfaceKey, Surface>,
    nodes: HashMap<ItemId, Node>,
    desktop: Desktop,
    stills_dir: PathBuf,
    still_pending: HashSet<String>,
    unload_waiting: HashMap<ItemId, (String, f64)>,
    unloaded: HashMap<ItemId, f64>,
    unload_timers: HashMap<ItemId, Retained<NSTimer>>,
    pending_starts: Vec<(SurfaceKey, ItemId)>,
    transitions: HashMap<SurfaceKey, ActiveTransition>,
    playing: usize,
    occluded: bool,
    occlusion_timer: Option<Retained<NSTimer>>,
    wake: HashMap<ItemId, WakeCheck>,
    wake_timers: HashMap<ItemId, Retained<NSTimer>>,
    rebuilds: HashMap<ItemId, Vec<Instant>>,
}

pub struct Playback {
    pub paused: bool,
    pub muted: bool,
    pub stop_after: u32,
    pub verdict: rules::Verdict,
}

impl Engine {
    pub fn new(mtm: MainThreadMarker, paths: &Paths) -> Self {
        let cgs = spaces::Cgs::load();
        let display = main_display(mtm);
        let topology = cgs.topology(&display.uuid);
        let api = crate::platform::desktop::DesktopApi::load();
        let mode = if topology.fallback || !api.per_space() { Mode::Screen } else { Mode::PerSpace };
        let mut desktop = Desktop::new(paths.swap_state_file(), mode);
        desktop.recover(mtm);
        return Self {
            mtm,
            cgs,
            topology,
            display,
            surfaces: HashMap::new(),
            nodes: HashMap::new(),
            desktop,
            stills_dir: paths.stills_dir(),
            still_pending: HashSet::new(),
            unload_waiting: HashMap::new(),
            unloaded: HashMap::new(),
            unload_timers: HashMap::new(),
            pending_starts: Vec::new(),
            transitions: HashMap::new(),
            playing: 0,
            occluded: false,
            occlusion_timer: None,
            wake: HashMap::new(),
            wake_timers: HashMap::new(),
            rebuilds: HashMap::new(),
        };
    }

    pub fn sync(&mut self, library: &Library, playback: &Playback) {
        self.refresh();
        let plan = spaces::plan(&self.topology);
        for action in spaces::diff(&self.existing(), &plan) {
            self.apply_action(action);
        }
        self.reposition();
        self.refresh_active();
        self.reapply(library, playback, None);
    }

    pub fn reapply(
        &mut self,
        library: &Library,
        playback: &Playback,
        transition: Option<&transition::ResolvedTransition>,
    ) {
        self.apply_assignments(library, transition.is_some());
        self.apply_frames(library);
        self.begin_transitions(transition);
        self.update_playback(library, playback);
    }

    pub fn refresh_playback(&mut self, library: &Library, playback: &Playback) {
        self.update_playback(library, playback);
    }

    pub fn any_playing(&self) -> bool {
        return self.playing > 0;
    }

    pub fn playing_count(&self) -> usize {
        return self.playing;
    }

    pub fn occluded(&self) -> bool {
        return self.occluded;
    }

    pub fn on_occlusion_changed(&mut self) -> bool {
        if self.occluded_now() {
            if !self.occluded && self.occlusion_timer.is_none() {
                self.schedule_occlusion_settle();
            }
            return false;
        }
        self.cancel_occlusion_settle();
        if self.occluded {
            self.occluded = false;
            return true;
        }
        return false;
    }

    pub fn on_occlusion_settled(&mut self) -> bool {
        self.occlusion_timer = None;
        if self.occluded_now() && !self.occluded {
            self.occluded = true;
            return true;
        }
        return false;
    }

    fn occluded_now(&self) -> bool {
        let active: Vec<bool> = self
            .surfaces
            .values()
            .filter(|surface| surface.window.is_on_active_space())
            .map(|surface| surface.window.occluded())
            .collect();
        if active.is_empty() {
            return false;
        }
        return active.iter().all(|occluded| *occluded);
    }

    fn schedule_occlusion_settle(&mut self) {
        let block = block2::RcBlock::new(|_timer: std::ptr::NonNull<NSTimer>| {
            crate::app::post(crate::app::Event::OcclusionSettled);
        });
        let timer =
            unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(OCCLUSION_SETTLE_SECONDS, false, &block) };
        crate::platform::apply_timer_tolerance(&timer, OCCLUSION_SETTLE_SECONDS);
        self.occlusion_timer = Some(timer);
    }

    fn cancel_occlusion_settle(&mut self) {
        if let Some(timer) = self.occlusion_timer.take() {
            timer.invalidate();
        }
    }

    pub fn on_wake(&mut self) {
        let playing: Vec<ItemId> = self
            .nodes
            .iter()
            .filter(|(_, node)| matches!(&node.body, NodeBody::Video(video) if video.is_playing()))
            .map(|(item, _)| *item)
            .collect();
        for item in playing {
            self.start_wake_check(item);
        }
    }

    pub fn on_wake_check(&mut self, item: ItemId) {
        let Some(check) = self.wake.get(&item).copied() else { return };
        let current = match self.nodes.get(&item).map(|node| &node.body) {
            Some(NodeBody::Video(video)) if video.is_playing() => video.current_time(),
            _ => {
                self.wake.remove(&item);
                return;
            }
        };
        if advanced(check.baseline, current, WAKE_PROGRESS_EPSILON) {
            self.wake.remove(&item);
            return;
        }
        match check.attempts {
            0 => {
                let Some(baseline) = self.nudge_video(item) else {
                    self.wake.remove(&item);
                    return;
                };
                self.wake.insert(item, WakeCheck { baseline, attempts: 1 });
                self.schedule_wake(item, WAKE_RECHECK_SECONDS);
            }
            1 => {
                let now = Instant::now();
                let ages: Vec<Duration> = self
                    .rebuilds
                    .get(&item)
                    .map_or_else(Vec::new, |times| times.iter().map(|at| now.duration_since(*at)).collect());
                if !rebuild_allowed(&ages, REBUILD_WINDOW, REBUILD_LIMIT) || !self.rebuild_video(item) {
                    self.wake.remove(&item);
                    return;
                }
                self.rebuilds.entry(item).or_default().push(now);
                self.wake.remove(&item);
                tracing::info!(%item, "rebuilt a stalled video after wake");
            }
            _ => {
                self.wake.remove(&item);
            }
        }
    }

    fn start_wake_check(&mut self, item: ItemId) {
        let Some(video) = self.nodes.get(&item).map(|node| &node.body) else { return };
        let NodeBody::Video(video) = video else { return };
        let baseline = video.current_time();
        self.wake.insert(item, WakeCheck { baseline, attempts: 0 });
        self.schedule_wake(item, WAKE_SETTLE_SECONDS);
    }

    fn schedule_wake(&mut self, item: ItemId, delay: f64) {
        if let Some(timer) = self.wake_timers.remove(&item) {
            timer.invalidate();
        }
        let block = block2::RcBlock::new(move |_timer: std::ptr::NonNull<NSTimer>| {
            crate::app::post(crate::app::Event::WakeCheck { item });
        });
        let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(delay, false, &block) };
        crate::platform::apply_timer_tolerance(&timer, delay);
        self.wake_timers.insert(item, timer);
    }

    fn nudge_video(&mut self, item: ItemId) -> Option<f64> {
        let Some(NodeBody::Video(video)) = self.nodes.get_mut(&item).map(|node| &mut node.body) else {
            return None;
        };
        video.nudge();
        return Some(video.current_time());
    }

    fn rebuild_video(&mut self, item: ItemId) -> bool {
        let mtm = self.mtm;
        let Some(NodeBody::Video(video)) = self.nodes.get_mut(&item).map(|node| &mut node.body) else {
            return false;
        };
        video.rebuild(mtm);
        return true;
    }

    pub fn forget(&mut self, item: ItemId) {
        let fading: Vec<SurfaceKey> =
            self.transitions.iter().filter(|(_, active)| active.old == item).map(|(key, _)| key.clone()).collect();
        for key in fading {
            self.finish_transition(&key);
        }
        for surface in self.surfaces.values_mut() {
            if surface.item == Some(item) {
                surface.item = None;
            }
        }
        if let Some(node) = self.nodes.get_mut(&item) {
            for key in node.keys() {
                node.detach(&key);
            }
        }
        self.prune_orphans();
    }

    pub fn on_video_loaded(&mut self, item: ItemId, library: &Library, playback: &Playback) {
        if let Some(node) = self.nodes.get_mut(&item)
            && let NodeBody::Video(video) = &mut node.body
        {
            video.on_loaded(self.mtm);
        }
        self.reapply(library, playback, None);
    }

    pub fn on_gif_tick(&mut self, item: ItemId) {
        if let Some(node) = self.nodes.get_mut(&item)
            && let NodeBody::Gif(gif) = &mut node.body
        {
            gif.tick();
        }
    }

    pub fn refresh_desktop(&mut self, library: &Library, force: bool) {
        let targets = self.desktop_targets();
        let stills = self.resolve_stills(library, &targets);
        self.desktop.update(self.mtm, &targets, &stills, force);
    }

    pub fn desktop_targets(&self) -> Vec<Target> {
        let mut targets: Vec<Target> = Vec::new();
        for surface in self.surfaces.values() {
            if targets.iter().any(|target| target.space == surface.spec.key.space) {
                continue;
            }
            targets.push(Target { space: surface.spec.key.space.clone(), display_id: self.display.id });
        }
        return targets;
    }

    fn resolve_stills(&mut self, library: &Library, targets: &[Target]) -> std::collections::BTreeMap<String, String> {
        let mut stills = std::collections::BTreeMap::new();
        let lock = library.placement.lock.and_then(|lock| {
            let item = playable_item(library, Some(lock.item))?;
            let entry = library.items.iter().find(|entry| entry.id == item)?;
            return Some((entry, lock.time));
        });
        for target in targets {
            let (item, time) = if let Some(found) = lock {
                found
            } else {
                let surface = SurfaceKey::new(target.space.clone());
                let Some(item_id) = playable_item(library, resolve_item(library, &surface)) else { continue };
                let Some(item) = library.items.iter().find(|entry| entry.id == item_id) else { continue };
                (item, item.still_time)
            };
            if let Some(path) = self.still_for(item, time) {
                stills.insert(target.space.clone(), path);
            }
        }
        return stills;
    }

    fn still_for(&mut self, item: &crate::model::Item, time: f32) -> Option<String> {
        if item.kind == ItemKind::Image && still::accepts(&item.path) {
            return Some(item.path.clone());
        }
        let key = still::cache_key(&item.path, still::mtime_secs(&item.path), time);
        let dest = still::cache_file(&self.stills_dir, &key);
        if dest.exists() {
            return Some(dest.to_string_lossy().into_owned());
        }
        let dest = dest.to_string_lossy().into_owned();
        if self.still_pending.insert(dest.clone()) {
            still::generate(item.id, item.kind, &item.path, time, PathBuf::from(&dest));
        }
        return None;
    }

    pub fn on_still_ready(&mut self, item: ItemId, path: &str, library: &Library) {
        self.still_pending.remove(path);
        if let Some(pending) = self.unload_waiting.get(&item)
            && pending.0 == path
        {
            let time = pending.1;
            self.unload_waiting.remove(&item);
            self.perform_unload(item, path, time);
        }
        self.refresh_desktop(library, false);
    }

    pub fn on_still_failed(&mut self, path: &str) {
        self.still_pending.remove(path);
    }

    pub fn on_desktop_verify(&mut self) {
        self.desktop.verify_tick(self.mtm);
    }

    pub fn on_lock(&mut self) {
        self.desktop.on_lock();
    }

    pub fn on_unlock(&mut self, library: &Library) {
        let targets = self.desktop_targets();
        let stills = self.resolve_stills(library, &targets);
        self.desktop.on_unlock(self.mtm, &targets, &stills);
    }

    pub fn desktop_report(&self) -> desktop::DesktopReport {
        return self.desktop.report(self.mtm, &self.desktop_targets());
    }

    pub fn restore_desktop(&mut self) {
        self.desktop.restore(self.mtm);
    }

    pub fn on_unload_tick(&mut self, item: ItemId, library: &Library, playback: &Playback) {
        if self.unloaded.contains_key(&item) {
            return;
        }
        let Some(kind) = self.nodes.get(&item).map(|node| node.kind) else { return };
        match kind {
            ItemKind::Video => self.unload_video_tick(item, library, playback),
            ItemKind::Gif => self.unload_gif_tick(item, playback),
            ItemKind::Image => {}
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    fn unload_video_tick(&mut self, item: ItemId, library: &Library, playback: &Playback) {
        let Some(node) = self.nodes.get(&item) else { return };
        let NodeBody::Video(video) = &node.body else { return };
        if video.is_playing() || !should_unload(video.paused_seconds(), playback.stop_after) {
            return;
        }
        let Some(entry) = library.items.iter().find(|entry| entry.id == item) else { return };
        let time = video.current_time();
        let key = still::cache_key(&entry.path, still::mtime_secs(&entry.path), time as f32);
        let dest = still::cache_file(&self.stills_dir, &key);
        let dest = dest.to_string_lossy().into_owned();
        if std::path::Path::new(&dest).exists() {
            self.perform_unload(item, &dest, time);
        } else if let std::collections::hash_map::Entry::Vacant(slot) = self.unload_waiting.entry(item) {
            slot.insert((dest.clone(), time));
            still::generate(item, ItemKind::Video, &entry.path, time as f32, PathBuf::from(&dest));
        }
    }

    fn unload_gif_tick(&mut self, item: ItemId, playback: &Playback) {
        let Some(node) = self.nodes.get(&item) else { return };
        let NodeBody::Gif(gif) = &node.body else { return };
        let paused = gif.paused_seconds();
        let playing = gif.playing;
        let bytes = gif.cached_bytes();
        if playing || bytes == 0 || !should_unload(paused, playback.stop_after) {
            return;
        }
        self.unload_timers.remove(&item);
        if let Some(node) = self.nodes.get_mut(&item)
            && let NodeBody::Gif(gif) = &mut node.body
        {
            gif.release_frames();
        }
        tracing::info!(%item, bytes, "released paused GIF frames");
    }

    fn perform_unload(&mut self, item: ItemId, still_path: &str, time: f64) {
        let Some(node) = self.nodes.get(&item) else { return };
        let keys = node.keys();
        let Some(image) = image::ImageNode::new(still_path) else { return };
        if let Some(node) = self.nodes.get_mut(&item) {
            for key in &keys {
                node.detach(key);
            }
        }
        self.nodes.insert(item, Node { kind: ItemKind::Image, body: NodeBody::Image(image) });
        for key in keys {
            if let Some(node) = self.nodes.get_mut(&item)
                && let Some(surface) = self.surfaces.get(&key)
            {
                node.attach(&key, surface.window.root());
            }
        }
        self.unloaded.insert(item, time);
        tracing::info!(%item, "unloaded paused video to a still frame");
    }

    fn reload_video(&mut self, item: ItemId, time: f64, library: &Library) {
        let Some(entry) = library.items.iter().find(|entry| entry.id == item) else { return };
        let video = video::VideoNode::new(self.mtm, item, &entry.path);
        let Some(old) = self.nodes.insert(item, Node { kind: ItemKind::Video, body: NodeBody::Video(video) }) else {
            return;
        };
        let keys = old.keys();
        for key in &keys {
            if let Some(node) = self.nodes.get_mut(&item)
                && let NodeBody::Video(video) = &mut node.body
                && let Some(surface) = self.surfaces.get(key)
            {
                video.attach(key, surface.window.root());
            }
        }
        if let Some(node) = self.nodes.get_mut(&item)
            && let NodeBody::Video(video) = &mut node.body
        {
            video.set_pending_seek(time);
        }
        self.unloaded.remove(&item);
    }

    fn schedule_unload(&mut self, item: ItemId, stop_after: u32) {
        if stop_after == 0 || self.unload_timers.contains_key(&item) {
            return;
        }
        let block = block2::RcBlock::new(move |_timer: std::ptr::NonNull<NSTimer>| {
            crate::app::post(crate::app::Event::VideoUnloadTick { item });
        });
        let interval = f64::from(stop_after);
        let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(interval, false, &block) };
        crate::platform::apply_timer_tolerance(&timer, interval);
        self.unload_timers.insert(item, timer);
    }

    fn cancel_unload(&mut self, item: ItemId) {
        if let Some(timer) = self.unload_timers.remove(&item) {
            timer.invalidate();
        }
    }

    pub fn rate_ready(&self, item: ItemId) -> (f64, bool) {
        return match self.nodes.get(&item).map(|node| &node.body) {
            Some(NodeBody::Video(video)) => (f64::from(video.rate()), video.is_ready()),
            _ => (0.0, false),
        };
    }

    pub fn spaces(&self) -> &spaces::Topology {
        return &self.topology;
    }

    pub fn separate_spaces(&self) -> bool {
        return NSScreen::screensHaveSeparateSpaces(self.mtm);
    }

    pub fn display(&self) -> &Display {
        return &self.display;
    }

    pub fn max_display_px(&self) -> u32 {
        let width = pixel_size(self.display.frame.size.width * self.display.scale);
        let height = pixel_size(self.display.frame.size.height * self.display.scale);
        return width.max(height);
    }

    pub fn windows(&self) -> Vec<WindowReport> {
        let level = i64::from(CGWindowLevelForKey(CGWindowLevelKey::DesktopIconWindowLevelKey)) - 1;
        let mut reports = Vec::new();
        for (key, surface) in &self.surfaces {
            let (rate, ready) = match surface.item {
                Some(item) => self.rate_ready(item),
                None => (0.0, false),
            };
            reports.push(WindowReport {
                space: key.space.clone(),
                number: surface.window.window_number(),
                level,
                frame: [
                    surface.frame.origin.x,
                    surface.frame.origin.y,
                    surface.frame.size.width,
                    surface.frame.size.height,
                ],
                is_on_active_space: surface.window.is_on_active_space(),
                active: surface.active,
                can_join_all: surface.spec.can_join_all,
                item: surface.item.map(|item| item.to_string()),
                rate,
                ready,
            });
        }
        reports.sort_by(|a, b| a.space.cmp(&b.space));
        return reports;
    }

    pub fn node_summary(&self) -> Vec<NodeReport> {
        let mut reports = Vec::new();
        for (item, node) in &self.nodes {
            reports.push(NodeReport {
                item: item.to_string(),
                kind: node.kind,
                layers: node.layer_count(),
                keys: node.keys().iter().map(|key| key.space.clone()).collect(),
            });
        }
        reports.sort_by(|a, b| a.item.cmp(&b.item));
        return reports;
    }

    fn refresh(&mut self) {
        self.display = main_display(self.mtm);
        self.topology = self.cgs.topology(&self.display.uuid);
    }

    fn reposition(&mut self) {
        let frame = self.display.frame;
        for surface in self.surfaces.values_mut() {
            if surface.frame != frame {
                surface.window.set_frame(frame);
                surface.frame = frame;
            }
        }
    }

    fn refresh_active(&mut self) {
        let active: Vec<(SurfaceKey, bool)> =
            self.surfaces.keys().map(|key| (key.clone(), self.topology.is_active(&key.space))).collect();
        for (key, is_active) in active {
            if let Some(surface) = self.surfaces.get_mut(&key) {
                surface.active = is_active;
            }
        }
    }

    fn existing(&self) -> Vec<spaces::Existing> {
        let mut existing = Vec::new();
        for surface in self.surfaces.values() {
            existing.push(spaces::Existing {
                key: surface.spec.key.clone(),
                space_id: surface.spec.space_id,
                can_join_all: surface.spec.can_join_all,
            });
        }
        return existing;
    }

    fn apply_action(&mut self, action: spaces::Action) {
        match action {
            spaces::Action::Create(spec) => self.create_surface(spec),
            spaces::Action::Remove(key) => self.remove_surface(&key),
            spaces::Action::Repin(key, space_id) => {
                if let Some(surface) = self.surfaces.get_mut(&key) {
                    let number = surface.window.window_number();
                    if let Some(old) = surface.spec.space_id {
                        self.cgs.unpin_window(number, old);
                    }
                    if let Some(new) = space_id {
                        self.cgs.pin_window(number, new);
                    }
                    surface.spec.space_id = space_id;
                }
            }
        }
    }

    fn create_surface(&mut self, spec: spaces::SurfaceSpec) {
        let frame = self.display.frame;
        let window = WallpaperWindow::create(self.mtm, frame, spec.can_join_all);
        if let Some(space_id) = spec.space_id {
            self.cgs.pin_window(window.window_number(), space_id);
        }
        let active = self.topology.is_active(&spec.key.space);
        self.surfaces.insert(spec.key.clone(), Surface { window, spec, frame, active, item: None });
    }

    fn remove_surface(&mut self, key: &SurfaceKey) {
        self.finish_transition(key);
        let Some(surface) = self.surfaces.remove(key) else { return };
        if let Some(item) = surface.item
            && let Some(node) = self.nodes.get_mut(&item)
        {
            node.detach(key);
        }
        if let Some(space_id) = surface.spec.space_id {
            self.cgs.unpin_window(surface.window.window_number(), space_id);
        }
        surface.window.close();
    }

    fn apply_assignments(&mut self, library: &Library, animate: bool) {
        let assignments: Vec<(SurfaceKey, Option<ItemId>)> =
            self.surfaces.keys().map(|key| (key.clone(), playable_item(library, resolve_item(library, key)))).collect();
        for (key, item) in assignments {
            let current = self.surfaces.get(&key).and_then(|surface| surface.item);
            if current == item {
                continue;
            }
            self.finish_transition(&key);
            let keep_old = animate && current.is_some() && item.is_some();
            if !keep_old {
                if let Some(old) = current
                    && let Some(node) = self.nodes.get_mut(&old)
                {
                    node.detach(&key);
                }
                if let Some(surface) = self.surfaces.get_mut(&key) {
                    surface.item = None;
                }
            }
            if let Some(item) = item {
                self.ensure_node(item, library);
                if let Some(node) = self.nodes.get_mut(&item) {
                    node.attach(&key, self.surfaces[&key].window.root());
                }
                if let Some(surface) = self.surfaces.get_mut(&key) {
                    surface.item = Some(item);
                }
                if keep_old {
                    self.pending_starts.push((key.clone(), current.unwrap()));
                }
            }
        }
        self.prune_orphans();
    }

    fn prune_orphans(&mut self) {
        let orphans: Vec<ItemId> =
            self.nodes.iter().filter(|(_, node)| node.keys().is_empty()).map(|(item, _)| *item).collect();
        for item in orphans {
            self.nodes.remove(&item);
            self.cancel_unload(item);
            self.cancel_wake(item);
            self.unload_waiting.remove(&item);
            self.unloaded.remove(&item);
            self.wake.remove(&item);
            self.rebuilds.remove(&item);
        }
    }

    fn cancel_wake(&mut self, item: ItemId) {
        if let Some(timer) = self.wake_timers.remove(&item) {
            timer.invalidate();
        }
    }

    fn begin_transitions(&mut self, spec: Option<&transition::ResolvedTransition>) {
        let pending = std::mem::take(&mut self.pending_starts);
        if pending.is_empty() {
            return;
        }
        let Some(spec) = spec.filter(|spec| !spec.instant()) else {
            self.detach_pending(&pending);
            return;
        };
        for (key, old) in pending {
            let Some(new) = self.surfaces.get(&key).and_then(|surface| surface.item) else { continue };
            let Some(layer) = self.nodes.get(&new).and_then(|node| node.layer(&key)) else {
                if let Some(node) = self.nodes.get_mut(&old) {
                    node.detach(&key);
                }
                continue;
            };
            transition::apply(&layer, spec);
            let block = block2::RcBlock::new({
                let key = key.clone();
                move |_timer: std::ptr::NonNull<NSTimer>| {
                    crate::app::post(crate::app::Event::TransitionDone { key: key.clone() });
                }
            });
            let interval = spec.timer_seconds();
            let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(interval, false, &block) };
            crate::platform::apply_timer_tolerance(&timer, interval);
            self.transitions.insert(key, ActiveTransition { old, layer, timer });
        }
        self.prune_orphans();
    }

    fn detach_pending(&mut self, pending: &[(SurfaceKey, ItemId)]) {
        for (key, old) in pending {
            if let Some(node) = self.nodes.get_mut(old) {
                node.detach(key);
            }
        }
        self.prune_orphans();
    }

    fn finish_transition(&mut self, key: &SurfaceKey) {
        let Some(active) = self.transitions.remove(key) else { return };
        active.timer.invalidate();
        if let Some(node) = self.nodes.get_mut(&active.old) {
            node.detach(key);
        }
        unsafe { active.layer.setMask(None) };
        self.prune_orphans();
    }

    pub fn on_transition_done(&mut self, key: &SurfaceKey) {
        self.finish_transition(key);
    }

    fn apply_frames(&self, library: &Library) {
        for (key, surface) in &self.surfaces {
            let Some(item) = surface.item else { continue };
            let Some(entry) = library.items.iter().find(|entry| entry.id == item) else { continue };
            let Some(node) = self.nodes.get(&item) else { continue };
            let media = node.size();
            let placement = geometry::resolve(
                entry.mode,
                entry.geometry(),
                Size::new(media.0, media.1),
                Size::new(surface.frame.size.width, surface.frame.size.height),
            );
            let rect = placement.rect;
            let frame = CGRect::new(CGPoint::new(rect.x, rect.y), CGSize::new(rect.w, rect.h));
            node.set_frame(key, frame);
        }
    }

    fn update_playback(&mut self, library: &Library, playback: &Playback) {
        let states: HashMap<SurfaceKey, bool> =
            self.surfaces.iter().map(|(key, surface)| (key.clone(), surface.active)).collect();
        let should = |key: &SurfaceKey| -> bool {
            let Some(active) = states.get(key) else { return false };
            return should_play(playback.paused, playback.verdict.action, *active);
        };
        let mut playing_count = 0usize;
        let mut transitions: Vec<(ItemId, bool, bool)> = Vec::new();
        for (item, node) in &mut self.nodes {
            let playing = node.keys().iter().any(&should);
            if playing && !matches!(node.body, NodeBody::Image(_)) {
                playing_count += 1;
            }
            match &mut node.body {
                NodeBody::Video(video) => {
                    let entry = library.items.iter().find(|entry| entry.id == video.item);
                    let volume = entry.map_or(1.0_f32, |entry| entry.volume);
                    let speed = entry.map_or(1.0_f32, |entry| entry.speed);
                    let muted = playback.muted || entry.is_none_or(|entry| entry.muted);
                    let was = video.is_playing();
                    if video.loaded {
                        video.apply_playback(playing, muted, volume, speed);
                    }
                    let now = video.is_playing();
                    if was != now {
                        transitions.push((*item, was, now));
                    }
                }
                NodeBody::Gif(gif) => {
                    let was = gif.playing;
                    if playing {
                        gif.resume();
                    } else {
                        gif.pause();
                    }
                    if was != gif.playing {
                        transitions.push((*item, was, gif.playing));
                    }
                }
                NodeBody::Image(_) => {}
            }
        }
        let reloading: Vec<(ItemId, f64)> = self
            .unloaded
            .iter()
            .filter(|(item, _)| self.nodes.get(item).is_some_and(|node| node.keys().iter().any(&should)))
            .map(|(item, time)| (*item, *time))
            .collect();
        playing_count += reloading.len();
        for (item, was, now) in transitions {
            if now {
                self.cancel_unload(item);
            } else if was {
                self.schedule_unload(item, playback.stop_after);
            }
        }
        for (item, time) in reloading {
            self.reload_video(item, time, library);
        }
        self.playing = playing_count;
    }

    fn ensure_node(&mut self, item: ItemId, library: &Library) {
        if self.nodes.contains_key(&item) {
            return;
        }
        let Some(entry) = library.items.iter().find(|entry| entry.id == item) else { return };
        let max_px = usize::try_from(self.max_display_px()).unwrap_or(0);
        let body = match entry.kind {
            ItemKind::Video => Some(NodeBody::Video(video::VideoNode::new(self.mtm, item, &entry.path))),
            ItemKind::Gif => gif::GifNode::new(item, &entry.path, max_px).map(NodeBody::Gif),
            ItemKind::Image => image::ImageNode::new(&entry.path).map(NodeBody::Image),
        };
        if let Some(body) = body {
            self.nodes.insert(item, Node { kind: entry.kind, body });
        }
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn pixel_size(points: f64) -> u32 {
    if !points.is_finite() || points <= 0.0 {
        return 0;
    }
    return points.min(f64::from(u32::MAX)) as u32;
}

pub fn should_play(user_paused: bool, action: rules::Action, on_active_space: bool) -> bool {
    return !user_paused && action.keeps_running() && on_active_space;
}

pub fn should_unload(paused_seconds: f64, stop_after: u32) -> bool {
    return stop_after > 0 && paused_seconds >= f64::from(stop_after);
}

pub fn advanced(baseline: f64, current: f64, epsilon: f64) -> bool {
    return (current - baseline).abs() > epsilon;
}

pub fn rebuild_allowed(ages: &[Duration], window: Duration, limit: usize) -> bool {
    return ages.iter().filter(|age| **age < window).count() < limit;
}

pub fn resolve_item(library: &Library, key: &SurfaceKey) -> Option<ItemId> {
    if let Some(assignment) = library.placement.spaces.get(&key.space)
        && assignment.item.is_some()
    {
        return assignment.item;
    }
    return library.placement.default.item;
}

pub fn playable_item(library: &Library, item: Option<ItemId>) -> Option<ItemId> {
    return item.filter(|id| library.items.iter().find(|entry| entry.id == *id).is_some_and(|entry| entry.playable));
}

fn main_display(mtm: MainThreadMarker) -> Display {
    let screens = NSScreen::screens(mtm);
    if screens.count() == 0 {
        return Display { uuid: String::new(), id: 0, frame: CGRect::ZERO, scale: 1.0 };
    }
    let screen = screens.objectAtIndex(0);
    return Display {
        uuid: display_uuid(&screen),
        id: display_id(&screen),
        frame: screen.frame(),
        scale: screen.backingScaleFactor(),
    };
}

fn screen_number(screen: &NSScreen) -> Option<u32> {
    let description = screen.deviceDescription();
    return description
        .objectForKey(&objc2_foundation::NSString::from_str("NSScreenNumber"))
        .and_then(|value| value.downcast_ref::<NSNumber>().map(objc2_foundation::NSNumber::unsignedIntValue));
}

fn display_id(screen: &NSScreen) -> u32 {
    return screen_number(screen).unwrap_or(0);
}

fn display_uuid(screen: &NSScreen) -> String {
    if let Some(identifier) = screen_number(screen) {
        let uuid = unsafe { objc2_color_sync::CGDisplayCreateUUIDFromDisplayID(identifier) };
        if let Some(string) = objc2_core_foundation::CFUUID::new_string(None, Some(&uuid)) {
            return string.to_string();
        }
    }
    return screen.localizedName().to_string();
}

#[derive(serde::Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct WindowReport {
    pub space: String,
    pub number: u32,
    pub level: i64,
    pub frame: [f64; 4],
    pub is_on_active_space: bool,
    pub active: bool,
    pub can_join_all: bool,
    pub item: Option<String>,
    pub rate: f64,
    pub ready: bool,
}

#[derive(serde::Serialize)]
pub struct NodeReport {
    pub item: String,
    pub kind: ItemKind,
    pub layers: usize,
    pub keys: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Assignment, Item};

    #[test]
    fn should_play_requires_every_precondition() {
        use crate::rules::Action;
        assert!(should_play(false, Action::KeepRunning, true));
        assert!(!should_play(true, Action::KeepRunning, true));
        assert!(!should_play(false, Action::Pause, true));
        assert!(!should_play(false, Action::Stop, true));
        assert!(!should_play(false, Action::KeepRunning, false));
    }

    #[test]
    fn unload_fires_only_after_the_delay() {
        assert!(!should_unload(29.0, 30));
        assert!(should_unload(30.0, 30));
        assert!(should_unload(31.0, 30));
    }

    #[test]
    fn unload_never_fires_when_disabled() {
        assert!(!should_unload(9999.0, 0));
    }

    #[test]
    fn advanced_requires_movement_past_the_epsilon() {
        assert!(advanced(1.0, 1.5, WAKE_PROGRESS_EPSILON));
        assert!(!advanced(1.0, 1.05, WAKE_PROGRESS_EPSILON));
        assert!(!advanced(1.0, 1.0, WAKE_PROGRESS_EPSILON));
    }

    #[test]
    fn advanced_treats_a_loop_wrap_as_progress() {
        assert!(advanced(4.9, 0.2, WAKE_PROGRESS_EPSILON));
    }

    #[test]
    fn rebuilds_are_limited_to_two_per_window() {
        assert!(rebuild_allowed(&[], REBUILD_WINDOW, REBUILD_LIMIT));
        assert!(rebuild_allowed(&[Duration::from_secs(1)], REBUILD_WINDOW, REBUILD_LIMIT));
        assert!(!rebuild_allowed(&[Duration::from_secs(1), Duration::from_secs(2)], REBUILD_WINDOW, REBUILD_LIMIT));
    }

    #[test]
    fn rebuilds_outside_the_window_are_forgotten() {
        let old = [Duration::from_secs(601), Duration::from_secs(602)];
        assert!(rebuild_allowed(&old, REBUILD_WINDOW, REBUILD_LIMIT));
    }

    #[test]
    fn resolve_item_prefers_space_then_default() {
        let item_a = ItemId::max();
        let item_b = ItemId::nil();
        let mut library = Library::default();
        library.placement.default.item = Some(item_b);
        library.placement.spaces.insert("S".into(), Assignment { item: Some(item_a), geometry_override: None });
        assert_eq!(resolve_item(&library, &SurfaceKey::new("S")), Some(item_a));
        assert_eq!(resolve_item(&library, &SurfaceKey::new("X")), Some(item_b));
    }

    #[test]
    fn item_geometry_reads_mode_map() {
        let mut item = Item {
            id: ItemId::nil(),
            kind: ItemKind::Video,
            path: "/a.mp4".into(),
            title: "a".into(),
            added_at: jiff::Timestamp::UNIX_EPOCH,
            folder: None,
            mode: proto::FillMode::Custom,
            modes: std::collections::BTreeMap::new(),
            volume: 1.0,
            speed: 1.0,
            muted: true,
            still_time: 0.0,
            optimized: false,
            width: 0,
            height: 0,
            duration: 0.0,
            fps: 0.0,
            codec: None,
            audio_codec: None,
            hdr: false,
            playable: true,
            needs_optimize: false,
            optimize_reason: None,
        };
        assert_eq!(item.geometry(), proto::Geometry::default());
        item.modes.insert(proto::FillMode::Custom, proto::Geometry { x: 0.5, y: 0.0, zoom: 2.0 });
        assert_eq!(item.geometry(), proto::Geometry { x: 0.5, y: 0.0, zoom: 2.0 });
    }
}
