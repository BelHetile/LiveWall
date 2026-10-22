use objc2_app_kit::NSWorkspace;
use objc2_core_foundation::{CFDictionary, CFString, CFType};
use objc2_core_graphics::{
    CGRectMakeWithDictionaryRepresentation, CGWindowListCopyWindowInfo, CGWindowListOption, kCGNullWindowID,
    kCGWindowAlpha, kCGWindowBounds, kCGWindowLayer, kCGWindowOwnerPID,
};

use crate::cf::{cf_f64, cf_i64, downcast_dict};
use crate::rules;

const FULLSCREEN_TOLERANCE: f64 = 2.0;

#[derive(Debug, Clone, Copy)]
#[allow(clippy::struct_excessive_bools)]
pub struct Signals {
    pub display_asleep: bool,
    pub screen_locked: bool,
    pub screensaver: bool,
    pub occluded: bool,
    pub thermal_hot: bool,
    pub low_power: bool,
}

pub fn snapshot(frame: rules::Rect, usable: rules::Rect, signals: Signals) -> rules::Snapshot {
    let windows = on_screen_windows(frame);
    let own = own_pid();
    let candidates: Vec<rules::Rect> =
        windows.iter().filter(|window| is_candidate(window, own)).map(|window| window.bounds).collect();
    let fullscreen = candidates.iter().any(|window| approximately_covers(*window, frame, FULLSCREEN_TOLERANCE));
    let usable = coverage_area(frame, usable);
    let coverage = coverage_fraction(&candidates, usable);
    return rules::Snapshot {
        frame,
        usable,
        fullscreen,
        coverage,
        frontmost: frontmost_bundle(),
        running_apps: running_bundles(),
        display_asleep: signals.display_asleep,
        screen_locked: signals.screen_locked,
        screensaver: signals.screensaver,
        occluded: signals.occluded,
        thermal_hot: signals.thermal_hot,
        low_power: signals.low_power,
    };
}

#[derive(Debug, Clone, Copy)]
struct Window {
    bounds: rules::Rect,
    layer: i64,
    pid: i32,
    alpha: f64,
}

fn own_pid() -> i32 {
    return std::process::id().cast_signed();
}

fn on_screen_windows(frame: rules::Rect) -> Vec<Window> {
    let options = CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let Some(raw) = CGWindowListCopyWindowInfo(options, kCGNullWindowID) else {
        return Vec::new();
    };
    let mut windows = Vec::new();
    for index in 0..raw.count() {
        let pointer = unsafe { raw.value_at_index(index) };
        if pointer.is_null() {
            continue;
        }
        let value: &CFType = unsafe { &*pointer.cast::<CFType>() };
        let Some(dictionary) = downcast_dict(Some(value)) else { continue };
        let Some(bounds) = cf_rect(dictionary, unsafe { kCGWindowBounds }) else { continue };
        let layer = cf_i64(dictionary.get(unsafe { kCGWindowLayer }).as_deref()).unwrap_or(0);
        let pid = cf_i64(dictionary.get(unsafe { kCGWindowOwnerPID }).as_deref())
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or(0);
        let alpha = cf_f64(dictionary.get(unsafe { kCGWindowAlpha }).as_deref()).unwrap_or(1.0);
        let flipped = rules::Rect::new(bounds.x, frame.h - (bounds.y + bounds.h), bounds.w, bounds.h);
        windows.push(Window { bounds: flipped, layer, pid, alpha });
    }
    return windows;
}

fn cf_rect(dictionary: &CFDictionary<CFString, CFType>, key: &CFString) -> Option<rules::Rect> {
    let bounds = dictionary.get(key)?;
    let bounds = bounds.downcast_ref::<CFDictionary>()?;
    let mut rect = objc2_core_foundation::CGRect::default();
    if !unsafe { CGRectMakeWithDictionaryRepresentation(Some(bounds), std::ptr::from_mut(&mut rect)) } {
        return None;
    }
    return Some(rules::Rect::new(rect.origin.x, rect.origin.y, rect.size.width, rect.size.height));
}

