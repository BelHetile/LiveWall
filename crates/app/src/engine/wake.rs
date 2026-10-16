use std::collections::HashMap;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2_foundation::NSTimer;

use crate::app::Event;
use crate::engine::Engine;
use crate::engine::node::Node;
use crate::engine::video::VideoNode;
use crate::model::ItemId;
use crate::platform::{cancel_timer, schedule_timer};

const SETTLE_SECONDS: f64 = 3.0;
const RECHECK_SECONDS: f64 = 2.0;
const PROGRESS_EPSILON: f64 = 0.1;
const REBUILD_LIMIT: usize = 2;
const REBUILD_WINDOW: Duration = Duration::from_mins(10);

struct Check {
    baseline: f64,
    attempts: u8,
}

#[derive(Default)]
pub struct Wake {
    checks: HashMap<ItemId, Check>,
    timers: HashMap<ItemId, Retained<NSTimer>>,
    rebuilds: HashMap<ItemId, Vec<Instant>>,
}

impl Wake {
    pub fn forget(&mut self, item: ItemId) {
        cancel_timer(self.timers.remove(&item));
        self.checks.remove(&item);
        self.rebuilds.remove(&item);
    }

    fn schedule(&mut self, item: ItemId, delay: f64) {
        cancel_timer(self.timers.remove(&item));
        let timer = schedule_timer(delay, false, move || Event::WakeCheck { item });
        self.timers.insert(item, timer);
    }
}

impl Engine {
    pub fn on_wake(&mut self) {
        let playing: Vec<ItemId> = self
            .nodes
            .iter()
            .filter(|(_, node)| node.video().is_some_and(VideoNode::is_playing))
            .map(|(item, _)| *item)
            .collect();
        for item in playing {
            let Some(video) = self.nodes.get(&item).and_then(Node::video) else { continue };
            let baseline = video.current_time();
            self.wake.checks.insert(item, Check { baseline, attempts: 0 });
            self.wake.schedule(item, SETTLE_SECONDS);
        }
    }

    pub fn on_wake_check(&mut self, item: ItemId) {
        let Some(check) = self.wake.checks.remove(&item) else { return };
        let Some(video) = self.nodes.get_mut(&item).and_then(Node::video_mut).filter(|video| video.is_playing()) else {
            return;
        };
        if advanced(check.baseline, video.current_time(), PROGRESS_EPSILON) {
            return;
        }
        match check.attempts {
            0 => {
                video.nudge();
                let baseline = video.current_time();
                self.wake.checks.insert(item, Check { baseline, attempts: 1 });
                self.wake.schedule(item, RECHECK_SECONDS);
            }
            1 => {
                let now = Instant::now();
                let ages: Vec<Duration> = self
                    .wake
                    .rebuilds
                    .get(&item)
                    .map_or_else(Vec::new, |times| times.iter().map(|at| now.duration_since(*at)).collect());
                if !rebuild_allowed(&ages, REBUILD_WINDOW, REBUILD_LIMIT) {
                    return;
                }
                video.rebuild(self.mtm);
                self.wake.rebuilds.entry(item).or_default().push(now);
                tracing::info!(%item, "rebuilt a stalled video after wake");
            }
            _ => {}
        }
    }
}

fn advanced(baseline: f64, current: f64, epsilon: f64) -> bool {
    return (current - baseline).abs() > epsilon;
}

fn rebuild_allowed(ages: &[Duration], window: Duration, limit: usize) -> bool {
    return ages.iter().filter(|age| **age < window).count() < limit;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advanced_requires_movement_past_the_epsilon() {
        assert!(advanced(1.0, 1.5, PROGRESS_EPSILON));
        assert!(!advanced(1.0, 1.05, PROGRESS_EPSILON));
        assert!(!advanced(1.0, 1.0, PROGRESS_EPSILON));
    }

    #[test]
    fn advanced_treats_a_loop_wrap_as_progress() {
        assert!(advanced(4.9, 0.2, PROGRESS_EPSILON));
    }

    #[test]
    fn rebuilds_are_limited_to_two_per_window() {
        assert!(rebuild_allowed(&[], REBUILD_WINDOW, REBUILD_LIMIT));
        assert!(rebuild_allowed(&[Duration::from_secs(1)], REBUILD_WINDOW, REBUILD_LIMIT));
        assert!(!rebuild_allowed(&[Duration::from_secs(1), Duration::from_secs(2)], REBUILD_WINDOW, REBUILD_LIMIT));
    }

    #[test]
    fn rebuilds_outside_the_window_are_forgotten() {
        let old = [Duration::from_secs(601), Duration::from_secs(602)];
        assert!(rebuild_allowed(&old, REBUILD_WINDOW, REBUILD_LIMIT));
    }
}
