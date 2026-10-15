use std::mem::size_of;
use std::ptr::{self, NonNull};

use block2::RcBlock;
use objc2_app_kit::NSWorkspace;
use objc2_core_audio::{
    AudioObjectAddPropertyListenerBlock, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
    AudioObjectPropertyAddress, AudioObjectPropertySelector, AudioObjectRemovePropertyListenerBlock,
    kAudioHardwarePropertyProcessObjectList, kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal,
    kAudioObjectSystemObject, kAudioProcessPropertyIsRunningOutput, kAudioProcessPropertyPID,
};
use objc2_core_foundation::{CFDictionary, CFNumber, CFString, CFType};
use objc2_core_graphics::{
    CGRectMakeWithDictionaryRepresentation, CGWindowListCopyWindowInfo, CGWindowListOption, kCGNullWindowID,
    kCGWindowAlpha, kCGWindowBounds, kCGWindowLayer, kCGWindowOwnerPID,
};

use crate::app::{Event, post};
use crate::rules;

pub const FULLSCREEN_TOLERANCE: f64 = 2.0;

pub struct Screen {
    pub uuid: String,
    pub frame: rules::Rect,
}

#[derive(Debug, Clone, Copy)]
pub struct Signals {
    pub display_asleep: bool,
    pub screen_locked: bool,
    pub low_power: bool,
}

pub fn snapshot(screens: &[Screen], signals: Signals) -> rules::Snapshot {
    let windows = on_screen_windows(screens);
    let own = own_pid();
    let displays = screens.iter().map(|screen| display_state(screen, &windows, own)).collect();
    return rules::Snapshot {
        displays,
        other_app_audio: external_audio_playing(),
        frontmost: frontmost_bundle(),
        running_apps: running_bundles(),
        display_asleep: signals.display_asleep,
        screen_locked: signals.screen_locked,
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

fn flip_height(screens: &[Screen]) -> f64 {
    if let Some(primary) = screens.iter().find(|screen| screen.frame.x == 0.0 && screen.frame.y == 0.0) {
        return primary.frame.h;
    }
    return screens.iter().map(|screen| screen.frame.y + screen.frame.h).fold(0.0, f64::max);
}

fn on_screen_windows(screens: &[Screen]) -> Vec<Window> {
    let flip = flip_height(screens);
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
        let Some(dictionary) = value.downcast_ref::<CFDictionary>() else { continue };
        let dictionary: &CFDictionary<CFString, CFType> = unsafe { dictionary.cast_unchecked() };
        let Some(bounds) = cf_rect(dictionary, unsafe { kCGWindowBounds }) else { continue };
        let layer = cf_i64(dictionary, unsafe { kCGWindowLayer }).unwrap_or(0);
        let pid =
            cf_i64(dictionary, unsafe { kCGWindowOwnerPID }).and_then(|value| i32::try_from(value).ok()).unwrap_or(0);
        let alpha = cf_f64(dictionary, unsafe { kCGWindowAlpha }).unwrap_or(1.0);
        let flipped = rules::Rect::new(bounds.x, flip - (bounds.y + bounds.h), bounds.w, bounds.h);
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

fn cf_i64(dictionary: &CFDictionary<CFString, CFType>, key: &CFString) -> Option<i64> {
    return dictionary.get(key).and_then(|value| value.downcast_ref::<CFNumber>().and_then(CFNumber::as_i64));
}

fn cf_f64(dictionary: &CFDictionary<CFString, CFType>, key: &CFString) -> Option<f64> {
    return dictionary.get(key).and_then(|value| value.downcast_ref::<CFNumber>().and_then(CFNumber::as_f64));
}

fn is_candidate(window: &Window, own: i32) -> bool {
    return window.pid != own && window.layer == 0 && window.alpha > 0.0;
}

fn display_state(screen: &Screen, windows: &[Window], own: i32) -> rules::DisplayState {
    let candidates: Vec<rules::Rect> =
        windows.iter().filter(|window| is_candidate(window, own)).map(|window| window.bounds).collect();
    let fullscreen = candidates.iter().any(|window| approximately_covers(*window, screen.frame, FULLSCREEN_TOLERANCE));
    let coverage = coverage_fraction(&candidates, screen.frame);
    return rules::DisplayState { display: screen.uuid.clone(), frame: screen.frame, fullscreen, coverage };
}

pub fn coverage_fraction(windows: &[rules::Rect], screen: rules::Rect) -> f64 {
    let area = screen.area();
    if area <= 0.0 {
        return 0.0;
    }
    let clipped = windows.iter().filter_map(|window| window.intersection(screen));
    return (union_area(&clipped.collect::<Vec<_>>()) / area).clamp(0.0, 1.0);
}

pub fn union_area(rects: &[rules::Rect]) -> f64 {
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

pub fn approximately_covers(window: rules::Rect, screen: rules::Rect, tolerance: f64) -> bool {
    return (window.x - screen.x).abs() <= tolerance
        && (window.y - screen.y).abs() <= tolerance
        && (window.w - screen.w).abs() <= tolerance
        && (window.h - screen.h).abs() <= tolerance;
}

fn global_address(selector: AudioObjectPropertySelector) -> AudioObjectPropertyAddress {
    return AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
}

fn system_object() -> AudioObjectID {
    return kAudioObjectSystemObject as AudioObjectID;
}

fn byte_size<T>() -> u32 {
    return u32::try_from(size_of::<T>()).unwrap_or(4);
}

fn property_size(object: AudioObjectID, address: AudioObjectPropertyAddress) -> Option<u32> {
    let mut size = 0u32;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(object, NonNull::from(&address), 0, ptr::null(), NonNull::from(&mut size))
    };
    return (status == 0).then_some(size);
}

fn property_u32(object: AudioObjectID, address: AudioObjectPropertyAddress) -> Option<u32> {
    let mut value = 0u32;
    let mut size = byte_size::<u32>();
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&address),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast(),
        )
    };
    return (status == 0).then_some(value);
}