fn is_candidate(window: &Window, own: i32) -> bool {
    return window.pid != own && window.layer == 0 && window.alpha > 0.0;
}

// A zoomed window stops at the menu bar and the Dock, so coverage is measured on the area left between them.
fn coverage_area(frame: rules::Rect, usable: rules::Rect) -> rules::Rect {
    return usable.intersection(frame).filter(|area| area.area() > 0.0).unwrap_or(frame);
}

fn coverage_fraction(windows: &[rules::Rect], screen: rules::Rect) -> f64 {
    let area = screen.area();
    if area <= 0.0 {
        return 0.0;
    }
    let clipped = windows.iter().filter_map(|window| window.intersection(screen));
    return (union_area(&clipped.collect::<Vec<_>>()) / area).clamp(0.0, 1.0);
}

fn union_area(rects: &[rules::Rect]) -> f64 {
    let normalized: Vec<rules::Rect> = rects.iter().copied().filter(|rect| rect.w > 0.0 && rect.h > 0.0).collect();
    if normalized.is_empty() {
        return 0.0;
    }
    let mut xs = Vec::with_capacity(normalized.len() * 2);
    for rect in &normalized {
        xs.push(rect.x);
        xs.push(rect.x + rect.w);
    }
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let mut area = 0.0;
    for pair in xs.windows(2) {
        let (left, right) = (pair[0], pair[1]);
        let width = right - left;
        if width <= 0.0 {
            continue;
        }
        let mid = left + width / 2.0;
        let mut intervals: Vec<(f64, f64)> = normalized
            .iter()
            .filter(|rect| rect.x <= mid && mid < rect.x + rect.w)
            .map(|rect| (rect.y, rect.y + rect.h))
            .collect();
        area += width * union_length(&mut intervals);
    }
    return area;
}

fn union_length(intervals: &mut [(f64, f64)]) -> f64 {
    if intervals.is_empty() {
        return 0.0;
    }
    intervals.sort_by(|a, b| f64::total_cmp(&a.0, &b.0));
    let (mut start, mut end) = intervals[0];
    let mut total = 0.0;
    for &(next_start, next_end) in &intervals[1..] {
        if next_start > end {
            total += end - start;
            start = next_start;
            end = next_end;
        } else if next_end > end {
            end = next_end;
        }
    }
    return total + (end - start);
}

fn approximately_covers(window: rules::Rect, screen: rules::Rect, tolerance: f64) -> bool {
    return (window.x - screen.x).abs() <= tolerance
        && (window.y - screen.y).abs() <= tolerance
        && (window.w - screen.w).abs() <= tolerance
        && (window.h - screen.h).abs() <= tolerance;
}

fn frontmost_bundle() -> Option<String> {
    return NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .and_then(|application| application.bundleIdentifier())
        .map(|identifier| identifier.to_string());
}

