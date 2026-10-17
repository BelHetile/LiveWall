pub mod desktop;
pub mod gif;
pub mod image;
pub mod still;
pub mod transition;
pub mod video;
pub mod window;

mod display;
mod layer;
mod node;
mod occlusion;
mod report;
mod stills;
mod transitions;
mod unload;
mod wake;

use std::collections::HashMap;

use objc2::MainThreadMarker;
use objc2_app_kit::NSScreen;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};

use crate::engine::desktop::{Desktop, Mode};
use crate::engine::display::Display;
use crate::engine::gif::GifNode;
use crate::engine::image::ImageNode;
use crate::engine::node::Node;
use crate::engine::occlusion::Occlusion;
use crate::engine::stills::Stills;
use crate::engine::transitions::ActiveTransition;
use crate::engine::unload::Unload;
use crate::engine::video::VideoNode;
use crate::engine::wake::Wake;
use crate::engine::window::WallpaperWindow;
use crate::geometry::{self, Size};
use crate::model::{ItemId, ItemKind, Library};
use crate::paths::{DirKind, Paths, Visibility};
use crate::platform::spaces::{self, SurfaceKey};
use crate::rules;

struct Surface {
    window: WallpaperWindow,
    spec: spaces::SurfaceSpec,
    frame: CGRect,
    active: bool,
    item: Option<ItemId>,
}

pub struct Engine {
    mtm: MainThreadMarker,
    cgs: spaces::Cgs,
    topology: spaces::Topology,
    display: Display,
    surfaces: HashMap<SurfaceKey, Surface>,
    nodes: HashMap<ItemId, Node>,
    desktop: Desktop,
    stills: Stills,
    unload: Unload,
    pending_starts: Vec<(SurfaceKey, ItemId)>,
    transitions: HashMap<SurfaceKey, ActiveTransition>,
    playing: usize,
    occlusion: Occlusion,
    wake: Wake,
    reveal: bool,
}

pub struct Playback {
    pub paused: bool,
    pub muted: bool,
    pub stop_after: u32,
    pub verdict: rules::Verdict,
    pub reveal: bool,
    pub silenced: Option<ItemId>,
}

