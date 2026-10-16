use proto::{FillMode, Geometry};

pub const ZOOM_MIN: f64 = 1.0;
pub const ZOOM_MAX: f64 = 4.0;
const EPSILON: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub w: f64,
    pub h: f64,
}

impl Size {
    pub const fn new(w: f64, h: f64) -> Self {
        return Self { w, h };
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slack {
    pub x: bool,
    pub y: bool,
    pub zoom: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub rect: Rect,
    pub slack: Slack,
}

pub fn base_scale(mode: FillMode, media: Size, screen: Size) -> Option<f64> {
    let cover = f64::max(screen.w / media.w, screen.h / media.h);
    let contain = f64::min(screen.w / media.w, screen.h / media.h);
    return match mode {
        FillMode::Fill | FillMode::Custom => Some(cover),
        FillMode::Fit => Some(contain),
        FillMode::Center => Some(1.0),
        FillMode::Stretch => None,
    };
}

pub fn resolve(mode: FillMode, geometry: Geometry, media: Size, screen: Size) -> Placement {
    let full = Rect { x: 0.0, y: 0.0, w: screen.w, h: screen.h };
    let none = Slack { x: false, y: false, zoom: false };
    if media.w <= 0.0 || media.h <= 0.0 || screen.w <= 0.0 || screen.h <= 0.0 {
        return Placement { rect: full, slack: none };
    }
    let Some(base) = base_scale(mode, media, screen) else {
        return Placement { rect: full, slack: none };
    };

    let zoom = geometry.zoom.clamp(ZOOM_MIN, ZOOM_MAX);
    let scale = base * zoom;
    let content_w = media.w * scale;
    let content_h = media.h * scale;
    let overflow_x = f64::max(content_w - screen.w, 0.0);
    let overflow_y = f64::max(content_h - screen.h, 0.0);
    let pan_x = geometry.x.clamp(-1.0, 1.0);
    let pan_y = geometry.y.clamp(-1.0, 1.0);

    let rect = Rect {
        x: (screen.w - content_w) * 0.5 + pan_x * overflow_x * 0.5,
        y: (screen.h - content_h) * 0.5 + pan_y * overflow_y * 0.5,
        w: content_w,
        h: content_h,
    };
    let slack = Slack { x: overflow_x > EPSILON, y: overflow_y > EPSILON, zoom: true };
    return Placement { rect, slack };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        return (a - b).abs() < 1e-4;
    }

    fn assert_rect(rect: Rect, x: f64, y: f64, w: f64, h: f64) {
        assert!(approx(rect.x, x), "x {} != {x}", rect.x);
        assert!(approx(rect.y, y), "y {} != {y}", rect.y);
        assert!(approx(rect.w, w), "w {} != {w}", rect.w);
        assert!(approx(rect.h, h), "h {} != {h}", rect.h);
    }

    const SCREEN: Size = Size::new(1000.0, 1000.0);
    const LANDSCAPE: Size = Size::new(1920.0, 1080.0);

    #[test]
    fn fill_covers_and_overflows_on_one_axis() {
        let placement = resolve(FillMode::Fill, Geometry::default(), LANDSCAPE, SCREEN);
        assert_rect(placement.rect, -388.888_89, 0.0, 1_777.777_78, 1000.0);
        assert_eq!(placement.slack, Slack { x: true, y: false, zoom: true });
    }

    #[test]
    fn fit_letterboxes() {
        let placement = resolve(FillMode::Fit, Geometry::default(), LANDSCAPE, SCREEN);
        assert_rect(placement.rect, 0.0, 218.75, 1000.0, 562.5);
        assert_eq!(placement.slack, Slack { x: false, y: false, zoom: true });
    }

    #[test]
    fn center_is_one_to_one() {
        let media = Size::new(400.0, 400.0);
        let screen = Size::new(800.0, 600.0);
        let placement = resolve(FillMode::Center, Geometry::default(), media, screen);
        assert_rect(placement.rect, 200.0, 100.0, 400.0, 400.0);
        assert_eq!(placement.slack, Slack { x: false, y: false, zoom: true });
    }

    #[test]
    fn stretch_is_gapless_and_has_no_slack() {
        let placement = resolve(FillMode::Stretch, Geometry::default(), LANDSCAPE, SCREEN);
        assert_rect(placement.rect, 0.0, 0.0, 1000.0, 1000.0);
        assert_eq!(placement.slack, Slack { x: false, y: false, zoom: false });
    }

    #[test]
    fn pan_reaches_both_edges_of_overflow() {
        let right = resolve(FillMode::Fill, Geometry { x: 1.0, y: 0.0, zoom: 1.0 }, LANDSCAPE, SCREEN);
        assert_rect(right.rect, 0.0, 0.0, 1_777.777_78, 1000.0);
        let left = resolve(FillMode::Fill, Geometry { x: -1.0, y: 0.0, zoom: 1.0 }, LANDSCAPE, SCREEN);
        assert_rect(left.rect, -777.777_78, 0.0, 1_777.777_78, 1000.0);
    }

    #[test]
    fn zoom_multiplies_base_scale() {
        let placement = resolve(FillMode::Fit, Geometry { x: 0.0, y: 0.0, zoom: 2.0 }, LANDSCAPE, SCREEN);
        assert_rect(placement.rect, -500.0, -62.5, 2000.0, 1125.0);
        assert_eq!(placement.slack, Slack { x: true, y: true, zoom: true });
    }

    #[test]
    fn custom_is_gapless_at_minimum_zoom() {
        let placement = resolve(FillMode::Custom, Geometry { x: 0.0, y: 0.0, zoom: 1.0 }, LANDSCAPE, SCREEN);
        assert!(placement.rect.w >= SCREEN.w - EPSILON);
        assert!(placement.rect.h >= SCREEN.h - EPSILON);
        assert_rect(placement.rect, -388.888_89, 0.0, 1_777.777_78, 1000.0);
    }

    #[test]
    fn pan_beyond_range_is_clamped() {
        let placement = resolve(FillMode::Fill, Geometry { x: 5.0, y: -5.0, zoom: 1.0 }, LANDSCAPE, SCREEN);
        assert_rect(placement.rect, 0.0, 0.0, 1_777.777_78, 1000.0);
    }

    #[test]
    fn zoom_is_clamped_to_the_documented_range() {
        let over = resolve(FillMode::Fill, Geometry { x: 0.0, y: 0.0, zoom: 99.0 }, LANDSCAPE, SCREEN);
        let max = resolve(FillMode::Fill, Geometry { x: 0.0, y: 0.0, zoom: ZOOM_MAX }, LANDSCAPE, SCREEN);
        assert!(approx(over.rect.w, max.rect.w));
        let under = resolve(FillMode::Fill, Geometry { x: 0.0, y: 0.0, zoom: -3.0 }, LANDSCAPE, SCREEN);
        let min = resolve(FillMode::Fill, Geometry { x: 0.0, y: 0.0, zoom: ZOOM_MIN }, LANDSCAPE, SCREEN);
        assert!(approx(under.rect.w, min.rect.w));
    }

    #[test]
    fn portrait_media_overflows_vertically() {
        let media = Size::new(1080.0, 1920.0);
        let placement = resolve(FillMode::Fill, Geometry::default(), media, SCREEN);
        assert_rect(placement.rect, 0.0, -388.888_89, 1000.0, 1_777.777_78);
        assert_eq!(placement.slack, Slack { x: false, y: true, zoom: true });
    }

    #[test]
    fn degenerate_inputs_yield_the_full_screen() {
        let placement = resolve(FillMode::Fill, Geometry::default(), Size::new(0.0, 0.0), SCREEN);
        assert_rect(placement.rect, 0.0, 0.0, 1000.0, 1000.0);
        assert!(!placement.slack.x && !placement.slack.y);
    }

    #[test]
    fn base_scale_matches_mode() {
        assert!(approx(base_scale(FillMode::Fill, LANDSCAPE, SCREEN).unwrap(), 1000.0 / 1080.0));
        assert!(approx(base_scale(FillMode::Fit, LANDSCAPE, SCREEN).unwrap(), 1000.0 / 1920.0));
        assert!(approx(base_scale(FillMode::Center, LANDSCAPE, SCREEN).unwrap(), 1.0));
        assert!(base_scale(FillMode::Stretch, LANDSCAPE, SCREEN).is_none());
    }
}