fn running_bundles() -> Vec<String> {
    let applications = NSWorkspace::sharedWorkspace().runningApplications();
    let mut bundles: Vec<String> = applications
        .to_vec()
        .iter()
        .filter_map(|application| application.bundleIdentifier())
        .map(|identifier| identifier.to_string())
        .collect();
    bundles.sort();
    bundles.dedup();
    return bundles;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> rules::Rect {
        return rules::Rect::new(x, y, w, h);
    }

    fn approx(a: f64, b: f64) -> bool {
        return (a - b).abs() < 1e-6;
    }

    #[test]
    fn union_area_of_disjoint_rects_is_the_sum() {
        assert!(approx(union_area(&[rect(0.0, 0.0, 2.0, 2.0), rect(5.0, 5.0, 3.0, 1.0)]), 7.0));
    }

    #[test]
    fn union_area_counts_overlaps_once() {
        let a = rect(0.0, 0.0, 10.0, 10.0);
        let b = rect(5.0, 5.0, 10.0, 10.0);
        assert!(approx(union_area(&[a, b]), 175.0));
    }

    #[test]
    fn union_area_handles_nested_rects() {
        let outer = rect(0.0, 0.0, 10.0, 10.0);
        let inner = rect(2.0, 2.0, 3.0, 3.0);
        assert!(approx(union_area(&[outer, inner]), 100.0));
    }

    #[test]
    fn union_area_of_a_column_of_rects() {
        let rects = [rect(0.0, 0.0, 1.0, 3.0), rect(0.0, 3.0, 1.0, 3.0), rect(0.0, 6.0, 1.0, 3.0)];
        assert!(approx(union_area(&rects), 9.0));
    }

    #[test]
    fn union_area_is_zero_for_empty_or_degenerate() {
        assert!(approx(union_area(&[]), 0.0));
        assert!(approx(union_area(&[rect(0.0, 0.0, 0.0, 10.0)]), 0.0));
    }

    #[test]
    fn coverage_clips_windows_to_the_screen() {
        let screen = rect(0.0, 0.0, 100.0, 100.0);
        assert!(approx(coverage_fraction(&[rect(-50.0, -50.0, 100.0, 100.0)], screen), 0.25));
        assert!(approx(coverage_fraction(&[rect(0.0, 0.0, 100.0, 100.0)], screen), 1.0));
    }

    #[test]
    fn coverage_never_double_counts_overlaps() {
        let screen = rect(0.0, 0.0, 100.0, 100.0);
        let windows = [rect(0.0, 0.0, 60.0, 100.0), rect(40.0, 0.0, 60.0, 100.0)];
        assert!(approx(coverage_fraction(&windows, screen), 1.0));
    }

    #[test]
    fn coverage_is_clamped() {
        let screen = rect(0.0, 0.0, 10.0, 10.0);
        assert!(approx(coverage_fraction(&[rect(0.0, 0.0, 1000.0, 1000.0)], screen), 1.0));
        assert!(approx(coverage_fraction(&[], screen), 0.0));
    }

    #[test]
    fn a_zoomed_window_covers_the_area_between_the_menu_bar_and_the_dock() {
        let frame = rect(0.0, 0.0, 1352.0, 878.0);
        let usable = rect(0.0, 78.0, 1352.0, 771.0);
        let zoomed = rect(0.0, 79.0, 1352.0, 770.0);
        assert!(coverage_fraction(&[zoomed], frame) < rules::COVERAGE_THRESHOLD);
        assert!(coverage_fraction(&[zoomed], coverage_area(frame, usable)) >= rules::COVERAGE_THRESHOLD);
    }

    #[test]
    fn coverage_falls_back_to_the_frame_without_a_usable_area() {
        let frame = rect(0.0, 0.0, 100.0, 100.0);
        assert_eq!(coverage_area(frame, rect(0.0, 0.0, 0.0, 0.0)), frame);
        assert_eq!(coverage_area(frame, rect(500.0, 500.0, 10.0, 10.0)), frame);
    }

    #[test]
    fn coverage_is_zero_for_degenerate_screens() {
        assert!(approx(coverage_fraction(&[rect(0.0, 0.0, 1.0, 1.0)], rect(0.0, 0.0, 0.0, 10.0)), 0.0));
    }

    #[test]
    fn fullscreen_detection_allows_a_small_tolerance() {
        let screen = rect(0.0, 0.0, 1920.0, 1080.0);
        assert!(approximately_covers(rect(0.0, 0.0, 1920.0, 1080.0), screen, FULLSCREEN_TOLERANCE));
        assert!(approximately_covers(rect(1.0, -1.0, 1920.0, 1080.0), screen, FULLSCREEN_TOLERANCE));
        assert!(!approximately_covers(rect(0.0, 0.0, 1280.0, 720.0), screen, FULLSCREEN_TOLERANCE));
    }

    #[test]
    fn degenerate_rects_are_never_fullscreen() {
        let screen = rect(0.0, 0.0, 1920.0, 1080.0);
        assert!(!approximately_covers(rect(0.0, 0.0, 0.0, 0.0), screen, FULLSCREEN_TOLERANCE));
    }
}
