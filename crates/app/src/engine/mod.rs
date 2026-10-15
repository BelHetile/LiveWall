pub mod gif;
pub mod image;
pub mod video;
pub mod window;

use std::collections::HashMap;

use objc2::MainThreadMarker;
use objc2_app_kit::NSScreen;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGWindowLevelForKey, CGWindowLevelKey};
use objc2_foundation::NSNumber;
use objc2_quartz_core::CALayer;

use crate::geometry::{self, Size};
use crate::model::{ItemId, ItemKind, Library};
use crate::platform::spaces::{self, SurfaceKey};
use crate::engine::window::WallpaperWindow;

pub struct Display {
    pub uuid: String,
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
}

pub struct Playback {
    pub paused: bool,
    pub muted: bool,
}

impl Engine {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let cgs = spaces::Cgs::load();
        let displays = collect_displays(mtm);
        let topology = cgs.topology();
        return Self {
            mtm,
            cgs,
            topology,
            displays,
            surfaces: HashMap::new(),
            nodes: HashMap::new(),
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
        let active: HashMap<SurfaceKey, bool> = self
            .surfaces
            .iter()
            .map(|(key, surface)| (key.clone(), surface.active))
            .collect();
        for node in self.nodes.values_mut() {
            let any_active = node.keys().iter().any(|key| active.get(key).copied().unwrap_or(false));
            let playing = should_play(playback.paused, false, true, any_active);
            match &mut node.body {
                NodeBody::Video(video) => {
                    let item = library.items.iter().find(|entry| entry.id == video.item);
                    let volume = item.map_or(1.0_f32, |entry| entry.volume);
                    let speed = item.map_or(1.0_f32, |entry| entry.speed);
                    let muted = playback.muted || item.is_none_or(|entry| entry.muted);
                    if video.loaded {
                        video.apply_playback(playing, muted, volume, speed);
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

#[allow(clippy::fn_params_excessive_bools)]
pub fn should_play(user_paused: bool, low_power: bool, rule_keep_running: bool, on_active_space: bool) -> bool {
    return !user_paused && !low_power && rule_keep_running && on_active_space;
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
        displays.push(Display {
            uuid: display_uuid(&screen),
            frame: screen.frame(),
            scale: screen.backingScaleFactor(),
        });
    }
    return displays;
}

fn display_uuid(screen: &NSScreen) -> String {
    let description = screen.deviceDescription();
    if let Some(value) = description.objectForKey(&objc2_foundation::NSString::from_str("NSScreenNumber"))
        && let Some(number) = value.downcast_ref::<NSNumber>()
    {
        let identifier = number.unsignedIntValue();
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
        assert!(should_play(false, false, true, true));
        assert!(!should_play(true, false, true, true));
        assert!(!should_play(false, true, true, true));
        assert!(!should_play(false, false, false, true));
        assert!(!should_play(false, false, true, false));
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
