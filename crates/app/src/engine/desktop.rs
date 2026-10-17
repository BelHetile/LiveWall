use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use objc2::MainThreadMarker;
use objc2_foundation::NSTimer;
use serde::{Deserialize, Serialize};

use crate::app::Event;
use crate::platform::desktop as ffi;
use crate::platform::{cancel_timer, schedule_timer};
use crate::store;

const SWAP_VERSION: u32 = 1;
const VERIFY_INTERVAL: f64 = 0.1;
const VERIFY_TIMEOUT: Duration = Duration::from_secs(3);
const RESTORE_ATTEMPTS: usize = 25;
const RESTORE_WAIT: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    PerSpace,
    Screen,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub space: String,
    pub display_id: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Original {
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
    return targets.iter().filter(|target| !recorded.contains_key(&target.space)).cloned().collect();
}

pub fn apply_targets(
    recorded: &BTreeMap<String, Original>,
    applied: &BTreeMap<String, String>,
    stills: &BTreeMap<String, String>,
    targets: &[Target],
) -> Vec<(Target, String)> {
    let mut plan = Vec::new();
    for target in targets {
        let surface = target.space.clone();
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

pub fn is_owned(original: &Original, owned: &[PathBuf]) -> bool {
    return original.url.as_deref().is_some_and(|url| owned.iter().any(|dir| Path::new(url).starts_with(dir)));
}

pub fn is_dangling(original: &Original) -> bool {
    return original.url.as_deref().is_some_and(|url| !Path::new(url).exists());
}

pub fn adopt_foreign(original: Original, recorded: &BTreeMap<String, Original>, owned: &[PathBuf]) -> Original {
    if !is_owned(&original, owned) {
        return original;
    }
    let donor = recorded
        .values()
        .filter(|candidate| !is_owned(candidate, owned))
        .min_by_key(|candidate| candidate.display_id != original.display_id);
    let Some(donor) = donor else {
        tracing::warn!(space = %original.space, "desktop picture is a LiveWall still and no other original is known");
        return original;
    };
    tracing::info!(space = %original.space, "desktop picture was a leftover LiveWall still; adopting another original");
    return Original { config: donor.config.clone(), url: donor.url.clone(), ..original };
}

pub fn is_user_wallpaper(original: &Original, owned: &[PathBuf]) -> bool {
    return original.url.is_some() && !is_owned(original, owned);
}

pub fn shows_still(current: Option<&str>, owned: &[PathBuf]) -> bool {
    return current.is_some_and(|url| owned.iter().any(|dir| Path::new(url).starts_with(dir)));
}

pub fn restore_source(
    original: &Original,
    recorded: &BTreeMap<String, Original>,
    owned: &[PathBuf],
) -> Option<Original> {
    if is_user_wallpaper(original, owned) {
        return Some(original.clone());
    }
    let donor = recorded
        .values()
        .filter(|candidate| is_user_wallpaper(candidate, owned))
        .min_by_key(|candidate| candidate.display_id != original.display_id)?;
    return Some(Original { config: donor.config.clone(), url: donor.url.clone(), ..original.clone() });
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
    owned: Vec<PathBuf>,
    recorded: BTreeMap<String, Original>,
    applied: BTreeMap<String, String>,
    pending: BTreeMap<String, Pending>,
    deferred: BTreeMap<String, (Target, String)>,
    verify_started: Option<Instant>,
    timer: Option<objc2::rc::Retained<NSTimer>>,
    locked: bool,
    last_error: Option<String>,
}

impl Desktop {
    pub fn new(swap_path: PathBuf, mode: Mode, owned: Vec<PathBuf>) -> Self {
        return Self {
            api: ffi::DesktopApi::load(),
            mode,
            swap_path,
            owned,
            recorded: BTreeMap::new(),
            applied: BTreeMap::new(),
            pending: BTreeMap::new(),
            deferred: BTreeMap::new(),
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
        self.send_originals(mtm, &BTreeSet::new(), None);
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

    pub fn settled(&self) -> bool {
        return self.pending.is_empty();
    }

    pub fn stills_in_use(&self) -> impl Iterator<Item = &str> {
        return self
            .applied
            .values()
            .chain(self.pending.values().map(|pending| &pending.still))
            .chain(self.deferred.values().map(|(_, still)| still))
            .map(String::as_str)
            .chain(self.recorded.values().filter_map(|original| original.url.as_deref()));
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
        let mut leftovers = Vec::new();
        for target in needed {
            let surface = target.space.clone();
            let original = match self.mode {
                Mode::PerSpace => match self.api.copy(target.display_id, &target.space) {
                    Some(config) => Original {
                        space: target.space.clone(),
                        display_id: target.display_id,
                        url: ffi::image_path(&config),
                        config: ffi::encode(&config),
                        taken_at: jiff::Timestamp::now(),
                    },
                    None => Original {
                        space: target.space.clone(),
                        display_id: target.display_id,
                        url: None,
                        config: None,
                        taken_at: jiff::Timestamp::now(),
                    },
                },
                Mode::Screen => {
                    let url = ffi::screen_path(mtm, target.display_id);
                    Original {
                        space: target.space.clone(),
                        display_id: target.display_id,
                        url: url.clone(),
                        config: None,
                        taken_at: jiff::Timestamp::now(),
                    }
                }
            };
            if is_owned(&original, &self.owned) || is_dangling(&original) {
                leftovers.push(original);
                continue;
            }
            self.recorded.insert(surface, original);
        }
        for original in leftovers {
            let adopted = adopt_foreign(original, &self.recorded, &self.owned);
            if is_dangling(&adopted) {
                tracing::warn!(space = %adopted.space, "desktop picture file is gone; the space has nothing to restore");
                self.recorded.insert(adopted.space.clone(), Original { url: None, config: None, ..adopted });
                continue;
            }
            self.recorded.insert(adopted.space.clone(), adopted);
        }
        self.persist();
    }

    fn apply(&mut self, mtm: MainThreadMarker, targets: &[Target], stills: &BTreeMap<String, String>) {
        if self.locked && self.mode == Mode::Screen {
            return;
        }
        self.deferred.retain(|surface, (_, still)| stills.get(surface) == Some(still));
        for (target, still) in apply_targets(&self.recorded, &self.applied, stills, targets) {
            let surface = target.space.clone();
            if self.pending.contains_key(&surface) {
                self.deferred.insert(surface, (target, still));
                continue;
            }
            self.send(mtm, &target, still);
        }
    }

    fn send(&mut self, mtm: MainThreadMarker, target: &Target, still: String) {
        match self.mode {
            Mode::PerSpace => {
                let config = ffi::still_config(&still);
                self.api.set(target.display_id, &config, &target.space);
            }
            Mode::Screen => {
                if let Err(error) = ffi::screen_set(mtm, target.display_id, &still) {
                    self.last_error = Some(error);
                    return;
                }
            }
        }
        let surface = target.space.clone();
        self.applied.insert(surface.clone(), still.clone());
        self.pending.insert(surface, Pending { display_id: target.display_id, space: target.space.clone(), still });
        self.verify_started = Some(Instant::now());
    }

    fn schedule(&mut self) {
        if self.pending.is_empty() || self.timer.is_some() {
            return;
        }
        self.timer = Some(schedule_timer(VERIFY_INTERVAL, true, || Event::DesktopVerify));
    }

    pub fn verify_tick(&mut self, mtm: MainThreadMarker) {
        let timed_out = self.verify_started.is_some_and(|started| started.elapsed() >= VERIFY_TIMEOUT);
        let settled: Vec<String> = self
            .pending
            .iter()
            .filter(|(_, pending)| timed_out || self.read_back(mtm, pending).as_deref() == Some(pending.still.as_str()))
            .map(|(surface, _)| surface.clone())
            .collect();
        if timed_out {
            tracing::warn!(pending = settled.len(), "desktop picture takeover unconfirmed");
        }
        for surface in settled {
            self.pending.remove(&surface);
            if let Some((target, still)) = self.deferred.remove(&surface) {
                self.send(mtm, &target, still);
            }
        }
        if self.pending.is_empty() {
            tracing::info!("desktop picture takeover settled");
            self.stop_verify();
        }
    }

    fn stop_verify(&mut self) {
        cancel_timer(self.timer.take());
        self.verify_started = None;
        self.pending.clear();
        self.deferred.clear();
    }

    fn read_back(&self, mtm: MainThreadMarker, pending: &Pending) -> Option<String> {
        return self.current(mtm, pending.display_id, &pending.space);
    }

    pub fn restore(&mut self, mtm: MainThreadMarker) {
        let in_flight: BTreeSet<String> = self.pending.keys().cloned().collect();
        let since = self.verify_started;
        self.stop_verify();
        self.send_originals(mtm, &in_flight, since);
        if let Err(error) = std::fs::remove_file(&self.swap_path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(%error, "cannot remove swap state");
        }
        self.recorded.clear();
        self.applied.clear();
    }

    pub fn revert(&mut self, mtm: MainThreadMarker, spaces: &[String]) {
        for space in spaces {
            if self.applied.remove(space).is_none() {
                continue;
            }
            let in_flight = self.pending.remove(space).is_some();
            self.deferred.remove(space);
            if let Some(original) = self.recorded.get(space)
                && let Some(source) = self.restorable(mtm, original, in_flight)
            {
                self.send_original(mtm, &source);
            }
        }
    }

    fn restorable(&self, mtm: MainThreadMarker, original: &Original, in_flight: bool) -> Option<Original> {
        let current = self.current(mtm, original.display_id, &original.space);
        if !in_flight && !shows_still(current.as_deref(), &self.owned) {
            return None;
        }
        let source = restore_source(original, &self.recorded, &self.owned);
        if source.is_none() {
            tracing::warn!(space = %original.space, "no original desktop picture is known; leaving the still on screen");
        }
        return source;
    }

    fn send_originals(&self, mtm: MainThreadMarker, in_flight: &BTreeSet<String>, since: Option<Instant>) {
        let mut unconfirmed = 0;
        for _ in 0..RESTORE_ATTEMPTS {
            let landing = !in_flight.is_empty() && since.is_some_and(|started| started.elapsed() < VERIFY_TIMEOUT);
            let sources: Vec<Original> = self
                .recorded
                .values()
                .filter_map(|original| self.restorable(mtm, original, landing && in_flight.contains(&original.space)))
                .collect();
            if sources.is_empty() && !landing {
                return;
            }
            for source in &sources {
                self.send_original(mtm, source);
            }
            std::thread::sleep(RESTORE_WAIT);
            unconfirmed = sources.iter().filter(|source| !self.restored(mtm, source)).count();
            if unconfirmed == 0 && !landing {
                return;
            }
        }
        tracing::warn!(surfaces = unconfirmed, "desktop pictures were not restored");
    }

    fn current(&self, mtm: MainThreadMarker, display_id: u32, space: &str) -> Option<String> {
        return match self.mode {
            Mode::PerSpace => self.api.copy(display_id, space).and_then(|config| ffi::image_path(&config)),
            Mode::Screen => ffi::screen_path(mtm, display_id),
        };
    }

    fn send_original(&self, mtm: MainThreadMarker, original: &Original) {
        match self.mode {
            Mode::PerSpace => {
                let Some(encoded) = &original.config else { return };
                let Some(config) = ffi::decode(encoded) else { return };
                if ffi::is_empty(&config) {
                    tracing::warn!(space = %original.space, "skipping restore of an empty desktop picture config");
                    return;
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

    fn restored(&self, mtm: MainThreadMarker, original: &Original) -> bool {
        let Some(url) = &original.url else { return true };
        return self.current(mtm, original.display_id, &original.space).is_some_and(|value| value == *url);
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
            let surface = target.space.clone();
            let original = self.recorded.get(&surface);
            let current = self.current(mtm, target.display_id, &target.space);
            let owned = self.applied.contains_key(&surface);
            surfaces.push(SurfaceStatus {
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

    fn target(space: &str) -> Target {
        return Target { space: space.into(), display_id: 1 };
    }

    fn original(space: &str) -> Original {
        return Original {
            space: space.into(),
            display_id: 1,
            config: Some("x".into()),
            url: Some("/a.jpg".into()),
            taken_at: jiff::Timestamp::UNIX_EPOCH,
        };
    }

    #[test]
    fn leftover_still_adopts_a_foreign_original() {
        let owned = vec![PathBuf::from("/data/Stills")];
        let recorded = BTreeMap::from([("S1".to_string(), original("S1"))]);
        let leftover =
            Original { url: Some("/data/Stills/abc.png".into()), config: Some("y".into()), ..original("S2") };
        let adopted = adopt_foreign(leftover, &recorded, &owned);
        assert_eq!(adopted.space, "S2");
        assert_eq!(adopted.url.as_deref(), Some("/a.jpg"));
        assert_eq!(adopted.config.as_deref(), Some("x"));
    }

    #[test]
    fn foreign_original_is_kept() {
        let owned = vec![PathBuf::from("/data/Stills")];
        let adopted = adopt_foreign(original("S2"), &BTreeMap::new(), &owned);
        assert_eq!(adopted, original("S2"));
    }

    #[test]
    fn leftover_without_donor_is_kept() {
        let owned = vec![PathBuf::from("/data/Stills")];
        let leftover = Original { url: Some("/data/Stills/abc.png".into()), ..original("S2") };
        let adopted = adopt_foreign(leftover.clone(), &BTreeMap::new(), &owned);
        assert_eq!(adopted, leftover);
    }

    #[test]
    fn capture_only_targets_not_already_recorded() {
        let recorded = BTreeMap::from([("S1".to_string(), original("S1"))]);
        let targets = [target("S1"), target("S2")];
        let needed = capture_targets(&recorded, &targets);
        assert_eq!(needed, vec![target("S2")]);
    }

    #[test]
    fn apply_skips_unrecorded_and_unchanged() {
        let recorded = BTreeMap::from([("S1".to_string(), original("S1")), ("S2".to_string(), original("S2"))]);
        let applied = BTreeMap::from([("S1".to_string(), "/still1.png".to_string())]);
        let stills = BTreeMap::from([
            ("S1".to_string(), "/still1.png".to_string()),
            ("S2".to_string(), "/still2.png".to_string()),
            ("S3".to_string(), "/still3.png".to_string()),
        ]);
        let targets = [target("S1"), target("S2"), target("S3")];
        let plan = apply_targets(&recorded, &applied, &stills, &targets);
        assert_eq!(plan, vec![(target("S2"), "/still2.png".to_string())]);
    }

    #[test]
    fn apply_retries_when_the_still_changes() {
        let recorded = BTreeMap::from([("S1".to_string(), original("S1"))]);
        let applied = BTreeMap::from([("S1".to_string(), "/old.png".to_string())]);
        let stills = BTreeMap::from([("S1".to_string(), "/new.png".to_string())]);
        let plan = apply_targets(&recorded, &applied, &stills, &[target("S1")]);
        assert_eq!(plan, vec![(target("S1"), "/new.png".to_string())]);
    }

    #[test]
    fn a_user_wallpaper_is_restored_as_recorded() {
        let owned = vec![PathBuf::from("/data/Stills")];
        assert_eq!(restore_source(&original("S1"), &BTreeMap::new(), &owned), Some(original("S1")));
    }

    #[test]
    fn a_leftover_still_restores_another_original_on_the_same_display_first() {
        let owned = vec![PathBuf::from("/data/Stills"), PathBuf::from("/data/private/Stills")];
        let leftover = Original { url: Some("/data/private/Stills/old.png".into()), ..original("S1") };
        let other = Original { display_id: 2, url: Some("/b.jpg".into()), ..original("S2") };
        let same = Original { url: Some("/c.jpg".into()), config: Some("c".into()), ..original("S3") };
        let recorded =
            BTreeMap::from([("S1".to_string(), leftover.clone()), ("S2".to_string(), other), ("S3".to_string(), same)]);
        let source = restore_source(&leftover, &recorded, &owned).expect("donor");
        assert_eq!(
            (source.space.as_str(), source.url.as_deref(), source.config.as_deref()),
            ("S1", Some("/c.jpg"), Some("c"))
        );
    }

    #[test]
    fn nothing_is_restored_without_a_known_user_wallpaper() {
        let owned = vec![PathBuf::from("/data/Stills")];
        let leftover = Original { url: Some("/data/Stills/old.png".into()), ..original("S1") };
        let pathless = Original { url: None, ..original("S2") };
        let recorded = BTreeMap::from([("S1".to_string(), leftover.clone()), ("S2".to_string(), pathless.clone())]);
        assert_eq!(restore_source(&leftover, &recorded, &owned), None);
        assert_eq!(restore_source(&pathless, &recorded, &owned), None);
    }

    #[test]
    fn a_desktop_picture_whose_file_is_gone_is_not_recorded() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("real.jpg");
        std::fs::write(&file, b"jpg").expect("file");
        let existing = Original { url: Some(file.to_string_lossy().into_owned()), ..original("S2") };
        assert!(!is_dangling(&existing));
        let missing = Original { url: Some("/tmp/livewall-e2e-gone/abc.png".into()), ..original("S1") };
        assert!(is_dangling(&missing));
        let pathless = Original { url: None, ..original("S3") };
        assert!(!is_dangling(&pathless));
    }
    #[test]
    fn only_a_desktop_showing_a_still_is_restored() {
        let owned = vec![PathBuf::from("/data/Stills")];
        assert!(shows_still(Some("/data/Stills/now.png"), &owned));
        assert!(!shows_still(Some("/Users/me/Pictures/mine.jpg"), &owned));
        assert!(!shows_still(None, &owned));
    }
}
