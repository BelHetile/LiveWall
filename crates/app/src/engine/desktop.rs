use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use objc2::MainThreadMarker;
use objc2_foundation::NSTimer;
use serde::{Deserialize, Serialize};

use crate::app::{Event, post};
use crate::platform::desktop as ffi;
use crate::store;

const SWAP_VERSION: u32 = 1;
const VERIFY_INTERVAL: f64 = 0.1;
const VERIFY_TIMEOUT: Duration = Duration::from_secs(3);
const RESTORE_TIMEOUT: Duration = Duration::from_secs(2);
const RESTORE_POLL: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    PerSpace,
    Screen,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub display: String,
    pub space: String,
    pub display_id: u32,
}

pub fn key(display: &str, space: &str) -> String {
    return format!("{display}|{space}");
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Original {
    pub display: String,
    pub space: String,
    pub display_id: u32,
    pub config: Option<String>,
    pub url: Option<String>,
    pub taken_at: jiff::Timestamp,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SwapState {
    v: u32,
    mode: Mode,
    surfaces: BTreeMap<String, Original>,
}

impl Default for SwapState {
    fn default() -> Self {
        return Self { v: SWAP_VERSION, mode: Mode::PerSpace, surfaces: BTreeMap::new() };
    }
}

pub fn capture_targets(recorded: &BTreeMap<String, Original>, targets: &[Target]) -> Vec<Target> {
    return targets
        .iter()
        .filter(|target| !recorded.contains_key(&key(&target.display, &target.space)))
        .cloned()
        .collect();
}

pub fn apply_targets(
    recorded: &BTreeMap<String, Original>,
    applied: &BTreeMap<String, String>,
    stills: &BTreeMap<String, String>,
    targets: &[Target],
) -> Vec<(Target, String)> {
    let mut plan = Vec::new();
    for target in targets {
        let surface = key(&target.display, &target.space);
        if !recorded.contains_key(&surface) {
            continue;
        }
        let Some(still) = stills.get(&surface) else { continue };
        if applied.get(&surface) == Some(still) {
            continue;
        }
        plan.push((target.clone(), still.clone()));
    }
    return plan;
}

pub fn restore_list(recorded: &BTreeMap<String, Original>) -> Vec<Original> {
    return recorded.values().cloned().collect();
}

#[derive(Clone, Debug)]
struct Pending {
    display_id: u32,
    space: String,
    still: String,
}

pub struct Desktop {
    api: ffi::DesktopApi,
    mode: Mode,
    swap_path: PathBuf,
    recorded: BTreeMap<String, Original>,
    applied: BTreeMap<String, String>,
    pending: BTreeMap<String, Pending>,
    verify_started: Option<Instant>,
    timer: Option<objc2::rc::Retained<NSTimer>>,
    locked: bool,
    last_error: Option<String>,
}

impl Desktop {
    pub fn new(swap_path: PathBuf, mode: Mode) -> Self {
        return Self {
            api: ffi::DesktopApi::load(),
            mode,
            swap_path,
            recorded: BTreeMap::new(),
            applied: BTreeMap::new(),
            pending: BTreeMap::new(),
            verify_started: None,
            timer: None,
            locked: false,
            last_error: None,
        };
    }

    pub fn recover(&mut self, mtm: MainThreadMarker) {
        if !self.swap_path.exists() {
            return;
        }
        let (state, _) = store::load::<SwapState>(&self.swap_path);
        if state.surfaces.is_empty() {
            let _ = std::fs::remove_file(&self.swap_path);
            return;
        }
        tracing::info!(surfaces = state.surfaces.len(), "restoring desktop pictures from a previous run");
        self.mode = state.mode;
        self.recorded = state.surfaces;
        self.restore(mtm);
    }

    pub fn update(
        &mut self,
        mtm: MainThreadMarker,
        targets: &[Target],
        stills: &BTreeMap<String, String>,
        force: bool,
    ) {
        if force {
            self.applied.clear();
        }
        self.capture(mtm, targets);
        self.apply(mtm, targets, stills);
        self.schedule();
    }

    pub fn on_lock(&mut self) {
        self.locked = true;
    }

    pub fn on_unlock(&mut self, mtm: MainThreadMarker, targets: &[Target], stills: &BTreeMap<String, String>) {
        self.locked = false;
        self.update(mtm, targets, stills, true);
    }

    fn capture(&mut self, mtm: MainThreadMarker, targets: &[Target]) {
        let needed = capture_targets(&self.recorded, targets);
        if needed.is_empty() {
            return;
        }
        for target in needed {
            let surface = key(&target.display, &target.space);
            let original = match self.mode {
                Mode::PerSpace => match self.api.copy(target.display_id, &target.space) {
                    Some(config) => Original {
                        display: target.display.clone(),
                        space: target.space.clone(),
                        display_id: target.display_id,
                        url: ffi::image_path(&config),
                        config: ffi::encode(&config),
                        taken_at: jiff::Timestamp::now(),
                    },
                    None => continue,
                },
                Mode::Screen => {
                    let url = ffi::screen_url(mtm, target.display_id);
                    Original {
                        display: target.display.clone(),
                        space: target.space.clone(),
                        display_id: target.display_id,
                        url: url.clone(),
                        config: None,
                        taken_at: jiff::Timestamp::now(),
                    }
                }
            };
            self.recorded.insert(surface, original);
        }
        self.persist();
    }

    fn apply(&mut self, mtm: MainThreadMarker, targets: &[Target], stills: &BTreeMap<String, String>) {
        if self.locked && self.mode == Mode::Screen {
            return;
        }
        for (target, still) in apply_targets(&self.recorded, &self.applied, stills, targets) {
            let surface = key(&target.display, &target.space);
            match self.mode {
                Mode::PerSpace => {
                    let config = ffi::still_config(&still);
                    self.api.set(target.display_id, &config, &target.space);
                }
                Mode::Screen => {
                    if let Err(error) = ffi::screen_set(mtm, target.display_id, &still) {
                        self.last_error = Some(error);
                        continue;
                    }
                }
            }
            self.applied.insert(surface.clone(), still.clone());
            self.pending.insert(surface, Pending { display_id: target.display_id, space: target.space.clone(), still });
        }
    }

    fn schedule(&mut self) {
        if self.pending.is_empty() || self.timer.is_some() {
            return;
        }
        self.verify_started = Some(Instant::now());
        let block = block2::RcBlock::new(|_timer: std::ptr::NonNull<NSTimer>| {
            post(Event::DesktopVerify);
        });
        let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(VERIFY_INTERVAL, true, &block) };
        self.timer = Some(timer);
    }

    pub fn verify_tick(&mut self, mtm: MainThreadMarker) {
        let mut confirmed = Vec::new();
        for (surface, pending) in &self.pending {
            if self.read_back(mtm, pending).as_deref() == Some(pending.still.as_str()) {
                confirmed.push(surface.clone());
            }
        }
        for surface in confirmed {
            self.pending.remove(&surface);
        }
        let timed_out = self.verify_started.is_some_and(|started| started.elapsed() >= VERIFY_TIMEOUT);
        if self.pending.is_empty() {
            tracing::info!("desktop picture takeover confirmed");
            self.stop_verify();
        } else if timed_out {
            tracing::warn!(pending = self.pending.len(), "desktop picture takeover unconfirmed");
            self.stop_verify();
        }
    }

    fn stop_verify(&mut self) {
        if let Some(timer) = self.timer.take() {
            timer.invalidate();
        }
        self.verify_started = None;
        self.pending.clear();
    }

    fn read_back(&self, mtm: MainThreadMarker, pending: &Pending) -> Option<String> {
        return match self.mode {
            Mode::PerSpace => {
                self.api.copy(pending.display_id, &pending.space).and_then(|config| ffi::image_path(&config))
            }
            Mode::Screen => ffi::screen_url(mtm, pending.display_id),
        };
    }

    pub fn restore(&mut self, mtm: MainThreadMarker) {
        self.stop_verify();
        let originals = restore_list(&self.recorded);
        for original in &originals {
            match self.mode {
                Mode::PerSpace => {
                    let Some(encoded) = &original.config else { continue };
                    let Some(config) = ffi::decode(encoded) else { continue };
                    if ffi::is_empty(&config) {
                        tracing::warn!(space = %original.space, "skipping restore of an empty desktop picture config");
                        continue;
                    }
                    self.api.set(original.display_id, &config, &original.space);
                }
                Mode::Screen => {
                    if let Some(url) = &original.url {
                        let _ = ffi::screen_set(mtm, original.display_id, url);
                    }
                }
            }
        }
        self.await_restore(mtm, &originals);
        if let Err(error) = std::fs::remove_file(&self.swap_path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(%error, "cannot remove swap state");
        }
        self.recorded.clear();
        self.applied.clear();
    }

    fn await_restore(&self, mtm: MainThreadMarker, originals: &[Original]) {
        let deadline = Instant::now() + RESTORE_TIMEOUT;
        loop {
            let confirmed = originals.iter().all(|original| self.restored(mtm, original));
            if confirmed || Instant::now() >= deadline {
                return;
            }
            std::thread::sleep(RESTORE_POLL);
        }
    }

    fn restored(&self, mtm: MainThreadMarker, original: &Original) -> bool {
        let Some(url) = &original.url else { return true };
        return match self.mode {
            Mode::PerSpace => self
                .api
                .copy(original.display_id, &original.space)
                .and_then(|config| ffi::image_path(&config))
                .is_some_and(|value| value.as_str() == url.as_str()),
            Mode::Screen => {
                ffi::screen_url(mtm, original.display_id).is_some_and(|value| value.as_str() == url.as_str())
            }
        };
    }

    fn persist(&self) {
        let state = SwapState { v: SWAP_VERSION, mode: self.mode, surfaces: self.recorded.clone() };
        if let Err(error) = store::save(&self.swap_path, &state) {
            tracing::warn!(%error, "cannot save swap state");
        }
    }

    pub fn report(&self, mtm: MainThreadMarker, targets: &[Target]) -> DesktopReport {
        let mut surfaces = Vec::new();
        for target in targets {
            let surface = key(&target.display, &target.space);
            let original = self.recorded.get(&surface);
            let current = match self.mode {
                Mode::PerSpace => {
                    self.api.copy(target.display_id, &target.space).and_then(|config| ffi::image_path(&config))
                }
                Mode::Screen => ffi::screen_url(mtm, target.display_id),
            };
            let owned = self.applied.contains_key(&surface);
            surfaces.push(SurfaceStatus {
                display: target.display.clone(),
                space: target.space.clone(),
                captured: original.is_some(),
                original_url: original.and_then(|value| value.url.clone()),
                taken_at: original.map(|value| value.taken_at),
                current,
                state: if owned { SurfaceState::Owned } else { SurfaceState::Idle },
            });
        }
        return DesktopReport {
            mode: self.mode,
            per_space_available: self.api.per_space(),
            locked: self.locked,
            error: self.last_error.clone(),
            surfaces,
        };
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SurfaceState {
    Idle,
    Owned,
}

#[derive(Debug, Serialize)]
pub struct SurfaceStatus {
    pub display: String,
    pub space: String,
    pub captured: bool,
    pub original_url: Option<String>,
    pub taken_at: Option<jiff::Timestamp>,
    pub current: Option<String>,
    pub state: SurfaceState,
}

#[derive(Debug, Serialize)]
pub struct DesktopReport {
    pub mode: Mode,
    pub per_space_available: bool,
    pub locked: bool,
    pub error: Option<String>,
    pub surfaces: Vec<SurfaceStatus>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(display: &str, space: &str) -> Target {
        return Target { display: display.into(), space: space.into(), display_id: 1 };
    }

    fn original(display: &str, space: &str) -> Original {
        return Original {
            display: display.into(),
            space: space.into(),
            display_id: 1,
            config: Some("x".into()),
            url: Some("/a.jpg".into()),
            taken_at: jiff::Timestamp::UNIX_EPOCH,
        };
    }

    #[test]
    fn capture_only_targets_not_already_recorded() {
        let recorded = BTreeMap::from([(key("D", "S1"), original("D", "S1"))]);
        let targets = [target("D", "S1"), target("D", "S2")];
        let needed = capture_targets(&recorded, &targets);
        assert_eq!(needed, vec![target("D", "S2")]);
    }

    #[test]
    fn apply_skips_unrecorded_and_unchanged() {
        let recorded = BTreeMap::from([(key("D", "S1"), original("D", "S1")), (key("D", "S2"), original("D", "S2"))]);
        let applied = BTreeMap::from([(key("D", "S1"), "/still1.png".to_string())]);
        let stills = BTreeMap::from([
            (key("D", "S1"), "/still1.png".to_string()),
            (key("D", "S2"), "/still2.png".to_string()),
            (key("D", "S3"), "/still3.png".to_string()),
        ]);
        let targets = [target("D", "S1"), target("D", "S2"), target("D", "S3")];
        let plan = apply_targets(&recorded, &applied, &stills, &targets);
        assert_eq!(plan, vec![(target("D", "S2"), "/still2.png".to_string())]);
    }

    #[test]
    fn apply_retries_when_the_still_changes() {
        let recorded = BTreeMap::from([(key("D", "S1"), original("D", "S1"))]);
        let applied = BTreeMap::from([(key("D", "S1"), "/old.png".to_string())]);
        let stills = BTreeMap::from([(key("D", "S1"), "/new.png".to_string())]);
        let plan = apply_targets(&recorded, &applied, &stills, &[target("D", "S1")]);
        assert_eq!(plan, vec![(target("D", "S1"), "/new.png".to_string())]);
    }

    #[test]
    fn restore_list_covers_every_recorded_surface() {
        let recorded = BTreeMap::from([(key("D", "S1"), original("D", "S1")), (key("D", "S2"), original("D", "S2"))]);
        let mut restored: Vec<String> = restore_list(&recorded).into_iter().map(|value| value.space).collect();
        restored.sort();
        assert_eq!(restored, vec!["S1".to_string(), "S2".to_string()]);
    }

    #[test]
    fn key_is_display_space_pair() {
        assert_eq!(key("D", "S"), "D|S");
    }
}
