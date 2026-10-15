use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGColor, CGMutablePath};
use objc2_foundation::{NSNumber, NSString, NSValue};
use objc2_quartz_core::{
    CABasicAnimation, CALayer, CAMediaTiming, CAMediaTimingFunction, CAShapeLayer, CATransaction, kCAFillRuleEvenOdd,
    kCAFillRuleNonZero, kCAMediaTimingFunctionLinear,
};
use proto::{Axis, Pos, Transition, TransitionKind};

const CIRCLE_SEGMENTS: usize = 96;
const WAVE_SAMPLES_PER_PERIOD: f64 = 16.0;
const WAVE_SAMPLES_MAX: usize = 8192;
const TIMER_MARGIN: f64 = 0.08;

pub struct ResolvedTransition {
    pub kind: TransitionKind,
    pub duration: f64,
    pub bezier: [f64; 4],
    pub angle: f64,
    pub pos: Pos,
    pub invert_y: bool,
    pub wave: [f64; 2],
}

impl ResolvedTransition {
    pub fn instant(&self) -> bool {
        return self.kind == TransitionKind::None || self.duration <= 0.0;
    }

    pub fn timer_seconds(&self) -> f64 {
        return self.duration + TIMER_MARGIN;
    }
}

pub fn resolve(settings: &Transition, request: Option<&Transition>) -> ResolvedTransition {
    return ResolvedTransition {
        kind: pick(request, settings, |value| value.kind).unwrap_or(TransitionKind::Fade),
        duration: pick(request, settings, |value| value.duration).unwrap_or(Transition::DEFAULT_DURATION),
        bezier: pick(request, settings, |value| value.bezier).unwrap_or(Transition::DEFAULT_BEZIER),
        angle: pick(request, settings, |value| value.angle).unwrap_or(Transition::DEFAULT_ANGLE),
        pos: pick(request, settings, |value| value.pos).unwrap_or_else(Pos::center),
        invert_y: pick(request, settings, |value| value.invert_y).unwrap_or(false),
        wave: pick(request, settings, |value| value.wave).unwrap_or(Transition::DEFAULT_WAVE),
    };
}

fn pick<T: Copy>(
    request: Option<&Transition>,
    settings: &Transition,
    field: impl Fn(&Transition) -> Option<T>,
) -> Option<T> {
    return request.and_then(&field).or_else(|| field(settings));
}

pub fn apply(layer: &CALayer, spec: &ResolvedTransition) {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    decorate(layer, spec);
    CATransaction::commit();
}

fn decorate(layer: &CALayer, spec: &ResolvedTransition) {
    let kind = match spec.kind {
        TransitionKind::Random => random_kind(),
        other => other,
    };
    let size = layer.bounds().size;
    match kind {
        TransitionKind::None | TransitionKind::Random => {}
        TransitionKind::Simple => crossfade(layer, spec, true),
        TransitionKind::Fade => crossfade(layer, spec, false),
        TransitionKind::Left => sweep(layer, spec, 180.0, false),
        TransitionKind::Right => sweep(layer, spec, 0.0, false),
        TransitionKind::Top => sweep(layer, spec, 90.0, false),
        TransitionKind::Bottom => sweep(layer, spec, 270.0, false),
        TransitionKind::Wipe => sweep(layer, spec, spec.angle, false),
        TransitionKind::Wave => sweep(layer, spec, spec.angle, true),
        TransitionKind::Grow => reveal_circle(layer, spec, size, point(&spec.pos, size, spec.invert_y), false),
        TransitionKind::Center => reveal_circle(layer, spec, size, (size.width / 2.0, size.height / 2.0), false),
        TransitionKind::Any => reveal_circle(layer, spec, size, random_point(size), false),
        TransitionKind::Outer => reveal_circle(layer, spec, size, point(&spec.pos, size, spec.invert_y), true),
    }
}

fn crossfade(layer: &CALayer, spec: &ResolvedTransition, linear: bool) {
    let animation = basic("opacity", spec, linear);
    let from = NSNumber::numberWithFloat(0.0);
    let to = NSNumber::numberWithFloat(1.0);
    unsafe {
        animation.setFromValue(Some(object(&*from)));
        animation.setToValue(Some(object(&*to)));
    }
    layer.addAnimation_forKey(AsRef::<objc2_quartz_core::CAAnimation>::as_ref(&*animation), None);
}

