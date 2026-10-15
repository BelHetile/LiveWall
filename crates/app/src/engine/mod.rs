pub mod desktop;
pub mod gif;
pub mod image;
pub mod still;
pub mod video;
pub mod window;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSScreen;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGWindowLevelForKey, CGWindowLevelKey};
use objc2_foundation::{NSNumber, NSTimer};
use objc2_quartz_core::CALayer;

use crate::geometry::{self, Size};
use crate::model::{ItemId, ItemKind, Library};
use crate::paths::Paths;
use crate::rules;
use crate::platform::spaces::{self, SurfaceKey};
use crate::engine::desktop::{Desktop, Mode, Target};
use crate::engine::window::WallpaperWindow;

pub struct Display {
    pub uuid: String,
    pub id: u32,
    pub frame: CGRect,
    pub scale: f64,
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
}

pub struct Engine {
    mtm: MainThreadMarker,
    cgs: spaces::Cgs,
    topology: spaces::Topology,
    displays: Vec<Display>,
    surfaces: HashMap<SurfaceKey, Surface>,
    nodes: HashMap<ItemId, Node>,
    desktop: Desktop,
    stills_dir: PathBuf,
    still_pending: HashSet<String>,
    unload_waiting: HashMap<ItemId, (String, f64)>,
    unloaded: HashMap<ItemId, f64>,
    unload_timers: HashMap<ItemId, Retained<NSTimer>>,
    playing: usize,
}

pub struct Playback {
    pub paused: bool,
    pub muted: bool,
    pub stop_after: u32,
    pub verdicts: Vec<rules::Verdict>,
}

impl Playback {
    fn action_for(&self, display: &str) -> rules::Action {
        for verdict in &self.verdicts {
            if verdict.display == display {
                return verdict.action;
            }
        }
        return rules::Action::KeepRunning;
    }
}

impl Engine {
    pub fn new(mtm: MainThreadMarker, paths: &Paths) -> Self {
        let cgs = spaces::Cgs::load();
        let displays = collect_displays(mtm);
        let topology = cgs.topology();
        let api = crate::platform::desktop::DesktopApi::load();
        let mode = if topology.fallback || !api.per_space() {
            Mode::Screen
        } else {
            Mode::PerSpace
        };
        let mut desktop = Desktop::new(paths.swap_state_file(), mode);
        desktop.recover(mtm);
        return Self {
            mtm,
            cgs,
            topology,
            displays,
            surfaces: HashMap::new(),
            nodes: HashMap::new(),
            desktop,
            stills_dir: paths.stills_dir(),
            still_pending: HashSet::new(),
            unload_waiting: HashMap::new(),
            unloaded: HashMap::new(),
            unload_timers: HashMap::new(),
            playing: 0,
        };
    }

    pub fn sync(&mut self, library: &Library, playback: &Playback) {
        self.refresh();
        let screens: Vec<String> = self.displays.iter().map(|display| display.uuid.clone()).collect();
        let plan = spaces::plan(&self.topology, &screens);
        for action in spaces::diff(&self.existing(), &plan) {
            self.apply_action(action);
        }
        self.reposition();
        self.refresh_active();
        self.reapply(library, playback);
    }

