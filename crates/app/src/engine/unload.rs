use std::collections::HashMap;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2_foundation::NSTimer;

use crate::app::Event;
use crate::engine::image::ImageNode;
use crate::engine::layer::without_actions;
use crate::engine::node::Node;
use crate::engine::stills::Still;
use crate::engine::video::VideoNode;
use crate::engine::{Engine, Playback};
use crate::model::{ItemId, Library};
use crate::platform::spaces::SurfaceKey;
use crate::platform::{cancel_timer, schedule_timer};

const COVER_POLL: f64 = 0.05;
const COVER_LIMIT: Duration = Duration::from_secs(3);

struct Waiting {
    path: String,
    time: f64,
}

pub struct Unloaded {
    pub time: f64,
    still: String,
}

#[derive(Default)]
pub struct Unload {
    pub unloaded: HashMap<ItemId, Unloaded>,
    waiting: HashMap<ItemId, Waiting>,
    timers: HashMap<ItemId, Retained<NSTimer>>,
    covers: HashMap<ItemId, (Node, Instant)>,
}

impl Unload {
    pub fn schedule(&mut self, item: ItemId, stop_after: u32) {
        if stop_after == 0 {
            return;
        }
        self.schedule_in(item, f64::from(stop_after));
    }

    fn schedule_in(&mut self, item: ItemId, seconds: f64) {
        if self.timers.contains_key(&item) {
            return;
        }
        let timer = schedule_timer(seconds, false, move || Event::VideoUnloadTick { item });
        self.timers.insert(item, timer);
    }

    fn reschedule(&mut self, item: ItemId, paused_seconds: f64, stop_after: u32) {
        self.schedule_in(item, (f64::from(stop_after) - paused_seconds).ceil().max(1.0));
    }

    pub fn cancel(&mut self, item: ItemId) {
        cancel_timer(self.timers.remove(&item));
        self.waiting.remove(&item);
    }

    pub fn forget(&mut self, item: ItemId) {
        self.cancel(item);
        self.waiting.remove(&item);
        self.unloaded.remove(&item);
        self.uncover(item);
    }

    pub fn uncover(&mut self, item: ItemId) {
        if let Some((mut cover, _)) = self.covers.remove(&item) {
            cover.layers_mut().clear();
        }
    }

    pub fn stills(&self) -> impl Iterator<Item = &str> {
        return self
            .waiting
            .values()
            .map(|waiting| waiting.path.as_str())
            .chain(self.unloaded.values().map(|unloaded| unloaded.still.as_str()));
    }
}

impl Engine {
    pub fn check_cover(&mut self, item: ItemId) {
        let Some((_, since)) = self.unload.covers.get(&item) else { return };
        let drawn = self.nodes.get(&item).and_then(Node::video).is_none_or(VideoNode::shows_frame);
        if drawn || since.elapsed() >= COVER_LIMIT {
            self.unload.uncover(item);
            return;
        }
        schedule_timer(COVER_POLL, false, move || Event::CoverTick { item });
    }

    pub fn on_unload_tick(&mut self, item: ItemId, library: &Library, playback: &Playback) {
        self.unload.timers.remove(&item);
        if self.unload.unloaded.contains_key(&item) {
            return;
        }
        match self.nodes.get(&item) {
            Some(Node::Video(_)) => self.unload_video_tick(item, library, playback.stop_after),
            Some(Node::Gif(_)) => self.unload_gif_tick(item, playback.stop_after),
            _ => {}
        }
    }

    pub(super) fn finish_unload_wait(&mut self, item: ItemId, path: &str, library: &Library) {
        let Some(time) =
            self.unload.waiting.get(&item).filter(|waiting| waiting.path == path).map(|waiting| waiting.time)
        else {
            return;
        };
        self.unload.waiting.remove(&item);
        self.perform_unload(item, path, time, library);
    }

    pub(super) fn reload_video(&mut self, item: ItemId, time: f64, library: &Library) {
        let Some(entry) = library.items.iter().find(|entry| entry.id == item) else { return };
        let video = VideoNode::new(self.mtm, item, &entry.path);
        let Some(old) = self.nodes.insert(item, Node::Video(video)) else {
            return;
        };
        let keys: Vec<SurfaceKey> = old.layers().keys().cloned().collect();
        self.unload.uncover(item);
        self.unload.covers.insert(item, (old, Instant::now()));
        for key in &keys {
            self.attach(item, key);
        }
        if let Some(video) = self.nodes.get_mut(&item).and_then(Node::video_mut) {
            video.set_pending_seek(time);
        }
        self.unload.unloaded.remove(&item);
        self.check_cover(item);
    }

    fn unload_video_tick(&mut self, item: ItemId, library: &Library, stop_after: u32) {
        let Some(video) = self.nodes.get(&item).and_then(Node::video) else { return };
        if video.is_playing() {
            return;
        }
        let paused = video.paused_seconds();
        if !should_unload(paused, stop_after) {
            self.unload.reschedule(item, paused, stop_after);
            return;
        }
        let Some(entry) = library.items.iter().find(|entry| entry.id == item) else { return };
        let time = video.current_time();
        match self.stills.ensure(entry, time, library.visibility(entry)) {
            Still::Ready(path) => self.perform_unload(item, &path, time, library),
            Still::Pending(path) => {
                self.unload.waiting.entry(item).or_insert(Waiting { path, time });
            }
        }
    }

    fn unload_gif_tick(&mut self, item: ItemId, stop_after: u32) {
        let Some(gif) = self.nodes.get_mut(&item).and_then(Node::gif_mut) else { return };
        let bytes = gif.cached_bytes();
        if gif.playing || bytes == 0 {
            return;
        }
        let paused = gif.paused_seconds();
        if !should_unload(paused, stop_after) {
            self.unload.reschedule(item, paused, stop_after);
            return;
        }
        gif.release_frames();
        tracing::info!(%item, bytes, "released paused GIF frames");
    }

    fn perform_unload(&mut self, item: ItemId, still_path: &str, time: f64, library: &Library) {
        let Some(node) = self.nodes.get(&item) else { return };
        let Some(image) = ImageNode::new(still_path) else { return };
        let keys: Vec<SurfaceKey> = node.layers().keys().cloned().collect();
        without_actions(|| {
            let Some(mut old) = self.nodes.insert(item, Node::Image(image)) else { return };
            for key in &keys {
                self.attach(item, key);
            }
            self.apply_frames(library);
            old.layers_mut().clear();
        });
        self.unload.unloaded.insert(item, Unloaded { time, still: still_path.to_string() });
        tracing::info!(%item, "unloaded paused video to a still frame");
    }
}

fn should_unload(paused_seconds: f64, stop_after: u32) -> bool {
    return stop_after > 0 && paused_seconds >= f64::from(stop_after);
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
