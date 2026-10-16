use std::time::Instant;

use objc2::rc::Retained;
use objc2_core_foundation::{CFDictionary, CFRetained, CFString, CFType};
use objc2_core_graphics::CGImage;
use objc2_foundation::NSTimer;
use objc2_image_io::{
    CGImageSource, kCGImagePropertyGIFDelayTime, kCGImagePropertyGIFDictionary, kCGImagePropertyGIFUnclampedDelayTime,
};
use objc2_quartz_core::{CALayer, kCAGravityResize};

use crate::app::Event;
use crate::cf::{cf_f64, cf_file_url, downcast_dict};
use crate::engine::layer::{LayerSet, set_image};
use crate::geometry::Size;
use crate::media::av::thumbnail_options;
use crate::media::plan::{GIF_DEFAULT_DELAY, GIF_MIN_DELAY, GIF_TINY_DELAY};
use crate::model::ItemId;
use crate::platform::spaces::SurfaceKey;
use crate::platform::{cancel_timer, schedule_timer};

const FRAME_CACHE_BUDGET: usize = 512 * 1024 * 1024;

pub struct GifNode {
    pub item: ItemId,
    pub size: Size,
    pub playing: bool,
    pub layers: LayerSet,
    source: CFRetained<CGImageSource>,
    delays: Vec<f64>,
    frames: Vec<Option<CFRetained<CGImage>>>,
    frame_bytes: Vec<usize>,
    total_bytes: usize,
    budget: usize,
    max_px: usize,
    index: usize,
    paused_since: Option<Instant>,
    timer: Option<Retained<NSTimer>>,
}

impl GifNode {
    pub fn new(item: ItemId, path: &str, max_px: usize) -> Option<Self> {
        let url = cf_file_url(path)?;
        let source = unsafe { CGImageSource::with_url(&url, None) }?;
        let delays = frame_delays(&source);
        let count = delays.len();
        return Some(Self {
            item,
            size: Size::new(0.0, 0.0),
            playing: false,
            layers: LayerSet::default(),
            source,
            delays,
            frames: (0..count).map(|_| None).collect(),
            frame_bytes: vec![0; count],
            total_bytes: 0,
            budget: FRAME_CACHE_BUDGET,
            max_px,
            index: 0,
            paused_since: None,
            timer: None,
        });
    }

    pub fn attach(&mut self, key: &SurfaceKey, root: &CALayer) {
        let layer = CALayer::layer();
        layer.setContentsGravity(unsafe { kCAGravityResize });
        if let Some(image) = self.frame_image(self.index) {
            set_image(&layer, &image);
        }
        self.layers.add(key, layer, root);
    }

    pub fn resume(&mut self) {
        self.paused_since = None;
        if self.playing {
            return;
        }
        self.playing = true;
        self.schedule();
    }

    pub fn pause(&mut self) {
        if !self.playing {
            return;
        }
        self.playing = false;
        self.paused_since = Some(Instant::now());
        cancel_timer(self.timer.take());
    }

    pub fn paused_seconds(&self) -> f64 {
        return self.paused_since.map_or(0.0, |since| since.elapsed().as_secs_f64());
    }

    pub fn cached_bytes(&self) -> usize {
        return self.total_bytes;
    }

    pub fn release_frames(&mut self) {
        self.frames.fill(None);
        self.frame_bytes.fill(0);
        self.total_bytes = 0;
    }

    pub fn tick(&mut self) {
        if !self.playing {
            return;
        }
        let count = self.delays.len();
        if count == 0 {
            return;
        }
        self.index = (self.index + 1) % count;
        self.show_current();
        self.schedule();
    }

    fn schedule(&mut self) {
        let delay = self.delays.get(self.index).copied().unwrap_or(GIF_DEFAULT_DELAY);
        let item = self.item;
        self.timer = Some(schedule_timer(delay, false, move || Event::GifTick { item }));
    }

    fn show_current(&mut self) {
        let Some(image) = self.frame_image(self.index) else {
            return;
        };
        self.layers.show(&image);
    }

    fn frame_image(&mut self, index: usize) -> Option<CFRetained<CGImage>> {
        if let Some(Some(cached)) = self.frames.get(index) {
            return Some(cached.clone());
        }
        let options = thumbnail_options(self.max_px);
        let options_ref: &CFDictionary = unsafe { (*options).cast_unchecked() };
        let image = unsafe { self.source.thumbnail_at_index(index, Some(options_ref)) }?;
        let width = u32::try_from(CGImage::width(Some(&image))).map_or(0.0, f64::from);
        let height = u32::try_from(CGImage::height(Some(&image))).map_or(0.0, f64::from);
        if width > 0.0 && height > 0.0 {
            self.size = Size::new(width, height);
        }
        let bytes = image_bytes(&image);
        if let Some(slot) = self.frames.get_mut(index) {
            *slot = Some(image.clone());
        }
        if let Some(slot) = self.frame_bytes.get_mut(index) {
            *slot = bytes;
        }
        self.total_bytes = self.total_bytes.saturating_add(bytes);
        self.evict();
        return Some(image);
    }

