pub mod auth;
pub mod desktop;
pub mod detect;
pub mod hotkeys;
pub mod observe;
pub mod spaces;
pub mod system;

use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_foundation::NSTimer;

use crate::app::{Event, post};

fn apply_timer_tolerance(timer: &NSTimer, interval: f64) {
    timer.setTolerance(interval * 0.1);
}

pub fn schedule_block(interval: f64, repeats: bool, action: impl Fn() + 'static) -> Retained<NSTimer> {
    let block = RcBlock::new(move |_timer: NonNull<NSTimer>| action());
    let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(interval, repeats, &block) };
    apply_timer_tolerance(&timer, interval);
    return timer;
}

pub fn schedule_timer(interval: f64, repeats: bool, event: impl Fn() -> Event + 'static) -> Retained<NSTimer> {
    return schedule_block(interval, repeats, move || post(event()));
}

pub fn cancel_timer(timer: Option<Retained<NSTimer>>) {
    if let Some(timer) = timer {
        timer.invalidate();
    }
}