impl Engine {
    pub fn new(mtm: MainThreadMarker, paths: &Paths) -> Self {
        let cgs = spaces::Cgs::load();
        let display = Display::main(mtm);
        let topology = cgs.topology(&display.uuid);
        let api = crate::platform::desktop::DesktopApi::load();
        let mode = if topology.fallback || !api.per_space() { Mode::Screen } else { Mode::PerSpace };
        let owned = vec![
            paths.dir_for(DirKind::Stills, Visibility::Public),
            paths.dir_for(DirKind::Stills, Visibility::Private),
            paths.dir_for(DirKind::Wallpapers, Visibility::Public),
            paths.dir_for(DirKind::Wallpapers, Visibility::Private),
        ];
        let mut desktop = Desktop::new(paths.swap_state_file(), mode, owned);
        desktop.recover(mtm);
        return Self {
            mtm,
            cgs,
            topology,
            display,
            surfaces: HashMap::new(),
            nodes: HashMap::new(),
            desktop,
            stills: Stills::new(paths),
            unload: Unload::default(),
            pending_starts: Vec::new(),
            transitions: HashMap::new(),
            playing: 0,
            occlusion: Occlusion::default(),
            wake: Wake::default(),
            reveal: true,
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
        self.reveal = playback.reveal;
        self.apply_assignments(library, transition.is_some(), playback.reveal);
        self.apply_frames(library);
        self.begin_transitions(transition);
        self.update_playback(library, playback);
    }

    pub fn hide_private(&mut self, library: &Library, playback: &Playback) {
        if !playback.reveal {
            self.finish_transitions_matching(|active| library.is_private_id(active.old));
        }
        self.reapply(library, playback, None);
    }

    pub fn refresh_playback(&mut self, library: &Library, playback: &Playback) {
        self.update_playback(library, playback);
    }

    pub fn any_playing(&self) -> bool {
        return self.playing > 0;
    }

    pub fn unloaded(&self, item: ItemId) -> bool {
        return self.unload.unloaded.contains_key(&item);
    }

    pub fn playing_count(&self) -> usize {
        return self.playing;
    }

    pub fn forget(&mut self, item: ItemId) {
        self.finish_transitions_matching(|active| active.old == item);
        for surface in self.surfaces.values_mut() {
            if surface.item == Some(item) {
                surface.item = None;
            }
        }
        if let Some(node) = self.nodes.get_mut(&item) {
            node.layers_mut().clear();
        }
        self.prune_orphans();
    }

    pub fn on_video_loaded(&mut self, item: ItemId, library: &Library, playback: &Playback) {
        if let Some(video) = self.nodes.get_mut(&item).and_then(Node::video_mut) {
            video.on_loaded(self.mtm);
        }
        self.reapply(library, playback, None);
        self.check_cover(item);
    }

    pub fn on_gif_tick(&mut self, item: ItemId) {
        if let Some(gif) = self.nodes.get_mut(&item).and_then(Node::gif_mut) {
            gif.tick();
        }
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
        return self.display.max_px();
    }

    fn refresh(&mut self) {
        self.display = Display::main(self.mtm);
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
        if let Some(item) = surface.item {
            self.detach(item, key);
        }
        if let Some(space_id) = surface.spec.space_id {
            self.cgs.unpin_window(surface.window.window_number(), space_id);
        }
        surface.window.close();
    }

    fn attach(&mut self, item: ItemId, key: &SurfaceKey) {
        if let Some(node) = self.nodes.get_mut(&item)
            && let Some(surface) = self.surfaces.get(key)
        {
            node.attach(key, surface.window.root());
        }
    }

    fn detach(&mut self, item: ItemId, key: &SurfaceKey) {
        if let Some(node) = self.nodes.get_mut(&item) {
            node.layers_mut().detach(key);
        }
    }

    fn apply_assignments(&mut self, library: &Library, animate: bool, reveal: bool) {
        let assignments: Vec<(SurfaceKey, Option<ItemId>)> = self
            .surfaces
            .keys()
            .map(|key| (key.clone(), library.playable_item(library.resolve(Some(&key.space), reveal))))
            .collect();
        for (key, item) in assignments {
            let current = self.surfaces.get(&key).and_then(|surface| surface.item);
            if current == item {
                continue;
            }
            self.finish_transition(&key);
            let keep_old = animate && current.is_some() && item.is_some();
            if !keep_old {
                if let Some(old) = current {
                    self.detach(old, &key);
                }
                if let Some(surface) = self.surfaces.get_mut(&key) {
                    surface.item = None;
                }
            }
            if let Some(item) = item {
                self.ensure_node(item, library);
                self.attach(item, &key);
                if let Some(surface) = self.surfaces.get_mut(&key) {
                    surface.item = Some(item);
                }
                if let Some(old) = current
                    && keep_old
                {
                    self.pending_starts.push((key.clone(), old));
                }
            }
        }
        self.prune_orphans();
    }

    fn prune_orphans(&mut self) {
        let orphans: Vec<ItemId> =
            self.nodes.iter().filter(|(_, node)| node.layers().is_empty()).map(|(item, _)| *item).collect();
        for item in orphans {
            self.nodes.remove(&item);
            self.unload.forget(item);
            self.wake.forget(item);
        }
    }

    pub(super) fn apply_frames(&self, library: &Library) {
        for (key, surface) in &self.surfaces {
            let Some(item) = surface.item else { continue };
            let Some(entry) = library.items.iter().find(|entry| entry.id == item) else { continue };
            let Some(node) = self.nodes.get(&item) else { continue };
            let placement = geometry::resolve(
                entry.mode,
                entry.geometry(),
                node.size(),
                Size::new(surface.frame.size.width, surface.frame.size.height),
            );
            let rect = placement.rect;
            let frame = CGRect::new(CGPoint::new(rect.x, rect.y), CGSize::new(rect.w, rect.h));
            node.layers().set_frame(key, frame);
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
        let mut resumed: Vec<ItemId> = Vec::new();
        let mut idle: Vec<ItemId> = Vec::new();
        for (item, node) in &mut self.nodes {
            let playing = node.layers().keys().any(&should);
            if playing && !matches!(node, Node::Image(_)) {
                playing_count += 1;
            }
            match node {
                Node::Video(video) => {
                    let entry = library.items.iter().find(|entry| entry.id == video.item);
                    let volume = entry.map_or(1.0_f32, |entry| entry.volume);
                    let speed = entry.map_or(1.0_f32, |entry| entry.speed);
                    let muted = playback.muted
                        || playback.silenced == Some(video.item)
                        || entry.is_none_or(|entry| entry.muted);
                    let was = video.is_playing();
                    if video.loaded {
                        video.apply_playback(playing, muted, volume, speed);
                    }
                    if video.is_playing() {
                        if !was {
                            resumed.push(*item);
                        }
                    } else {
                        idle.push(*item);
                    }
                }
                Node::Gif(gif) => {
                    let was = gif.playing;
                    if playing {
                        gif.resume();
                    } else {
                        gif.pause();
                    }
                    if gif.playing {
                        if !was {
                            resumed.push(*item);
                        }
                    } else if gif.cached_bytes() > 0 {
                        idle.push(*item);
                    }
                }
                Node::Image(_) => {}
            }
        }
        let reloading: Vec<(ItemId, f64)> = self
            .unload
            .unloaded
            .iter()
            .filter(|(item, _)| self.nodes.get(item).is_some_and(|node| node.layers().keys().any(&should)))
            .map(|(item, unloaded)| (*item, unloaded.time))
            .collect();
        playing_count += reloading.len();
        for item in resumed {
            self.unload.cancel(item);
        }
        for item in idle {
            self.unload.schedule(item, playback.stop_after);
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
        if let Some(node) = self.build_node(item, library) {
            self.nodes.insert(item, node);
        }
    }

    fn build_node(&self, item: ItemId, library: &Library) -> Option<Node> {
        let entry = library.items.iter().find(|entry| entry.id == item)?;
        let max_px = usize::try_from(self.display.max_px()).unwrap_or(0);
        return match entry.kind {
            ItemKind::Video => Some(Node::Video(VideoNode::new(self.mtm, item, &entry.path))),
            ItemKind::Gif => GifNode::new(item, &entry.path, max_px).map(Node::Gif),
            ItemKind::Image => ImageNode::new(&entry.path).map(Node::Image),
        };
    }

    pub fn rehome(&mut self, item: ItemId, library: &Library) {
        self.unload.cancel(item);
        if let Some(time) = self.unload.unloaded.get(&item).map(|unloaded| unloaded.time) {
            self.reload_video(item, time, library);
            return;
        }
        let Some(node) = self.nodes.get_mut(&item) else { return };
        if let Some(video) = node.video_mut() {
            let time = video.current_time();
            video.apply_playback(false, true, 0.0, 1.0);
            self.reload_video(item, time, library);
            return;
        }
        let keys: Vec<SurfaceKey> = node.layers().keys().cloned().collect();
        let Some(fresh) = self.build_node(item, library) else { return };
        layer::without_actions(|| {
            let Some(mut old) = self.nodes.insert(item, fresh) else { return };
            for key in &keys {
                self.attach(item, key);
            }
            self.apply_frames(library);
            old.layers_mut().clear();
        });
    }
}

pub fn should_play(user_paused: bool, action: rules::Action, on_active_space: bool) -> bool {
    return !user_paused && action.keeps_running() && on_active_space;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Item;

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
