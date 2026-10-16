use std::collections::HashMap;

use objc2::rc::Retained;
use objc2_foundation::NSTimer;

use crate::app::Event;
use crate::engine::image::ImageNode;
use crate::engine::node::Node;
use crate::engine::stills::Still;
use crate::engine::video::VideoNode;
use crate::engine::{Engine, Playback};
use crate::model::{ItemId, Library};
use crate::platform::spaces::SurfaceKey;
use crate::platform::{cancel_timer, schedule_timer};

struct Waiting {
    path: String,
    time: f64,
}

#[derive(Default)]
pub struct Unload {
    pub unloaded: HashMap<ItemId, f64>,
    waiting: HashMap<ItemId, Waiting>,
    timers: HashMap<ItemId, Retained<NSTimer>>,
}

impl Unload {
    pub fn schedule(&mut self, item: ItemId, stop_after: u32) {
        if stop_after == 0 || self.timers.contains_key(&item) {
            return;
        }
        let timer = schedule_timer(f64::from(stop_after), false, move || Event::VideoUnloadTick { item });
        self.timers.insert(item, timer);
    }

    pub fn cancel(&mut self, item: ItemId) {
        cancel_timer(self.timers.remove(&item));
    }

    pub fn forget(&mut self, item: ItemId) {
        self.cancel(item);
        self.waiting.remove(&item);
        self.unloaded.remove(&item);
    }
}

impl Engine {
    pub fn on_unload_tick(&mut self, item: ItemId, library: &Library, playback: &Playback) {
        if self.unload.unloaded.contains_key(&item) {
            return;
        }
        match self.nodes.get(&item) {
            Some(Node::Video(_)) => self.unload_video_tick(item, library, playback.stop_after),
            Some(Node::Gif(_)) => self.unload_gif_tick(item, playback.stop_after),
            _ => {}
        }
    }

    pub(super) fn finish_unload_wait(&mut self, item: ItemId, path: &str) {
        let Some(time) =
            self.unload.waiting.get(&item).filter(|waiting| waiting.path == path).map(|waiting| waiting.time)
        else {
            return;
        };
        self.unload.waiting.remove(&item);
        self.perform_unload(item, path, time);
    }

    pub(super) fn reload_video(&mut self, item: ItemId, time: f64, library: &Library) {
        let Some(entry) = library.items.iter().find(|entry| entry.id == item) else { return };
        let video = VideoNode::new(self.mtm, item, &entry.path);
        let Some(mut old) = self.nodes.insert(item, Node::Video(video)) else {
            return;
        };
        let keys: Vec<SurfaceKey> = old.layers().keys().cloned().collect();
        old.layers_mut().clear();
        for key in &keys {
            self.attach(item, key);
        }
        if let Some(video) = self.nodes.get_mut(&item).and_then(Node::video_mut) {
            video.set_pending_seek(time);
        }
        self.unload.unloaded.remove(&item);
    }

    fn unload_video_tick(&mut self, item: ItemId, library: &Library, stop_after: u32) {
        let Some(video) = self.nodes.get(&item).and_then(Node::video) else { return };
        if video.is_playing() || !should_unload(video.paused_seconds(), stop_after) {
            return;
        }
        let Some(entry) = library.items.iter().find(|entry| entry.id == item) else { return };
        let time = video.current_time();
        match self.stills.ensure(entry, time, library.visibility(entry)) {
            Still::Ready(path) => self.perform_unload(item, &path, time),
            Still::Pending(path) => {
                self.unload.waiting.entry(item).or_insert(Waiting { path, time });
            }
        }
    }

    fn unload_gif_tick(&mut self, item: ItemId, stop_after: u32) {
        let Some(gif) = self.nodes.get_mut(&item).and_then(Node::gif_mut) else { return };
        let bytes = gif.cached_bytes();
        if gif.playing || bytes == 0 || !should_unload(gif.paused_seconds(), stop_after) {
            return;
        }
        self.unload.timers.remove(&item);
        gif.release_frames();
        tracing::info!(%item, bytes, "released paused GIF frames");
    }

    fn perform_unload(&mut self, item: ItemId, still_path: &str, time: f64) {
        let Some(node) = self.nodes.get_mut(&item) else { return };
        let Some(image) = ImageNode::new(still_path) else { return };
        let keys: Vec<SurfaceKey> = node.layers().keys().cloned().collect();
        node.layers_mut().clear();
        self.nodes.insert(item, Node::Image(image));
        for key in &keys {
            self.attach(item, key);
        }
        self.unload.unloaded.insert(item, time);
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