fn sweep(layer: &CALayer, spec: &ResolvedTransition, angle: f64, wavy: bool) {
    let size = layer.bounds().size;
    let direction = direction(angle);
    let normal = (-direction.1, direction.0);
    let diagonal = (size.width * size.width + size.height * size.height).sqrt();
    let (amplitude, period) = if wavy { (spec.wave[1], spec.wave[0].max(1.0)) } else { (0.0, 1.0) };
    let extent = diagonal + amplitude;
    let path = edge_path(direction, normal, extent, amplitude, period, wavy);

    let (low, high) = projection_range(size, direction);
    let start = offset(direction, low - amplitude);
    let end = offset(direction, high + amplitude);

    let mask = CAShapeLayer::layer();
    mask.setAnchorPoint(CGPoint::new(0.0, 0.0));
    mask.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(1.0, 1.0)));
    mask.setPosition(end);
    let fill = fill_color();
    mask.setFillColor(Some(&*fill));
    mask.setFillRule(unsafe { kCAFillRuleNonZero });
    mask.setPath(Some(&**path));

    let animation = basic("position", spec, false);
    let from = unsafe { NSValue::valueWithPoint(start) };
    let to = unsafe { NSValue::valueWithPoint(end) };
    unsafe {
        animation.setFromValue(Some(object(&*from)));
        animation.setToValue(Some(object(&*to)));
    }
    mask.addAnimation_forKey(
        AsRef::<objc2_quartz_core::CAAnimation>::as_ref(&*animation),
        Some(&NSString::from_str("sweep")),
    );
    unsafe { layer.setMask(Some(&mask)) };
}

fn reveal_circle(layer: &CALayer, spec: &ResolvedTransition, size: CGSize, center: (f64, f64), outer: bool) {
    let radius = max_radius(center, size);
    let small = (radius * 0.001).max(0.5);
    let (from_radius, to_radius) = if outer { (radius, small) } else { (small, radius) };
    let from = circle_path(size, center, from_radius, outer);
    let to = circle_path(size, center, to_radius, outer);

    let mask = CAShapeLayer::layer();
    mask.setAnchorPoint(CGPoint::new(0.0, 0.0));
    mask.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(1.0, 1.0)));
    mask.setPosition(CGPoint::ZERO);
    let fill = fill_color();
    mask.setFillColor(Some(&*fill));
    mask.setFillRule(unsafe { if outer { kCAFillRuleEvenOdd } else { kCAFillRuleNonZero } });
    mask.setPath(Some(&**to));

    let animation = basic("path", spec, false);
    unsafe {
        animation.setFromValue(Some(object(&*from)));
        animation.setToValue(Some(object(&*to)));
    }
    mask.addAnimation_forKey(
        AsRef::<objc2_quartz_core::CAAnimation>::as_ref(&*animation),
        Some(&NSString::from_str("reveal")),
    );
    unsafe { layer.setMask(Some(&mask)) };
}

fn basic(key: &str, spec: &ResolvedTransition, linear: bool) -> Retained<CABasicAnimation> {
    let animation = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str(key)));
    animation.setDuration(spec.duration);
    animation.setTimingFunction(Some(&timing_function(spec, linear)));
    return animation;
}

fn timing_function(spec: &ResolvedTransition, linear: bool) -> Retained<CAMediaTimingFunction> {
    if linear {
        return unsafe { CAMediaTimingFunction::functionWithName(kCAMediaTimingFunctionLinear) };
    }
    let [c1x, c1y, c2x, c2y] = spec.bezier;
    return CAMediaTimingFunction::functionWithControlPoints(
        to_float(c1x),
        to_float(c1y),
        to_float(c2x),
        to_float(c2y),
    );
}

