use std::time::Instant;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_av_foundation::{
    AVAsynchronousKeyValueLoading, AVLayerVideoGravityResize, AVPlayerItem, AVPlayerLayer, AVPlayerLooper,
    AVQueuePlayer, AVURLAsset,
};
use objc2_core_media::{CMTime, kCMTimeZero};
use objc2_foundation::{NSArray, NSString};
use objc2_quartz_core::CALayer;

use crate::app::{Event, post};
use crate::cf::file_url;
use crate::engine::layer::LayerSet;
use crate::geometry::Size;
use crate::media::av::CM_TIMESCALE;
use crate::model::ItemId;
use crate::platform::spaces::SurfaceKey;

pub struct VideoNode {
    pub item: ItemId,
    pub size: Size,
    pub loaded: bool,
    pub layers: LayerSet,
    playing: bool,
    paused_since: Option<Instant>,
    speed: f32,
    pending_seek: Option<f64>,
    player: Retained<AVQueuePlayer>,
    asset: Retained<AVURLAsset>,
    looper: Option<Retained<AVPlayerLooper>>,
}

impl VideoNode {
    pub fn new(mtm: MainThreadMarker, item: ItemId, path: &str) -> Self {
        let url = file_url(path);
        let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) };
        let player = unsafe { AVQueuePlayer::new(mtm) };
        unsafe {
            player.setAutomaticallyWaitsToMinimizeStalling(false);
            player.setRate(0.0);
        }
        let node = Self {
            item,
            size: Size::new(0.0, 0.0),
            loaded: false,
            layers: LayerSet::default(),
            playing: false,
            paused_since: None,
            speed: 1.0,
            pending_seek: None,
            player,
            asset,
            looper: None,
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
        self.size = video_size(&self.asset);
        let player_item = unsafe { AVPlayerItem::playerItemWithAsset(&self.asset, mtm) };
        let looper = unsafe { AVPlayerLooper::playerLooperWithPlayer_templateItem(&self.player, &player_item) };
        self.looper = Some(looper);
        self.loaded = true;
        if let Some(seconds) = self.pending_seek.take() {
            unsafe { self.player.seekToTime(CMTime::with_seconds(seconds.max(0.0), CM_TIMESCALE)) };
        }
    }

    pub fn attach(&mut self, key: &SurfaceKey, root: &CALayer) {
        let layer = unsafe { AVPlayerLayer::playerLayerWithPlayer(Some(&self.player)) };
        if let Some(gravity) = unsafe { AVLayerVideoGravityResize } {
            unsafe { layer.setVideoGravity(gravity) };
        }
        self.layers.add(key, layer.into_super(), root);
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

    pub fn nudge(&mut self) {
        let time = self.current_time();
        let zero = unsafe { kCMTimeZero };
        unsafe {
            self.player.seekToTime_toleranceBefore_toleranceAfter(
                CMTime::with_seconds(time.max(0.0), CM_TIMESCALE),
                zero,
                zero,
            );
            if self.playing {
                self.player.play();
                self.player.setRate(self.speed);
            }
        }
    }

    pub fn rebuild(&mut self, mtm: MainThreadMarker) {
        let time = self.current_time();
        if let Some(looper) = self.looper.take() {
            unsafe { looper.disableLooping() };
        }
        let player = unsafe { AVQueuePlayer::new(mtm) };
        unsafe {
            self.player.pause();
            self.player.removeAllItems();
            player.setAutomaticallyWaitsToMinimizeStalling(false);
            player.setRate(0.0);
            player.setMuted(self.player.isMuted());
            player.setVolume(self.player.volume());
        }
        for layer in self.layers.iter().filter_map(|layer| layer.downcast_ref::<AVPlayerLayer>()) {
            unsafe { layer.setPlayer(Some(&player)) };
        }
        self.player = player;
        let player_item = unsafe { AVPlayerItem::playerItemWithAsset(&self.asset, mtm) };
        self.looper = Some(unsafe { AVPlayerLooper::playerLooperWithPlayer_templateItem(&self.player, &player_item) });
        unsafe {
            self.player.seekToTime(CMTime::with_seconds(time.max(0.0), CM_TIMESCALE));
            if self.playing {
                self.player.play();
                self.player.setRate(self.speed);
            }
        }
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
}

#[allow(deprecated)]
fn video_size(asset: &AVURLAsset) -> Size {
    let Some(media_type) = (unsafe { objc2_av_foundation::AVMediaTypeVideo }) else {
        return Size::new(0.0, 0.0);
    };
    let tracks = unsafe { asset.tracksWithMediaType(media_type) };
    let Some(track) = tracks.firstObject() else {
        return Size::new(0.0, 0.0);
    };
    let size = unsafe { track.naturalSize() };
    let transform = unsafe { track.preferredTransform() };
    let rotated = transform.b.abs() > 0.5 || transform.c.abs() > 0.5;
    if rotated {
        return Size::new(size.height, size.width);
    }
    return Size::new(size.width, size.height);
}
