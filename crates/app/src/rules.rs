use serde::{Deserialize, Serialize};

use crate::model::{AppRule, RuleScope, Settings};

pub const COVERAGE_THRESHOLD: f64 = 0.9;
pub const SELF_BUNDLE_ID: &str = "io.github.belhetile.livewall";
pub const FOCUS_EXEMPT: [&str; 1] = ["com.apple.finder"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    #[default]
    KeepRunning,
    Pause,
    Stop,
}

impl Action {
    pub fn keeps_running(self) -> bool {
        return self == Self::KeepRunning;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        return Self { x, y, w, h };
    }

    pub fn area(self) -> f64 {
        return self.w.max(0.0) * self.h.max(0.0);
    }

    pub fn intersection(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.w).min(other.x + other.w);
        let bottom = (self.y + self.h).min(other.y + other.h);
        if right <= x || bottom <= y {
            return None;
        }
        return Some(Self { x, y, w: right - x, h: bottom - y });
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct Snapshot {
    #[serde(default)]
    pub frame: Rect,
    #[serde(default)]
    pub fullscreen: bool,
    #[serde(default)]
    pub coverage: f64,
    #[serde(default)]
    pub other_app_audio: bool,
    #[serde(default)]
    pub frontmost: Option<String>,
    #[serde(default)]
    pub running_apps: Vec<String>,
    #[serde(default)]
    pub display_asleep: bool,
    #[serde(default)]
    pub screen_locked: bool,
    #[serde(default)]
    pub screensaver: bool,
    #[serde(default)]
    pub occluded: bool,
    #[serde(default)]
    pub thermal_hot: bool,
    #[serde(default)]
    pub low_power: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Cause {
    DisplayAsleep,
    ScreenLocked,
    Screensaver,
    Occluded,
    Thermal,
    LowPower,
    OtherAppAudio,
    Fullscreen,
    Coverage,
    Focus,
    App(String),
}

impl Cause {
    pub fn label(&self) -> String {
        return match self {
            Self::DisplayAsleep => "display-asleep".to_string(),
            Self::ScreenLocked => "screen-locked".to_string(),
            Self::Screensaver => "screensaver".to_string(),
            Self::Occluded => "occluded".to_string(),
            Self::Thermal => "thermal".to_string(),
            Self::LowPower => "low-power".to_string(),
            Self::OtherAppAudio => "other-app-audio".to_string(),
            Self::Fullscreen => "fullscreen".to_string(),
            Self::Coverage => "coverage".to_string(),
            Self::Focus => "focus".to_string(),
            Self::App(bundle) => format!("app:{bundle}"),
        };
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub display_asleep: Action,
    pub screen_locked: Action,
    pub screensaver: Action,
    pub occlusion: Action,
    pub thermal: Action,
    pub low_power: Action,
    pub other_app_audio: Action,
    pub fullscreen: Action,
    pub coverage: Action,
    pub coverage_threshold: f64,
    pub focus: Option<Action>,
    pub app_rules: Vec<AppRule>,
    pub self_bundle: String,
}

impl Config {
    pub fn from_settings(settings: &Settings) -> Self {
        return Self {
            display_asleep: Action::Stop,
            screen_locked: Action::Stop,
            screensaver: Action::Stop,
            occlusion: Action::Pause,
            thermal: toggle(settings.pause_when_hot),
            low_power: Action::Pause,
            other_app_audio: Action::Pause,
            fullscreen: toggle(settings.pause_when_fullscreen),
            coverage: toggle(settings.pause_when_maximized),
            coverage_threshold: COVERAGE_THRESHOLD,
            focus: settings.pause_when_focused.then_some(Action::Pause),
            app_rules: settings.app_rules.clone(),
            self_bundle: SELF_BUNDLE_ID.to_string(),
        };
    }
}

fn toggle(on: bool) -> Action {
    if on {
        return Action::Pause;
    }
    return Action::KeepRunning;
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Verdict {
    pub action: Action,
    pub cause: Option<Cause>,
}

pub fn evaluate(snapshot: &Snapshot, config: &Config) -> Verdict {
    let mut action = Action::KeepRunning;
    let mut cause = None;
    if snapshot.display_asleep {
        consider(&mut action, &mut cause, config.display_asleep, Cause::DisplayAsleep);
    }
    if snapshot.screen_locked {
        consider(&mut action, &mut cause, config.screen_locked, Cause::ScreenLocked);
    }
    if snapshot.screensaver {
        consider(&mut action, &mut cause, config.screensaver, Cause::Screensaver);
    }
    if snapshot.occluded {
        consider(&mut action, &mut cause, config.occlusion, Cause::Occluded);
    }
    if snapshot.thermal_hot {
        consider(&mut action, &mut cause, config.thermal, Cause::Thermal);
    }
    if snapshot.low_power {
        consider(&mut action, &mut cause, config.low_power, Cause::LowPower);
    }
    if snapshot.other_app_audio {
        consider(&mut action, &mut cause, config.other_app_audio, Cause::OtherAppAudio);
    }
    if snapshot.fullscreen {
        consider(&mut action, &mut cause, config.fullscreen, Cause::Fullscreen);
    }
    if snapshot.coverage >= config.coverage_threshold {
        consider(&mut action, &mut cause, config.coverage, Cause::Coverage);
    }
    if let Some(focus) = config.focus
        && !focus_exempt(snapshot, config)
    {
        consider(&mut action, &mut cause, focus, Cause::Focus);
    }
    for rule in &config.app_rules {
        if app_running(rule, snapshot) {
            consider(&mut action, &mut cause, Action::Pause, Cause::App(rule.bundle_id.clone()));
        }
    }
    return Verdict { action, cause };
}

fn consider(action: &mut Action, cause: &mut Option<Cause>, candidate: Action, why: Cause) {
    if candidate > *action {
        *action = candidate;
        *cause = Some(why);
    }
}

fn focus_exempt(snapshot: &Snapshot, config: &Config) -> bool {
    let Some(frontmost) = &snapshot.frontmost else {
        return false;
    };
    return frontmost == &config.self_bundle || FOCUS_EXEMPT.contains(&frontmost.as_str());
}

fn app_running(rule: &AppRule, snapshot: &Snapshot) -> bool {
    return match rule.scope {
        RuleScope::Frontmost => snapshot.frontmost.as_deref() == Some(rule.bundle_id.as_str()),
        RuleScope::Running => snapshot.running_apps.iter().any(|bundle| bundle == &rule.bundle_id),
    };
}

pub fn geometry_poll_needed(any_playing: bool, verdict: &Verdict, settings: &Settings) -> bool {
    if !settings.pause_when_fullscreen && !settings.pause_when_maximized {
        return false;
    }
    let geometry_paused = matches!(verdict.cause, Some(Cause::Fullscreen | Cause::Coverage));
    return any_playing || geometry_paused;
}

pub fn should_rotate(auto_rotate: bool, paused: bool, verdict: &Verdict, eligible: usize) -> bool {
    return auto_rotate && !paused && verdict.action.keeps_running() && eligible >= 2;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(fullscreen: bool, coverage: f64) -> Snapshot {
        return Snapshot { frame: Rect::new(0.0, 0.0, 1920.0, 1080.0), fullscreen, coverage, ..Snapshot::default() };
    }

    fn base() -> Config {
        return Config::from_settings(&Settings::default());
    }

    fn one(snapshot: &Snapshot, config: &Config) -> Verdict {
        return evaluate(snapshot, config);
    }

    #[test]
    fn action_ordering_is_least_to_most_restrictive() {
        assert!(Action::KeepRunning < Action::Pause);
        assert!(Action::Pause < Action::Stop);
        assert!(Action::KeepRunning.keeps_running());
        assert!(!Action::Pause.keeps_running());
        assert!(!Action::Stop.keeps_running());
    }

    #[test]
    fn idle_snapshot_keeps_running() {
        let verdict = one(&make(false, 0.0), &base());
        assert_eq!(verdict.action, Action::KeepRunning);
        assert_eq!(verdict.cause, None);
    }

    #[test]
    fn display_asleep_stops() {
        let snapshot = Snapshot { display_asleep: true, ..make(false, 0.0) };
        let verdict = one(&snapshot, &base());
        assert_eq!(verdict.action, Action::Stop);
        assert_eq!(verdict.cause, Some(Cause::DisplayAsleep));
    }

    #[test]
    fn screen_locked_stops() {
        let snapshot = Snapshot { screen_locked: true, ..make(false, 0.0) };
        let verdict = one(&snapshot, &base());
        assert_eq!(verdict.action, Action::Stop);
        assert_eq!(verdict.cause, Some(Cause::ScreenLocked));
    }

    #[test]
    fn screensaver_stops() {
        let snapshot = Snapshot { screensaver: true, ..make(false, 0.0) };
        let verdict = one(&snapshot, &base());
        assert_eq!(verdict.action, Action::Stop);
        assert_eq!(verdict.cause, Some(Cause::Screensaver));
    }

    #[test]
    fn occluded_pauses() {
        let snapshot = Snapshot { occluded: true, ..make(false, 0.0) };
        let verdict = one(&snapshot, &base());
        assert_eq!(verdict.action, Action::Pause);
        assert_eq!(verdict.cause, Some(Cause::Occluded));
    }

    #[test]
    fn thermal_pauses_when_hot() {
        let snapshot = Snapshot { thermal_hot: true, ..make(false, 0.0) };
        let verdict = one(&snapshot, &base());
        assert_eq!(verdict.action, Action::Pause);
        assert_eq!(verdict.cause, Some(Cause::Thermal));
    }

    #[test]
    fn thermal_ignored_when_disabled() {
        let config = Config::from_settings(&Settings { pause_when_hot: false, ..Settings::default() });
        let snapshot = Snapshot { thermal_hot: true, ..make(false, 0.0) };
        assert_eq!(one(&snapshot, &config).action, Action::KeepRunning);
    }

    #[test]
    fn low_power_pauses() {
        let snapshot = Snapshot { low_power: true, ..make(false, 0.0) };
        let verdict = one(&snapshot, &base());
        assert_eq!(verdict.action, Action::Pause);
        assert_eq!(verdict.cause, Some(Cause::LowPower));
    }

    #[test]
    fn other_app_audio_pauses() {
        let snapshot = Snapshot { other_app_audio: true, ..make(false, 0.0) };
        let verdict = one(&snapshot, &base());
        assert_eq!(verdict.action, Action::Pause);
        assert_eq!(verdict.cause, Some(Cause::OtherAppAudio));
    }

    #[test]
    fn fullscreen_pauses_when_enabled() {
        let verdict = one(&make(true, 1.0), &base());
        assert_eq!(verdict.action, Action::Pause);
        assert_eq!(verdict.cause, Some(Cause::Fullscreen));
    }

    #[test]
    fn fullscreen_ignored_when_disabled() {
        let mut config = base();
        config.fullscreen = Action::KeepRunning;
        assert_eq!(one(&make(true, 0.0), &config).action, Action::KeepRunning);
    }

    #[test]
    fn coverage_fires_at_threshold_only() {
        let config = base();
        assert_eq!(one(&make(false, 0.89), &config).action, Action::KeepRunning);
        let at = one(&make(false, 0.9), &config);
        assert_eq!(at.action, Action::Pause);
        assert_eq!(at.cause, Some(Cause::Coverage));
    }

    #[test]
    fn coverage_ignored_when_disabled() {
        let mut config = base();
        config.coverage = Action::KeepRunning;
        assert_eq!(one(&make(false, 1.0), &config).action, Action::KeepRunning);
    }

    #[test]
    fn focus_pauses_for_other_apps() {
        let mut config = base();
        config.focus = Some(Action::Pause);
        let focused = Snapshot { frontmost: Some("com.example.app".into()), ..make(false, 0.0) };
        let verdict = one(&focused, &config);
        assert_eq!(verdict.action, Action::Pause);
        assert_eq!(verdict.cause, Some(Cause::Focus));
    }

    #[test]
    fn focus_exempts_self_and_finder() {
        let mut config = base();
        config.focus = Some(Action::Pause);
        for frontmost in [SELF_BUNDLE_ID.to_string(), "com.apple.finder".to_string()] {
            let snapshot = Snapshot { frontmost: Some(frontmost), ..make(false, 0.0) };
            assert_eq!(one(&snapshot, &config).action, Action::KeepRunning);
        }
    }

    #[test]
    fn app_rules_match_running_and_frontmost() {
        let running =
            Snapshot { frontmost: Some("com.other".into()), running_apps: vec!["com.game".into()], ..make(false, 0.0) };
        let mut config = base();
        config.app_rules = vec![AppRule { bundle_id: "com.game".into(), scope: RuleScope::Running }];
        let verdict = one(&running, &config);
        assert_eq!(verdict.action, Action::Pause);
        assert_eq!(verdict.cause, Some(Cause::App("com.game".into())));

        let frontmost =
            Snapshot { frontmost: Some("com.game".into()), running_apps: vec!["com.game".into()], ..make(false, 0.0) };
        let mut config = base();
        config.app_rules = vec![AppRule { bundle_id: "com.game".into(), scope: RuleScope::Frontmost }];
        assert_eq!(one(&frontmost, &config).cause, Some(Cause::App("com.game".into())));

        let background =
            Snapshot { frontmost: Some("com.other".into()), running_apps: vec!["com.game".into()], ..make(false, 0.0) };
        assert_eq!(one(&background, &config).action, Action::KeepRunning);
    }

    #[test]
    fn app_rules_can_be_empty() {
        let mut config = base();
        config.app_rules = vec![];
        assert_eq!(one(&make(false, 0.0), &config).action, Action::KeepRunning);
    }

    #[test]
    fn most_restrictive_wins() {
        let snapshot = Snapshot { low_power: true, other_app_audio: true, ..make(true, 1.0) };
        let verdict = one(&snapshot, &base());
        assert_eq!(verdict.action, Action::Pause);
        assert_eq!(verdict.cause, Some(Cause::LowPower));

        let stopped = Snapshot { screen_locked: true, ..snapshot };
        assert_eq!(one(&stopped, &base()).action, Action::Stop);
    }

    #[test]
    fn settings_toggles_drive_geometry_rules() {
        let settings = Settings { pause_when_fullscreen: false, pause_when_maximized: false, ..Settings::default() };
        let config = Config::from_settings(&settings);
        assert_eq!(config.fullscreen, Action::KeepRunning);
        assert_eq!(config.coverage, Action::KeepRunning);
        let settings = Settings { pause_when_fullscreen: true, pause_when_maximized: true, ..Settings::default() };
        let config = Config::from_settings(&settings);
        assert_eq!(config.fullscreen, Action::Pause);
        assert_eq!(config.coverage, Action::Pause);
    }

    #[test]
    fn focus_setting_maps_to_optional_rule() {
        let off = Config::from_settings(&Settings { pause_when_focused: false, ..Settings::default() });
        assert_eq!(off.focus, None);
        let on = Config::from_settings(&Settings { pause_when_focused: true, ..Settings::default() });
        assert_eq!(on.focus, Some(Action::Pause));
    }

    #[test]
    fn geometry_polling_requires_playback_and_a_geometry_rule() {
        let settings = |fullscreen: bool, maximized: bool| {
            return Settings {
                pause_when_fullscreen: fullscreen,
                pause_when_maximized: maximized,
                ..Settings::default()
            };
        };
        let paused = Verdict { action: Action::Pause, cause: Some(Cause::Coverage) };
        let audio = Verdict { action: Action::Pause, cause: Some(Cause::OtherAppAudio) };
        let streaming = Verdict { action: Action::KeepRunning, cause: None };
        assert!(geometry_poll_needed(true, &streaming, &settings(true, false)));
        assert!(geometry_poll_needed(true, &streaming, &settings(false, true)));
        assert!(geometry_poll_needed(false, &paused, &settings(false, true)));
        assert!(!geometry_poll_needed(false, &audio, &settings(true, true)));
        assert!(!geometry_poll_needed(false, &streaming, &settings(true, true)));
        assert!(!geometry_poll_needed(true, &paused, &settings(false, false)));
    }

    #[test]
    fn rotation_requires_enabled_running_playback_and_two_items() {
        let running = Verdict::default();
        let paused = Verdict { action: Action::Pause, cause: Some(Cause::OtherAppAudio) };
        let stopped = Verdict { action: Action::Stop, cause: Some(Cause::ScreenLocked) };
        assert!(should_rotate(true, false, &running, 2));
        assert!(should_rotate(true, false, &running, 10));
        assert!(!should_rotate(false, false, &running, 2));
        assert!(!should_rotate(true, true, &running, 2));
        assert!(!should_rotate(true, false, &paused, 2));
        assert!(!should_rotate(true, false, &stopped, 2));
        assert!(!should_rotate(true, false, &running, 1));
        assert!(!should_rotate(true, false, &running, 0));
    }

    #[test]
    fn rect_intersection_and_area() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);
        assert_eq!(a.intersection(b), Some(Rect::new(5.0, 5.0, 5.0, 5.0)));
        assert_eq!(a.intersection(Rect::new(20.0, 20.0, 1.0, 1.0)), None);
        assert!((a.area() - 100.0).abs() < 1e-9);
    }
}
