use objc2::rc::Retained;
use objc2_foundation::NSTimer;

use crate::app::Event;
use crate::engine::Engine;
use crate::platform::{cancel_timer, schedule_timer};

const SETTLE_SECONDS: f64 = 1.0;

#[derive(Default)]
pub struct Occlusion {
    occluded: bool,
    timer: Option<Retained<NSTimer>>,
}

impl Engine {
    pub fn occluded(&self) -> bool {
        return self.occlusion.occluded;
    }

    pub fn on_occlusion_changed(&mut self) -> bool {
        if self.occluded_now() {
            if !self.occlusion.occluded && self.occlusion.timer.is_none() {
                self.occlusion.timer = Some(schedule_timer(SETTLE_SECONDS, false, || Event::OcclusionSettled));
            }
            return false;
        }
        cancel_timer(self.occlusion.timer.take());
        if self.occlusion.occluded {
            self.occlusion.occluded = false;
            return true;
        }
        return false;
    }

    pub fn on_occlusion_settled(&mut self) -> bool {
        self.occlusion.timer = None;
        if self.occluded_now() && !self.occlusion.occluded {
            self.occlusion.occluded = true;
            return true;
        }
        return false;
    }

    fn occluded_now(&self) -> bool {
        let active: Vec<bool> = self
            .surfaces
            .values()
            .filter(|surface| surface.window.is_on_active_space())
            .map(|surface| surface.window.occluded())
            .collect();
        if active.is_empty() {
            return false;
        }
        return active.iter().all(|occluded| *occluded);
    }
}