    pub fn reapply(&mut self, library: &Library, playback: &Playback) {
        self.apply_assignments(library);
        self.apply_frames(library);
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

    pub fn on_video_loaded(&mut self, item: ItemId, library: &Library, playback: &Playback) {
        if let Some(node) = self.nodes.get_mut(&item)
            && let NodeBody::Video(video) = &mut node.body
        {
            video.on_loaded(self.mtm);
        }
        self.reapply(library, playback);
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
        let mut seen = HashSet::new();
        let mut targets = Vec::new();
        for surface in self.surfaces.values() {
            let key = &surface.spec.key;
            if !seen.insert((key.display.clone(), key.space.clone())) {
                continue;
            }
            let Some(display) = self.displays.iter().find(|display| display.uuid == key.display) else { continue };
            targets.push(Target { display: key.display.clone(), space: key.space.clone(), display_id: display.id });
        }
        return targets;
    }

    fn resolve_stills(&mut self, library: &Library, targets: &[Target]) -> std::collections::BTreeMap<String, String> {
        let mut stills = std::collections::BTreeMap::new();
        for target in targets {
            let surface = SurfaceKey::new(target.display.clone(), target.space.clone());
            let Some(item_id) = resolve_item(library, &surface) else { continue };
            let Some(item) = library.items.iter().find(|entry| entry.id == item_id) else { continue };
            if let Some(path) = self.still_for(item) {
                stills.insert(desktop::key(&target.display, &target.space), path);
            }
        }
        return stills;
    }

    fn still_for(&mut self, item: &crate::model::Item) -> Option<String> {
        if item.kind == ItemKind::Image && still::accepts(&item.path) {
            return Some(item.path.clone());
        }
        let key = still::cache_key(&item.path, still::mtime_secs(&item.path), item.still_time);
        let dest = still::cache_file(&self.stills_dir, &key);
        if dest.exists() {
            return Some(dest.to_string_lossy().into_owned());
        }
        let dest = dest.to_string_lossy().into_owned();
        if self.still_pending.insert(dest.clone()) {
            still::generate(item.id, item.kind, &item.path, item.still_time, PathBuf::from(&dest));
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

    #[allow(clippy::cast_possible_truncation)]
    pub fn on_unload_tick(&mut self, item: ItemId, library: &Library, playback: &Playback) {
        if self.unloaded.contains_key(&item) {
            return;
        }
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
        let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(f64::from(stop_after), false, &block) };
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

    pub fn displays(&self) -> &[Display] {
        return &self.displays;
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
                display: key.display.clone(),
                space: key.space.clone(),
                number: surface.window.window_number(),
                level,
                frame: [surface.frame.origin.x, surface.frame.origin.y, surface.frame.size.width, surface.frame.size.height],
                is_on_active_space: surface.window.is_on_active_space(),
                active: surface.active,
                can_join_all: surface.spec.can_join_all,
                item: surface.item.map(|item| item.to_string()),
                rate,
                ready,
            });
        }
        reports.sort_by_key(|report| (report.display.clone(), report.space.clone()));
        return reports;
    }

    pub fn node_summary(&self) -> Vec<NodeReport> {
        let mut reports = Vec::new();
        for (item, node) in &self.nodes {
            reports.push(NodeReport {
                item: item.to_string(),
                kind: node.kind,
                layers: node.layer_count(),
                keys: node.keys().iter().map(|key| format!("{}/{}", key.display, key.space)).collect(),
            });
        }
        reports.sort_by(|a, b| a.item.cmp(&b.item));
        return reports;
    }

    fn refresh(&mut self) {
        self.displays = collect_displays(self.mtm);
        self.topology = self.cgs.topology();
    }

    fn reposition(&mut self) {
        let frames: Vec<(SurfaceKey, CGRect)> = self
            .surfaces
            .keys()
            .map(|key| (key.clone(), self.display_frame(&key.display)))
            .collect();
        for (key, frame) in frames {
            if let Some(surface) = self.surfaces.get_mut(&key)
                && surface.frame != frame
            {
                surface.window.set_frame(frame);
                surface.frame = frame;
            }
        }
    }

    fn refresh_active(&mut self) {
        let active: Vec<(SurfaceKey, bool)> = self
            .surfaces
            .keys()
            .map(|key| (key.clone(), self.topology.is_active(&key.display, &key.space)))
            .collect();
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
        let frame = self.display_frame(&spec.key.display);
        let window = WallpaperWindow::create(self.mtm, frame, spec.can_join_all);
        if let Some(space_id) = spec.space_id {
            self.cgs.pin_window(window.window_number(), space_id);
        }
        let active = self.topology.is_active(&spec.key.display, &spec.key.space);
        self.surfaces.insert(
            spec.key.clone(),
            Surface { window, spec, frame, active, item: None },
        );
    }

