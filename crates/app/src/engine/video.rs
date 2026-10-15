use std::time::Instant;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_av_foundation::{
    AVAsynchronousKeyValueLoading, AVLayerVideoGravityResize, AVPlayerItem, AVPlayerLayer, AVPlayerLooper,
    AVQueuePlayer, AVURLAsset,
};
use objc2_core_foundation::CGRect;
use objc2_core_media::CMTime;
use objc2_foundation::{NSArray, NSString, NSURL};
use objc2_quartz_core::CALayer;

use crate::app::{Event, post};
use crate::model::ItemId;
use crate::platform::spaces::SurfaceKey;

pub struct VideoNode {
    pub item: ItemId,
    pub size: (f64, f64),
    pub loaded: bool,
    pub duration: f64,
    playing: bool,
    paused_since: Option<Instant>,
    speed: f32,
    pending_seek: Option<f64>,
    player: Retained<AVQueuePlayer>,
    asset: Retained<AVURLAsset>,
    looper: Option<Retained<AVPlayerLooper>>,
    layers: Vec<(SurfaceKey, Retained<AVPlayerLayer>)>,
}

impl VideoNode {
    pub fn new(mtm: MainThreadMarker, item: ItemId, path: &str) -> Self {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) };
        let player = unsafe { AVQueuePlayer::new(mtm) };
        unsafe {
            player.setAutomaticallyWaitsToMinimizeStalling(false);
            player.setRate(0.0);
        }
        let node = Self {
            item,
            size: (0.0, 0.0),
            loaded: false,
            duration: 0.0,
            playing: false,
            paused_since: None,
            speed: 1.0,
            pending_seek: None,
            player,
            asset,
            looper: None,
            layers: Vec::new(),
        };
        node.load();
        return node;
    }

    fn load(&self) {
        let item = self.item;
        let keys = NSArray::from_retained_slice(&[NSString::from_str("duration"), NSString::from_str("tracks")]);
        let block = block2::RcBlock::new(move || {
            post(Event::VideoLoaded { item });
        });
        unsafe { self.asset.loadValuesAsynchronouslyForKeys_completionHandler(&keys, Some(&block)) };
    }

    pub fn on_loaded(&mut self, mtm: MainThreadMarker) {
        self.duration = unsafe { self.asset.duration().seconds() };
        self.size = video_size(&self.asset);
        let player_item = unsafe { AVPlayerItem::playerItemWithAsset(&self.asset, mtm) };
        let looper = unsafe { AVPlayerLooper::playerLooperWithPlayer_templateItem(&self.player, &player_item) };
        self.looper = Some(looper);
        self.loaded = true;
        if let Some(seconds) = self.pending_seek.take() {
            unsafe { self.player.seekToTime(CMTime::with_seconds(seconds.max(0.0), 600)) };
        }
    }

    pub fn attach(&mut self, key: &SurfaceKey, root: &CALayer) {
        let layer = unsafe { AVPlayerLayer::playerLayerWithPlayer(Some(&self.player)) };
        if let Some(gravity) = unsafe { AVLayerVideoGravityResize } {
            unsafe { layer.setVideoGravity(gravity) };
        }
        root.addSublayer(&layer);
        self.layers.push((key.clone(), layer));
    }

    pub fn detach(&mut self, key: &SurfaceKey) {
        if let Some(index) = self.layers.iter().position(|(existing, _)| existing == key) {
            let (_, layer) = self.layers.remove(index);
            layer.removeFromSuperlayer();
        }
    }

    pub fn set_frame(&self, key: &SurfaceKey, frame: CGRect) {
        for (existing, layer) in &self.layers {
            if existing == key {
                layer.setFrame(frame);
            }
        }
    }

    pub fn apply_playback(&mut self, playing: bool, muted: bool, volume: f32, speed: f32) {
        self.speed = speed;
        unsafe {
            self.player.setMuted(muted);
            self.player.setVolume(volume.clamp(0.0, 1.0));
        }
        self.set_playing(playing);
    }

    fn set_playing(&mut self, playing: bool) {
        if playing == self.playing {
            return;
        }
        self.playing = playing;
        if playing {
            self.paused_since = None;
            unsafe {
                self.player.play();
                self.player.setRate(self.speed);
            }
        } else {
            self.paused_since = Some(Instant::now());
            unsafe { self.player.pause() };
        }
    }

    pub fn is_playing(&self) -> bool {
        return self.playing;
    }

    pub fn paused_seconds(&self) -> f64 {
        return self.paused_since.map_or(0.0, |since| since.elapsed().as_secs_f64());
    }

    pub fn current_time(&self) -> f64 {
        return unsafe { self.player.currentTime().seconds() };
    }

    pub fn set_pending_seek(&mut self, seconds: f64) {
        self.pending_seek = Some(seconds);
    }

    pub fn rate(&self) -> f32 {
        return unsafe { self.player.rate() };
    }

    pub fn is_ready(&self) -> bool {
        return self.loaded && self.looper.is_some();
    }

    pub fn layer_count(&self) -> usize {
        return self.layers.len();
    }

    pub fn layer(&self, key: &SurfaceKey) -> Option<Retained<CALayer>> {
        return self.layers.iter().find(|(existing, _)| existing == key).map(|(_, layer)| layer.clone().into_super());
    }

    pub fn keys(&self) -> Vec<SurfaceKey> {
        return self.layers.iter().map(|(key, _)| key.clone()).collect();
    }
}

#[allow(deprecated)]
fn video_size(asset: &AVURLAsset) -> (f64, f64) {
    let Some(media_type) = (unsafe { objc2_av_foundation::AVMediaTypeVideo }) else {
        return (0.0, 0.0);
    };
    let tracks = unsafe { asset.tracksWithMediaType(media_type) };
    let Some(track) = tracks.firstObject() else {
        return (0.0, 0.0);
    };
    let size = unsafe { track.naturalSize() };
    let transform = unsafe { track.preferredTransform() };
    let rotated = transform.b.abs() > 0.5 || transform.c.abs() > 0.5;
    let (width, height) = if rotated { (size.height, size.width) } else { (size.width, size.height) };
    return (width, height);
}