fn property_i32(object: AudioObjectID, address: AudioObjectPropertyAddress) -> Option<i32> {
    let mut value = 0i32;
    let mut size = byte_size::<i32>();
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&address),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast(),
        )
    };
    return (status == 0).then_some(value);
}

fn process_object_list() -> Vec<AudioObjectID> {
    let object = system_object();
    let address = global_address(kAudioHardwarePropertyProcessObjectList);
    let Some(size) = property_size(object, address) else { return Vec::new() };
    let count = usize::try_from(size).unwrap_or(0) / size_of::<AudioObjectID>();
    if count == 0 {
        return Vec::new();
    }
    let mut list = vec![0u32; count];
    let mut size = size;
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&address),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut list[0]).cast(),
        )
    };
    if status != 0 {
        return Vec::new();
    }
    return list;
}

fn process_pid(object: AudioObjectID) -> Option<i32> {
    return property_i32(object, global_address(kAudioProcessPropertyPID));
}

fn process_running_output(object: AudioObjectID) -> bool {
    return property_u32(object, global_address(kAudioProcessPropertyIsRunningOutput)).unwrap_or(0) != 0;
}

pub fn external_audio_playing() -> bool {
    let own = own_pid();
    return process_object_list().into_iter().any(|object| {
        return process_pid(object).is_some_and(|pid| pid != own) && process_running_output(object);
    });
}

pub fn frontmost_bundle() -> Option<String> {
    return NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .and_then(|application| application.bundleIdentifier())
        .map(|identifier| identifier.to_string());
}

pub fn running_bundles() -> Vec<String> {
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

type ListenerBlock = RcBlock<dyn Fn(u32, NonNull<AudioObjectPropertyAddress>)>;

struct AudioListener {
    object: AudioObjectID,
    address: AudioObjectPropertyAddress,
    block: ListenerBlock,
}

impl AudioListener {
    fn add(object: AudioObjectID, selector: AudioObjectPropertySelector) -> Option<Self> {
        let address = global_address(selector);
        let block: ListenerBlock = RcBlock::new(|_count: u32, _addresses: NonNull<AudioObjectPropertyAddress>| {
            post(Event::AudioChanged);
        });
        let status = unsafe {
            AudioObjectAddPropertyListenerBlock(
                object,
                NonNull::from(&address),
                Some(dispatch2::DispatchQueue::main()),
                RcBlock::as_ptr(&block),
            )
        };
        if status != 0 {
            return None;
        }
        return Some(Self { object, address, block });
    }

    fn remove(&self) {
        unsafe {
            AudioObjectRemovePropertyListenerBlock(
                self.object,
                NonNull::from(&self.address),
                Some(dispatch2::DispatchQueue::main()),
                RcBlock::as_ptr(&self.block),
            );
        }
    }
}

pub struct AudioMonitor {
    processes: Vec<(AudioObjectID, AudioListener)>,
    process_list: Option<AudioListener>,
}

impl AudioMonitor {
    pub fn install() -> Self {
        let mut monitor = Self {
            processes: Vec::new(),
            process_list: AudioListener::add(system_object(), kAudioHardwarePropertyProcessObjectList),
        };
        monitor.rebind();
        return monitor;
    }

    pub fn rebind(&mut self) {
        let objects = process_object_list();
        for (object, listener) in &self.processes {
            if !objects.contains(object) {
                listener.remove();
            }
        }
        self.processes.retain(|(object, _)| objects.contains(object));
        let existing: Vec<AudioObjectID> = self.processes.iter().map(|(object, _)| *object).collect();
        for object in objects {
            if existing.contains(&object) {
                continue;
            }
            if let Some(listener) = AudioListener::add(object, kAudioProcessPropertyIsRunningOutput) {
                self.processes.push((object, listener));
            }
        }
    }
}

impl Drop for AudioMonitor {
    fn drop(&mut self) {
        for (_, listener) in &self.processes {
            listener.remove();
        }
        if let Some(listener) = &self.process_list {
            listener.remove();
        }
    }
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