#[allow(clippy::cast_precision_loss, clippy::many_single_char_names)]
fn edge_path(
    direction: (f64, f64),
    normal: (f64, f64),
    extent: f64,
    amplitude: f64,
    period: f64,
    wavy: bool,
) -> CFRetained<CGMutablePath> {
    let path = CGMutablePath::new();
    let samples = if wavy { wave_samples(extent, period) } else { 2 };
    let point_at = |along: f64, across: f64| {
        CGPoint::new(along * direction.0 + across * normal.0, along * direction.1 + across * normal.1)
    };
    let edge = |across: f64| {
        if !wavy {
            return 0.0;
        }
        return amplitude * (std::f64::consts::TAU * across / period).sin();
    };
    let transform = std::ptr::null();
    unsafe {
        let first = point_at(edge(-extent), -extent);
        CGMutablePath::move_to_point(Some(&path), transform, first.x, first.y);
        for index in 1..=samples {
            let across = -extent + 2.0 * extent * (index as f64) / (samples as f64);
            let point = point_at(edge(across), across);
            CGMutablePath::add_line_to_point(Some(&path), transform, point.x, point.y);
        }
        let top = point_at(-extent, extent);
        CGMutablePath::add_line_to_point(Some(&path), transform, top.x, top.y);
        let bottom = point_at(-extent, -extent);
        CGMutablePath::add_line_to_point(Some(&path), transform, bottom.x, bottom.y);
        CGMutablePath::close_subpath(Some(&path));
    }
    return path;
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn wave_samples(extent: f64, period: f64) -> usize {
    let wanted = (2.0 * extent / period * WAVE_SAMPLES_PER_PERIOD).ceil();
    return (wanted as usize).clamp(2, WAVE_SAMPLES_MAX);
}

#[allow(clippy::cast_precision_loss)]
fn circle_path(size: CGSize, center: (f64, f64), radius: f64, outer: bool) -> CFRetained<CGMutablePath> {
    let path = CGMutablePath::new();
    let transform = std::ptr::null();
    unsafe {
        if outer {
            CGMutablePath::add_rect(Some(&path), transform, CGRect::new(CGPoint::ZERO, size));
        }
        let first = arc_point(center, radius, 0.0);
        CGMutablePath::move_to_point(Some(&path), transform, first.0, first.1);
        for index in 1..CIRCLE_SEGMENTS {
            let angle = std::f64::consts::TAU * (index as f64) / (CIRCLE_SEGMENTS as f64);
            let point = arc_point(center, radius, angle);
            CGMutablePath::add_line_to_point(Some(&path), transform, point.0, point.1);
        }
        CGMutablePath::close_subpath(Some(&path));
    }
    return path;
}

fn arc_point(center: (f64, f64), radius: f64, angle: f64) -> (f64, f64) {
    return (center.0 + radius * angle.cos(), center.1 + radius * angle.sin());
}

fn fill_color() -> CFRetained<CGColor> {
    return CGColor::new_generic_gray(0.0, 1.0);
}

fn object<T: AsRef<AnyObject> + ?Sized>(value: &T) -> &AnyObject {
    return value.as_ref();
}

#[allow(clippy::cast_possible_truncation)]
fn to_float(value: f64) -> f32 {
    return value as f32;
}

fn direction(angle: f64) -> (f64, f64) {
    let radians = angle.to_radians();
    return (-radians.cos(), -radians.sin());
}

fn offset(direction: (f64, f64), distance: f64) -> CGPoint {
    return CGPoint::new(direction.0 * distance, direction.1 * distance);
}

fn projection_range(size: CGSize, direction: (f64, f64)) -> (f64, f64) {
    let corners = [(0.0, 0.0), (size.width, 0.0), (size.width, size.height), (0.0, size.height)];
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for (x, y) in corners {
        let projection = x * direction.0 + y * direction.1;
        low = low.min(projection);
        high = high.max(projection);
    }
    return (low, high);
}

fn max_radius(center: (f64, f64), size: CGSize) -> f64 {
    let corners = [(0.0, 0.0), (size.width, 0.0), (size.width, size.height), (0.0, size.height)];
    let mut radius: f64 = 0.0;
    for (x, y) in corners {
        let dx = x - center.0;
        let dy = y - center.1;
        radius = radius.max((dx * dx + dy * dy).sqrt());
    }
    return radius;
}

fn point(pos: &Pos, size: CGSize, invert_y: bool) -> (f64, f64) {
    let x = axis_value(pos.x, size.width);
    let raw = axis_value(pos.y, size.height);
    let y = if invert_y { size.height - raw } else { raw };
    return (x, y);
}

fn axis_value(axis: Axis, extent: f64) -> f64 {
    return match axis {
        Axis::Fraction(fraction) => fraction * extent,
        Axis::Pixels(pixels) => pixels_to_f64(pixels),
    };
}

#[allow(clippy::cast_precision_loss)]
fn pixels_to_f64(pixels: i64) -> f64 {
    return pixels as f64;
}

fn random_point(size: CGSize) -> (f64, f64) {
    return (random_unit() * size.width, random_unit() * size.height);
}

fn random_kind() -> TransitionKind {
    let pool = TransitionKind::RANDOM_POOL;
    let index = random_index(pool.len());
    return pool[index];
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
fn random_index(len: usize) -> usize {
    let seed = random_unit();
    if len == 0 {
        return 0;
    }
    return ((seed * len as f64) as usize).min(len - 1);
}

fn random_unit() -> f64 {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.subsec_nanos());
    return f64::from(seed) / 1_000_000_000.0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::transition;

    fn approx(a: f64, b: f64) -> bool {
        return (a - b).abs() < 1e-9;
    }

    #[test]
    fn resolve_falls_back_to_defaults() {
        let spec = transition::resolve(&Transition::default(), None);
        assert_eq!(spec.kind, TransitionKind::Fade);
        assert!(approx(spec.duration, 1.0));
        assert!(spec.bezier.iter().zip(Transition::DEFAULT_BEZIER).all(|(a, b)| approx(*a, b)));
        assert!(approx(spec.angle, 45.0));
        assert_eq!(spec.pos, Pos::center());
        assert!(!spec.invert_y);
        assert!(spec.wave.iter().zip(Transition::DEFAULT_WAVE).all(|(a, b)| approx(*a, b)));
    }

    #[test]
    fn resolve_prefers_request_then_settings() {
        let settings = Transition { kind: Some(TransitionKind::Wipe), duration: Some(2.0), ..Transition::default() };
        let request = Transition { duration: Some(0.5), angle: Some(90.0), ..Transition::default() };
        let spec = transition::resolve(&settings, Some(&request));
        assert_eq!(spec.kind, TransitionKind::Wipe);
        assert!(approx(spec.duration, 0.5));
        assert!(approx(spec.angle, 90.0));
    }

    #[test]
    fn instant_only_when_none_or_zero_duration() {
        let mut spec = transition::resolve(&Transition::default(), None);
        assert!(!spec.instant());
        spec.kind = TransitionKind::None;
        assert!(spec.instant());
        spec.kind = TransitionKind::Fade;
        spec.duration = 0.0;
        assert!(spec.instant());
    }

    #[test]
    fn angle_zero_sweeps_right_to_left() {
        let dir = transition::direction(0.0);
        assert!(approx(dir.0, -1.0));
        assert!(approx(dir.1, 0.0));
    }

    #[test]
    fn angle_ninety_sweeps_top_to_bottom() {
        let dir = transition::direction(90.0);
        assert!(approx(dir.0, 0.0));
        assert!(approx(dir.1, -1.0));
    }

    #[test]
    fn angle_two_seventy_sweeps_bottom_to_top() {
        let dir = transition::direction(270.0);
        assert!(approx(dir.0, 0.0));
        assert!(approx(dir.1, 1.0));
    }

    #[test]
    fn point_converts_fractions_pixels_and_aliases() {
        let size = CGSize::new(200.0, 100.0);
        assert_eq!(transition::point(&Pos::center(), size, false), (100.0, 50.0));
        assert_eq!(transition::point(&Pos::parse("top-right").unwrap(), size, false), (200.0, 100.0));
        assert_eq!(transition::point(&Pos::parse("10,20").unwrap(), size, false), (10.0, 20.0));
        assert_eq!(transition::point(&Pos::parse("0.0,0.0").unwrap(), size, true), (0.0, 100.0));
    }

    #[test]
    fn projection_range_spans_the_whole_screen() {
        let size = CGSize::new(100.0, 40.0);
        let (low, high) = transition::projection_range(size, transition::direction(0.0));
        assert!(approx(low, -100.0));
        assert!(approx(high, 0.0));
    }

    #[test]
    fn max_radius_reaches_the_farthest_corner() {
        let size = CGSize::new(100.0, 100.0);
        assert!(approx(transition::max_radius((50.0, 50.0), size), 50.0 * 2.0_f64.sqrt()));
    }

    #[test]
    fn random_kind_never_picks_none_or_random() {
        for _ in 0..64 {
            let kind = transition::random_kind();
            assert_ne!(kind, TransitionKind::None);
            assert_ne!(kind, TransitionKind::Random);
        }
    }
}