    fn evict(&mut self) {
        for index in frames_to_evict(&self.frame_bytes, self.index, self.total_bytes, self.budget) {
            if let Some(slot) = self.frames.get_mut(index)
                && slot.take().is_some()
            {
                let freed = self.frame_bytes.get(index).copied().unwrap_or(0);
                self.total_bytes = self.total_bytes.saturating_sub(freed);
                if let Some(bytes) = self.frame_bytes.get_mut(index) {
                    *bytes = 0;
                }
            }
        }
    }
}

pub fn delay_seconds(unclamped: Option<f64>, delay: Option<f64>) -> f64 {
    let Some(raw) = unclamped.filter(|value| *value > 0.0).or(delay.filter(|value| *value > 0.0)) else {
        return GIF_DEFAULT_DELAY;
    };
    if raw < GIF_TINY_DELAY {
        return GIF_DEFAULT_DELAY;
    }
    return raw.max(GIF_MIN_DELAY);
}

pub fn frames_to_evict(frame_bytes: &[usize], current: usize, total_bytes: usize, budget: usize) -> Vec<usize> {
    if total_bytes <= budget {
        return Vec::new();
    }
    let mut order: Vec<usize> =
        (0..frame_bytes.len()).filter(|index| frame_bytes[*index] > 0 && *index != current).collect();
    let distance = |index: usize| -> usize {
        return index.abs_diff(current);
    };
    order.sort_by_key(|index| std::cmp::Reverse(distance(*index)));
    let mut remaining = total_bytes;
    let mut evicted = Vec::new();
    for index in order {
        if remaining <= budget {
            break;
        }
        remaining = remaining.saturating_sub(frame_bytes[index]);
        evicted.push(index);
    }
    return evicted;
}

fn frame_delays(source: &CGImageSource) -> Vec<f64> {
    let count = unsafe { source.count() };
    let mut delays = Vec::with_capacity(count);
    for index in 0..count {
        let properties = unsafe { source.properties_at_index(index, None) };
        let raw = properties.and_then(|properties| {
            let properties: &CFDictionary<CFString, CFType> = unsafe { properties.cast_unchecked() };
            let gif = properties.get(unsafe { kCGImagePropertyGIFDictionary })?;
            let gif = downcast_dict(Some(&gif))?;
            let unclamped = cf_f64(gif.get(unsafe { kCGImagePropertyGIFUnclampedDelayTime }).as_deref());
            let delay = cf_f64(gif.get(unsafe { kCGImagePropertyGIFDelayTime }).as_deref());
            return Some((unclamped, delay));
        });
        let (unclamped, delay) = raw.unwrap_or((None, None));
        delays.push(delay_seconds(unclamped, delay));
    }
    return delays;
}

fn image_bytes(image: &CGImage) -> usize {
    let width = CGImage::width(Some(image));
    let height = CGImage::height(Some(image));
    return width.saturating_mul(height).saturating_mul(4);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        return (a - b).abs() < 1e-9;
    }

    #[test]
    fn delay_prefers_unclamped() {
        assert!(approx(delay_seconds(Some(0.5), Some(0.1)), 0.5));
    }

    #[test]
    fn delay_falls_back_to_clamped() {
        assert!(approx(delay_seconds(None, Some(0.25)), 0.25));
    }

    #[test]
    fn delay_treats_tiny_values_as_a_tenth() {
        assert!(approx(delay_seconds(Some(0.005), None), 0.1));
        assert!(approx(delay_seconds(Some(0.0), Some(0.008)), 0.1));
    }

    #[test]
    fn delay_has_a_floor() {
        assert!(approx(delay_seconds(Some(0.011), None), 0.02));
    }

    #[test]
    fn delay_defaults_when_absent() {
        assert!(approx(delay_seconds(None, None), 0.1));
    }

    #[test]
    fn eviction_is_a_noop_within_budget() {
        assert!(frames_to_evict(&[10, 10, 10], 0, 30, 100).is_empty());
    }

    #[test]
    fn eviction_drops_the_farthest_frames_first() {
        let evicted = frames_to_evict(&[10, 10, 10, 10], 0, 40, 25);
        assert_eq!(evicted, vec![3, 2]);
    }

    #[test]
    fn eviction_keeps_the_current_frame() {
        let evicted = frames_to_evict(&[10, 10, 10], 1, 30, 0);
        assert!(!evicted.contains(&1));
    }
}