    fn remove_surface(&mut self, key: &SurfaceKey) {
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

    fn apply_assignments(&mut self, library: &Library) {
        let assignments: Vec<(SurfaceKey, Option<ItemId>)> = self
            .surfaces
            .keys()
            .map(|key| (key.clone(), resolve_item(library, key)))
            .collect();
        for (key, item) in assignments {
            let current = self.surfaces.get(&key).and_then(|surface| surface.item);
            if current == item {
                continue;
            }
            if let Some(old) = current
                && let Some(node) = self.nodes.get_mut(&old)
            {
                node.detach(&key);
            }
            if let Some(surface) = self.surfaces.get_mut(&key) {
                surface.item = None;
            }
            if let Some(item) = item {
                self.ensure_node(item, library);
                if let Some(node) = self.nodes.get_mut(&item) {
                    node.attach(&key, self.surfaces[&key].window.root());
                }
                if let Some(surface) = self.surfaces.get_mut(&key) {
                    surface.item = Some(item);
                }
            }
        }
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
        let states: HashMap<SurfaceKey, (String, bool)> = self
            .surfaces
            .iter()
            .map(|(key, surface)| (key.clone(), (key.display.clone(), surface.active)))
            .collect();
        let should = |key: &SurfaceKey| -> bool {
            let Some((display, active)) = states.get(key) else { return false };
            return should_play(playback.paused, playback.action_for(display), *active);
        };
        let mut playing_count = 0usize;
        let mut transitions: Vec<(ItemId, bool, bool)> = Vec::new();
        for (item, node) in &mut self.nodes {
            let playing = node.keys().iter().any(&should);
            if playing {
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
                    if playing {
                        gif.resume();
                    } else {
                        gif.pause();
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
        let max_px = self.max_pixel_size();
        let body = match entry.kind {
            ItemKind::Video => Some(NodeBody::Video(video::VideoNode::new(self.mtm, item, &entry.path))),
            ItemKind::Gif => gif::GifNode::new(item, &entry.path, max_px).map(NodeBody::Gif),
            ItemKind::Image => image::ImageNode::new(&entry.path).map(NodeBody::Image),
        };
        if let Some(body) = body {
            self.nodes.insert(item, Node { kind: entry.kind, body });
        }
    }

    fn display_frame(&self, uuid: &str) -> CGRect {
        for display in &self.displays {
            if display.uuid == uuid {
                return display.frame;
            }
        }
        return self.displays.first().map_or(CGRect::ZERO, |display| display.frame);
    }

    fn max_pixel_size(&self) -> usize {
        let mut max = 0u32;
        for display in &self.displays {
            let width = pixel_size(display.frame.size.width * display.scale);
            let height = pixel_size(display.frame.size.height * display.scale);
            max = max.max(width.max(height));
        }
        return usize::try_from(max).unwrap_or(0);
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

pub fn resolve_item(library: &Library, key: &SurfaceKey) -> Option<ItemId> {
    if let Some(assignment) = library.placement.spaces.get(&key.space)
        && assignment.item.is_some()
    {
        return assignment.item;
    }
    if let Some(assignment) = library.placement.displays.get(&key.display)
        && assignment.item.is_some()
    {
        return assignment.item;
    }
    return library.placement.default.item;
}

fn collect_displays(mtm: MainThreadMarker) -> Vec<Display> {
    let screens = NSScreen::screens(mtm);
    let mut displays = Vec::new();
    for index in 0..screens.count() {
        let screen = screens.objectAtIndex(index);
        let id = display_id(&screen);
        displays.push(Display {
            uuid: display_uuid(&screen),
            id,
            frame: screen.frame(),
            scale: screen.backingScaleFactor(),
        });
    }
    return displays;
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
    pub display: String,
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
    fn resolve_item_prefers_space_then_display_then_default() {
        let item_a = ItemId::max();
        let item_b = ItemId::nil();
        let mut library = Library::default();
        library.placement.default.item = Some(item_b);
        library.placement.displays.insert("D".into(), Assignment { item: Some(item_a), geometry_override: None });
        assert_eq!(resolve_item(&library, &SurfaceKey::new("D", "S")), Some(item_a));
        library.placement.spaces.insert("S".into(), Assignment { item: Some(item_b), geometry_override: None });
        assert_eq!(resolve_item(&library, &SurfaceKey::new("D", "S")), Some(item_b));
        assert_eq!(resolve_item(&library, &SurfaceKey::new("X", "Y")), Some(item_b));
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
        };
        assert_eq!(item.geometry(), proto::Geometry::default());
        item.modes.insert(proto::FillMode::Custom, proto::Geometry { x: 0.5, y: 0.0, zoom: 2.0 });
        assert_eq!(item.geometry(), proto::Geometry { x: 0.5, y: 0.0, zoom: 2.0 });
    }
}
