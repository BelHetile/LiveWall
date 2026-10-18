use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityButtonRole, NSAccessibilitySliderRole, NSBezierPath, NSButton, NSColor,
    NSControlSize, NSControlTextEditingDelegate, NSEvent, NSImage, NSImageView, NSLayoutAttribute, NSPopover,
    NSPopoverBehavior, NSSlider, NSTextField, NSTextFieldDelegate, NSView, NSViewController,
    NSViewLayerContentsRedrawPolicy,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGImageAlphaInfo, CGImageByteOrderInfo,
    kCGColorSpaceSRGB,
};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSRectEdge, NSSize};
use objc2_quartz_core::{CALayer, CATransaction, CATransform3D, CATransform3DIdentity};

use crate::i18n::tr;
use crate::style::{Key, Rgba, Tag};
use crate::ui::controls::{self, ControlView};
use crate::ui::metrics::{self, Text};
use crate::ui::theme::{self, Theme};
use crate::ui::widgets::{self, IconRole, ns, resolving};

const RING_OFFSET: CGFloat = 3.0;
const RING_WIDTH: CGFloat = 2.0;
const TAG_DIAMETER: CGFloat = 20.0;
const FIELD_WIDTH: CGFloat = 96.0;
const WHEEL_DIAMETER: CGFloat = 160.0;
const MARKER_DIAMETER: CGFloat = 16.0;
const MARKER_RING: CGFloat = 2.0;
const MARKER_SHADOW_OPACITY: f32 = 0.3;
const MARKER_SHADOW_RADIUS: CGFloat = 2.0;
const MARKER_SHADOW_LIFT: CGFloat = 1.0;
const PANEL_MIN: CGFloat = 192.0;
const HUE_STEP: f64 = 3.0;
const SAT_STEP: f64 = 0.05;
const BYTES_PER_PIXEL: usize = 4;
const LEFT_KEY: u16 = 123;
const RIGHT_KEY: u16 = 124;
const DOWN_KEY: u16 = 125;
const UP_KEY: u16 = 126;

pub const RING_PAD: CGFloat = RING_OFFSET + RING_WIDTH;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Tag(Tag),
    Hex(Rgba),
}

impl Pick {
    pub fn parse(text: &str) -> Option<Self> {
        if let Some(tag) = Tag::parse(text) {
            return Some(Self::Tag(tag));
        }
        return Rgba::parse(text).map(Self::Hex);
    }

    pub fn rgba(self, dark: bool) -> Rgba {
        return match self {
            Self::Tag(tag) if dark => tag.reference().derive_dark(),
            Self::Tag(tag) => tag.reference(),
            Self::Hex(color) => color,
        };
    }
}

fn parse_hex(text: &str) -> Option<Rgba> {
    let text = text.trim();
    let digits = text.strip_prefix('#').unwrap_or(text);
    if digits.len() != 6 {
        return None;
    }
    return Rgba::parse(digits);
}

fn contains(rect: CGRect, point: CGPoint) -> bool {
    return point.x >= rect.origin.x
        && point.x <= rect.origin.x + rect.size.width
        && point.y >= rect.origin.y
        && point.y <= rect.origin.y + rect.size.height;
}

fn scaled(value: f64) -> CATransform3D {
    return CATransform3D { m11: value, m22: value, m33: 1.0, m44: 1.0, ..unsafe { CATransform3DIdentity } };
}

type Press = Rc<dyn Fn(&Dot)>;

pub struct DotIvars {
    diameter: CGFloat,
    inset: CGFloat,
    ringed: bool,
    glyph: Option<&'static str>,
    disc: Retained<CALayer>,
    halo: Retained<CALayer>,
    mark: Retained<NSImageView>,
    marked: Cell<Option<&'static str>>,
    fill: RefCell<Option<Retained<NSColor>>>,
    rim: RefCell<Option<Retained<NSColor>>>,
    empty: Retained<NSColor>,
    edge: Retained<NSColor>,
    ring: Retained<NSColor>,
    plain: Retained<NSColor>,
    control: controls::Control,
    action: RefCell<Option<Press>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = DotIvars]
    pub struct Dot;

    impl Dot {
        #[unsafe(method(wantsUpdateLayer))]
        fn wants_update_layer(&self) -> bool {
            true
        }

        #[unsafe(method(updateLayer))]
        fn update_layer(&self) {
            self.colorize();
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            self.ivars().control.enabled()
        }

        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: CGPoint) -> Option<Retained<NSView>> {
            if self.isHidden() || !contains(self.frame(), point) {
                None
            } else {
                Some(Retained::into_super(self.retain()))
            }
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            controls::mouse_down(self);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            controls::mouse_dragged(self, event);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            controls::mouse_up(self, event);
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn did_move_to_window(&self) {
            let () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            controls::window_changed(self);
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            controls::entered(self);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            controls::exited(self);
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if controls::key_event(event) {
                self.fire();
                return;
            }
            let _: () = unsafe { msg_send![super(self), keyDown: event] };
        }

        #[unsafe(method(drawFocusRingMask))]
        fn draw_focus_ring_mask(&self) {
            NSBezierPath::bezierPathWithOvalInRect(self.disc_rect()).fill();
        }

        #[unsafe(method(focusRingMaskBounds))]
        fn focus_ring_mask_bounds(&self) -> CGRect {
            self.disc_rect()
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_perform_press(&self) -> bool {
            let enabled = self.ivars().control.enabled();
            if enabled {
                self.fire();
            }
            enabled
        }
    }
);

impl ControlView for Dot {
    fn view(&self) -> &NSView {
        self
    }

    fn control(&self) -> &controls::Control {
        &self.ivars().control
    }

    fn apply(&self, immediate: bool) {
        self.sync_state(immediate);
    }

    fn activate(&self) {
        self.fire();
    }
}

impl Dot {
    pub fn new(
        mtm: MainThreadMarker,
        diameter: CGFloat,
        inset: CGFloat,
        ringed: bool,
        glyph: Option<&'static str>,
        theme: &Theme,
    ) -> Retained<Self> {
        let size = diameter + inset * 2.0;
        let mark = NSImageView::new(mtm);
        let this = Self::alloc(mtm).set_ivars(DotIvars {
            diameter,
            inset,
            ringed,
            glyph,
            disc: CALayer::new(),
            halo: CALayer::new(),
            mark: mark.clone(),
            marked: Cell::new(None),
            fill: RefCell::new(None),
            rim: RefCell::new(None),
            empty: theme.color(Key::FieldFill),
            edge: theme.color(Key::Hairline),
            ring: theme.color(Key::Selection),
            plain: theme.color(Key::TextSecondary),
            control: controls::Control::new(),
            action: RefCell::new(None),
        });
        let this: Retained<Self> =
            unsafe { msg_send![super(this), initWithFrame: CGRect::new(CGPoint::ZERO, CGSize::new(size, size))] };
        this.setWantsLayer(true);
        this.setLayerContentsRedrawPolicy(NSViewLayerContentsRedrawPolicy::OnSetNeedsDisplay);
        let ivars = this.ivars();
        let disc = this.disc_rect();
        let reach = RING_PAD;
        let outer = CGRect::new(
            CGPoint::new(disc.origin.x - reach, disc.origin.y - reach),
            CGSize::new(diameter + reach * 2.0, diameter + reach * 2.0),
        );
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        ivars.disc.setFrame(disc);
        ivars.disc.setCornerRadius(metrics::capsule(diameter));
        ivars.halo.setFrame(outer);
        ivars.halo.setCornerRadius(metrics::capsule(outer.size.width));
        ivars.halo.setBorderWidth(RING_WIDTH);
        ivars.halo.setOpacity(0.0);
        if let Some(layer) = this.layer() {
            layer.addSublayer(&ivars.halo);
            layer.addSublayer(&ivars.disc);
        }
        CATransaction::commit();
        mark.setFrame(disc);
        mark.setContentTintColor(Some(&ivars.plain));
        mark.setHidden(true);
        this.addSubview(&mark);
        controls::track(&this, &this);
        this.setAccessibilityElement(true);
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
        this.sync_state(true);
        this.setNeedsDisplay(true);
        return this;
    }

    pub fn fit(&self) {
        let size = self.frame().size;
        self.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.widthAnchor().constraintEqualToConstant(size.width).setActive(true);
        self.heightAnchor().constraintEqualToConstant(size.height).setActive(true);
    }

    pub fn on_press(&self, action: impl Fn(&Dot) + 'static) {
        *self.ivars().action.borrow_mut() = Some(Rc::new(action));
    }

    pub fn set_fill(&self, fill: Option<&NSColor>) {
        *self.ivars().fill.borrow_mut() = fill.map(Retained::from);
        self.sync_state(true);
        self.setNeedsDisplay(true);
    }

    pub fn set_rim(&self, rim: Option<&NSColor>) {
        *self.ivars().rim.borrow_mut() = rim.map(Retained::from);
        self.setNeedsDisplay(true);
    }

    pub fn set_selected(&self, selected: bool) {
        controls::set_selected(self, selected);
    }

    pub fn set_enabled(&self, enabled: bool) {
        controls::set_enabled(self, enabled);
    }

    fn disc_rect(&self) -> CGRect {
        let ivars = self.ivars();
        return CGRect::new(CGPoint::new(ivars.inset, ivars.inset), CGSize::new(ivars.diameter, ivars.diameter));
    }

    fn fire(&self) {
        let action = self.ivars().action.borrow().clone();
        if let Some(action) = action {
            action(self);
        }
    }

    fn sync_state(&self, immediate: bool) {
        let ivars = self.ivars();
        let control = &ivars.control;
        let selected = control.selected() && ivars.ringed;
        let pressed = control.pressed();
        let hovered = control.hovered() && control.enabled();
        let halo = if selected {
            1.0
        } else if pressed {
            metrics::EDGE_PRESSED
        } else if hovered {
            metrics::EDGE_HOVER
        } else {
            0.0
        };
        let scale = if pressed { metrics::scale(metrics::SELECT_SCALE) } else { 1.0 };
        CATransaction::begin();
        if immediate || pressed {
            CATransaction::setDisableActions(true);
        } else {
            CATransaction::setAnimationDuration(metrics::MOTION_QUICK);
            CATransaction::setAnimationTimingFunction(Some(&metrics::ease_out()));
        }
        ivars.halo.setOpacity(halo);
        ivars.disc.setTransform(scaled(scale));
        CATransaction::commit();
        let empty = ivars.fill.borrow().is_none();
        let name = if selected { Some("checkmark") } else { ivars.glyph.filter(|_| empty) };
        if ivars.marked.replace(name) != name {
            ivars.mark.setImage(name.and_then(|name| widgets::icon(name, IconRole::Small)).as_deref());
        }
        ivars.mark.setHidden(name.is_none());
    }

    fn colorize(&self) {
        let ivars = self.ivars();
        let fill = ivars.fill.borrow().clone();
        let rim = ivars.rim.borrow().clone();
        resolving(self, || {
            let base = fill.as_deref().unwrap_or(&ivars.empty);
            ivars.disc.setBackgroundColor(Some(&base.CGColor()));
            let (width, border) = match &rim {
                Some(rim) => (metrics::BORDER_RING, rim),
                None => (metrics::BORDER_HAIRLINE, &ivars.edge),
            };
            ivars.disc.setBorderWidth(width);
            ivars.disc.setBorderColor(Some(&border.CGColor()));
            ivars.halo.setBorderColor(Some(&ivars.ring.CGColor()));
            let ink = fill
                .as_deref()
                .and_then(theme::rgba)
                .map_or_else(|| ivars.plain.clone(), |color| theme::ns_color(color.on()));
            ivars.mark.setContentTintColor(Some(&ink));
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Hsv {
    h: f64,
    s: f64,
    v: f64,
}

fn rgba_to_hsv(color: Rgba) -> Hsv {
    let [r, g, b, _] = color.components();
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let sector = if delta <= 0.0 {
        0.0
    } else if r >= g && r >= b {
        (g - b) / delta
    } else if g >= b {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    let sat = if max <= 0.0 { 0.0 } else { delta / max };
    return Hsv { h: (sector * 60.0).rem_euclid(360.0), s: sat, v: max };
}

fn hsv_to_rgba(hsv: Hsv) -> Rgba {
    let hue = hsv.h.rem_euclid(360.0);
    let chroma = hsv.v * hsv.s;
    let floor = hsv.v - chroma;
    let turn = (hue / 60.0).rem_euclid(2.0) - 1.0;
    let slope = chroma * (1.0 - turn.abs()) + floor;
    let (r, g, b) = if hue < 60.0 {
        (hsv.v, slope, floor)
    } else if hue < 120.0 {
        (slope, hsv.v, floor)
    } else if hue < 180.0 {
        (floor, hsv.v, slope)
    } else if hue < 240.0 {
        (floor, slope, hsv.v)
    } else if hue < 300.0 {
        (slope, floor, hsv.v)
    } else {
        (hsv.v, floor, slope)
    };
    return Rgba::unit(r, g, b, 1.0);
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_sign_loss)]
fn paint_disc(pixels: &mut [u8], side: usize) {
    let half = side as f64 / 2.0;
    for (row, strip) in pixels.chunks_exact_mut(side * BYTES_PER_PIXEL).enumerate() {
        let py = (row as f64 + 0.5) / half - 1.0;
        for (column, pixel) in strip.chunks_exact_mut(BYTES_PER_PIXEL).enumerate() {
            let px = (column as f64 + 0.5) / half - 1.0;
            let reach = px.hypot(py);
            let coverage = ((1.0 - reach) * half + 0.5).clamp(0.0, 1.0);
            if coverage <= 0.0 {
                continue;
            }
            let color = hsv_to_rgba(Hsv { h: (-py).atan2(px).to_degrees(), s: reach.min(1.0), v: 1.0 });
            for (channel, value) in [color.r, color.g, color.b, color.a].into_iter().enumerate() {
                pixel[channel] = (f64::from(value) * coverage) as u8;
            }
        }
    }
}

fn disc_image(side: usize, diameter: CGFloat) -> Option<Retained<NSImage>> {
    if side == 0 {
        return None;
    }
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
    let mut pixels = vec![0u8; side * side * BYTES_PER_PIXEL];
    paint_disc(&mut pixels, side);
    let info = CGImageByteOrderInfo::Order32Big.0 | CGImageAlphaInfo::PremultipliedLast.0;
    let context = unsafe {
        CGBitmapContextCreate(pixels.as_mut_ptr().cast(), side, side, 8, side * BYTES_PER_PIXEL, Some(&space), info)
    }?;
    let image = CGBitmapContextCreateImage(Some(&context))?;
    drop(context);
    return Some(NSImage::initWithCGImage_size(NSImage::alloc(), &image, NSSize::new(diameter, diameter)));
}

type Moved = Rc<dyn Fn(&Wheel)>;

pub struct WheelIvars {
    control: controls::Control,
    disc: Retained<CALayer>,
    marker: Retained<CALayer>,
    built: Cell<(usize, CGFloat)>,
    hue: Cell<f64>,
    sat: Cell<f64>,
    bright: Cell<f64>,
    moved: RefCell<Option<Moved>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = WheelIvars]
    pub struct Wheel;

    impl Wheel {
        #[unsafe(method(wantsUpdateLayer))]
        fn wants_update_layer(&self) -> bool {
            true
        }

        #[unsafe(method(updateLayer))]
        fn update_layer(&self) {
            self.place_marker();
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            self.ivars().control.enabled()
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            controls::mouse_down(self);
            self.drag(event);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            controls::mouse_dragged(self, event);
            self.drag(event);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            controls::mouse_up(self, event);
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn did_move_to_window(&self) {
            let () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            controls::window_changed(self);
            self.refresh_image();
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let delta = match event.keyCode() {
                LEFT_KEY => (-HUE_STEP, 0.0),
                RIGHT_KEY => (HUE_STEP, 0.0),
                UP_KEY => (0.0, SAT_STEP),
                DOWN_KEY => (0.0, -SAT_STEP),
                _ => {
                    let _: () = unsafe { msg_send![super(self), keyDown: event] };
                    return;
                }
            };
            self.nudge(delta.0, delta.1);
        }

        #[unsafe(method(drawFocusRingMask))]
        fn draw_focus_ring_mask(&self) {
            NSBezierPath::bezierPathWithOvalInRect(self.bounds()).fill();
        }

        #[unsafe(method(focusRingMaskBounds))]
        fn focus_ring_mask_bounds(&self) -> CGRect {
            self.bounds()
        }

        #[unsafe(method(accessibilityPerformIncrement))]
        fn accessibility_perform_increment(&self) -> bool {
            self.nudge(HUE_STEP, 0.0);
            true
        }

        #[unsafe(method(accessibilityPerformDecrement))]
        fn accessibility_perform_decrement(&self) -> bool {
            self.nudge(-HUE_STEP, 0.0);
            true
        }
    }
);

impl ControlView for Wheel {
    fn view(&self) -> &NSView {
        self
    }

    fn control(&self) -> &controls::Control {
        &self.ivars().control
    }

    fn apply(&self, _immediate: bool) {
        self.setNeedsDisplay(true);
    }

    fn activate(&self) {}
}

impl Wheel {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(WheelIvars {
            control: controls::Control::new(),
            disc: CALayer::new(),
            marker: CALayer::new(),
            built: Cell::new((0, 0.0)),
            hue: Cell::new(0.0),
            sat: Cell::new(1.0),
            bright: Cell::new(1.0),
            moved: RefCell::new(None),
        });
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithFrame: CGRect::new(CGPoint::ZERO, CGSize::new(WHEEL_DIAMETER, WHEEL_DIAMETER))]
        };
        this.setWantsLayer(true);
        this.setLayerContentsRedrawPolicy(NSViewLayerContentsRedrawPolicy::OnSetNeedsDisplay);
        let ivars = this.ivars();
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        if let Some(layer) = this.layer() {
            layer.addSublayer(&ivars.disc);
            layer.addSublayer(&ivars.marker);
        }
        ivars.disc.setFrame(this.bounds());
        ivars.marker.setCornerRadius(metrics::capsule(MARKER_DIAMETER));
        ivars.marker.setBorderWidth(MARKER_RING);
        ivars.marker.setBorderColor(Some(&NSColor::whiteColor().CGColor()));
        ivars.marker.setShadowColor(Some(&NSColor::blackColor().CGColor()));
        ivars.marker.setShadowOpacity(MARKER_SHADOW_OPACITY);
        ivars.marker.setShadowRadius(MARKER_SHADOW_RADIUS);
        ivars.marker.setShadowOffset(CGSize::new(0.0, MARKER_SHADOW_LIFT));
        CATransaction::commit();
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.widthAnchor().constraintEqualToConstant(WHEEL_DIAMETER).setActive(true);
        this.heightAnchor().constraintEqualToConstant(WHEEL_DIAMETER).setActive(true);
        this.setAccessibilityElement(true);
        this.setAccessibilityRole(Some(unsafe { NSAccessibilitySliderRole }));
        this.place_marker();
        return this;
    }

    pub fn hue(&self) -> f64 {
        return self.ivars().hue.get();
    }

    pub fn sat(&self) -> f64 {
        return self.ivars().sat.get();
    }

    pub fn on_move(&self, moved: Moved) {
        *self.ivars().moved.borrow_mut() = Some(moved);
    }

    pub fn set_wheel(&self, hue: f64, sat: f64, bright: f64) {
        let ivars = self.ivars();
        ivars.hue.set(hue.rem_euclid(360.0));
        ivars.sat.set(sat.clamp(0.0, 1.0));
        ivars.bright.set(bright.clamp(0.0, 1.0));
        self.place_marker();
    }

    fn place_marker(&self) {
        let ivars = self.ivars();
        let bounds = self.bounds();
        let reach = ivars.sat.get() * (bounds.size.width - MARKER_DIAMETER) / 2.0;
        let angle = ivars.hue.get().to_radians();
        let x = bounds.size.width / 2.0 + reach * angle.cos() - MARKER_DIAMETER / 2.0;
        let y = bounds.size.height / 2.0 + reach * angle.sin() - MARKER_DIAMETER / 2.0;
        let color = hsv_to_rgba(Hsv { h: ivars.hue.get(), s: ivars.sat.get(), v: ivars.bright.get() });
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        ivars.marker.setFrame(CGRect::new(CGPoint::new(x, y), CGSize::new(MARKER_DIAMETER, MARKER_DIAMETER)));
        ivars.marker.setBackgroundColor(Some(&theme::ns_color(color).CGColor()));
        CATransaction::commit();
        unsafe { self.setAccessibilityValue(Some(&ns(&color.to_hex()))) };
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn refresh_image(&self) {
        let scale = self.window().map_or(1.0, |window| window.backingScaleFactor());
        let diameter = self.bounds().size.width;
        let side = ((diameter * scale).round() as usize).max(1);
        if self.ivars().built.get() == (side, scale) {
            return;
        }
        let Some(image) = disc_image(side, diameter) else {
            return;
        };
        let ivars = self.ivars();
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        unsafe { ivars.disc.setContents(Some(&image)) };
        ivars.disc.setContentsScale(scale);
        CATransaction::commit();
        ivars.built.set((side, scale));
    }

    fn drag(&self, event: &NSEvent) {
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        let bounds = self.bounds();
        let (cx, cy) = (bounds.size.width / 2.0, bounds.size.height / 2.0);
        let (dx, dy) = ((point.x - cx) / cx, (point.y - cy) / cy);
        let reach = dx.hypot(dy);
        let (dx, dy) = if reach > 1.0 { (dx / reach, dy / reach) } else { (dx, dy) };
        self.set_wheel(dy.atan2(dx).to_degrees(), reach.min(1.0), self.ivars().bright.get());
        self.fire();
    }

    fn nudge(&self, dh: f64, ds: f64) {
        let ivars = self.ivars();
        let hue = (ivars.hue.get() + dh).rem_euclid(360.0);
        let sat = (ivars.sat.get() + ds).clamp(0.0, 1.0);
        self.set_wheel(hue, sat, ivars.bright.get());
        self.fire();
    }

    fn fire(&self) {
        let moved = self.ivars().moved.borrow().clone();
        if let Some(moved) = moved {
            moved(self);
        }
    }
}

type Handler = Rc<dyn Fn(Option<Pick>)>;

pub struct PanelIvars {
    view: Retained<NSView>,
    wheel: Retained<Wheel>,
    slider: Retained<NSSlider>,
    field: Retained<NSTextField>,
    reset: Retained<NSButton>,
    dots: Vec<(Tag, Retained<Dot>)>,
    text: Retained<NSColor>,
    error: Retained<NSColor>,
    current: Cell<Option<Pick>>,
    dark: Cell<bool>,
    syncing: Cell<bool>,
    shown: RefCell<String>,
    handler: RefCell<Handler>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = PanelIvars]
    pub struct Panel;

    unsafe impl NSObjectProtocol for Panel {}

    impl Panel {
        #[unsafe(method(resetColor:))]
        fn reset_color(&self, _sender: Option<&AnyObject>) {
            self.choose(None);
        }

        #[unsafe(method(brightnessChanged:))]
        fn brightness_changed(&self, sender: &NSSlider) {
            if self.ivars().syncing.get() {
                return;
            }
            let ivars = self.ivars();
            let color =
                hsv_to_rgba(Hsv { h: ivars.wheel.hue(), s: ivars.wheel.sat(), v: sender.doubleValue().clamp(0.0, 1.0) });
            self.choose(Some(Pick::Hex(color)));
        }
    }

    unsafe impl NSControlTextEditingDelegate for Panel {
        #[unsafe(method(controlTextDidChange:))]
        fn text_did_change(&self, _notification: &NSNotification) {
            self.validate();
        }

        #[unsafe(method(controlTextDidEndEditing:))]
        fn text_did_end(&self, _notification: &NSNotification) {
            self.commit();
        }
    }

    unsafe impl NSTextFieldDelegate for Panel {}
);

impl Panel {
    pub fn new(mtm: MainThreadMarker, theme: &Theme, tags: bool) -> Retained<Self> {
        let wheel = Wheel::new(mtm);
        let slider =
            unsafe { NSSlider::sliderWithValue_minValue_maxValue_target_action(1.0, 0.0, 1.0, None, None, mtm) };
        slider.setControlSize(NSControlSize::Small);
        slider.setContinuous(true);
        slider.setAccessibilityLabel(Some(&ns(&tr("Brightness"))));
        let field = widgets::input(mtm, "#RRGGBB", FIELD_WIDTH);
        field.setFont(Some(&metrics::digits(Text::Subheadline)));
        widgets::describe(&field, &tr("Color"), &tr("Color"));
        let reset = widgets::compact(mtm, "", Some(sel!(resetColor:)), None);
        reset.setHidden(true);
        let dots: Vec<(Tag, Retained<Dot>)> = if tags {
            Tag::ALL
                .iter()
                .map(|tag| {
                    let dot = Dot::new(mtm, TAG_DIAMETER, RING_PAD, true, None, theme);
                    dot.set_fill(Some(&theme::system(tag.system())));
                    widgets::describe(&dot, &tr(tag.title()), &tr(tag.title()));
                    dot.fit();
                    return (*tag, dot);
                })
                .collect()
        } else {
            Vec::new()
        };
        let slider_view: &NSView = &slider;
        let field_view: &NSView = &field;
        let reset_view: &NSView = &reset;
        let gap = widgets::spacer(mtm);
        let entry = widgets::stack(mtm, &[field_view, &gap, reset_view], false, metrics::SPACE_8);
        let entry_view: &NSView = &entry;
        let holder = NSView::new(mtm);
        let holder_view: &NSView = &holder;
        holder.addSubview(&wheel);
        let cells: Vec<&NSView> = dots.iter().map(|(_, dot)| &***dot).collect();
        let row = widgets::stack(mtm, &cells, false, metrics::SPACE_4);
        let row_view: &NSView = &row;
        let mut views: Vec<&NSView> = Vec::new();
        if !dots.is_empty() {
            views.push(row_view);
        }
        views.push(holder_view);
        views.push(slider_view);
        views.push(entry_view);
        let body = widgets::stack(mtm, &views, true, metrics::SPACE_8);
        body.setAlignment(NSLayoutAttribute::Leading);
        for pinned in [holder_view, slider_view, entry_view] {
            pinned.leadingAnchor().constraintEqualToAnchor(&body.leadingAnchor()).setActive(true);
            pinned.trailingAnchor().constraintEqualToAnchor(&body.trailingAnchor()).setActive(true);
        }
        wheel.centerXAnchor().constraintEqualToAnchor(&holder.centerXAnchor()).setActive(true);
        wheel.topAnchor().constraintEqualToAnchor(&holder.topAnchor()).setActive(true);
        holder.bottomAnchor().constraintEqualToAnchor(&wheel.bottomAnchor()).setActive(true);
        body.widthAnchor().constraintGreaterThanOrEqualToConstant(PANEL_MIN).setActive(true);
        let this = Self::alloc(mtm).set_ivars(PanelIvars {
            view: widgets::view(&body),
            wheel: wheel.clone(),
            slider: slider.clone(),
            field: field.clone(),
            reset: reset.clone(),
            dots,
            text: theme.color(Key::FieldText),
            error: theme.color(Key::Destructive),
            current: Cell::new(None),
            dark: Cell::new(false),
            syncing: Cell::new(false),
            shown: RefCell::new(String::new()),
            handler: RefCell::new(Rc::new(|_| {})),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        unsafe {
            field.setDelegate(Some(ProtocolObject::from_ref(&*this)));
            reset.setTarget(Some(&this));
            slider.setTarget(Some(&this));
            slider.setAction(Some(sel!(brightnessChanged:)));
        }
        widgets::describe(&wheel, &tr("Color"), &tr("Color"));
        let weak = Weak::new(&*this);
        wheel.on_move(Rc::new(move |wheel| {
            if let Some(panel) = weak.load() {
                panel.wheel_moved(wheel);
            }
        }));
        for (position, (_, dot)) in this.ivars().dots.iter().enumerate() {
            let panel = Weak::new(&*this);
            dot.on_press(move |_| {
                if let Some(panel) = panel.load() {
                    panel.choose_tag(position);
                }
            });
        }
        return this;
    }

    pub fn view(&self) -> Retained<NSView> {
        return self.ivars().view.clone();
    }

    pub fn set_handler(&self, handler: Handler) {
        *self.ivars().handler.borrow_mut() = handler;
    }

    pub fn configure(&self, current: Option<Pick>, dark: bool, reset: Option<&str>, handler: Handler) {
        let ivars = self.ivars();
        ivars.dark.set(dark);
        self.set_handler(handler);
        match reset {
            Some(title) => {
                ivars.reset.setTitle(&ns(title));
                ivars.reset.setHidden(false);
            }
            None => ivars.reset.setHidden(true),
        }
        self.select(current);
    }

    pub fn select(&self, pick: Option<Pick>) {
        self.sync(pick, true);
    }

    fn sync(&self, pick: Option<Pick>, field: bool) {
        let ivars = self.ivars();
        ivars.current.set(pick);
        let dark = ivars.dark.get();
        for (tag, dot) in &ivars.dots {
            let on = match pick {
                Some(Pick::Tag(other)) => other == *tag,
                Some(Pick::Hex(color)) => color == Pick::Tag(*tag).rgba(dark),
                None => false,
            };
            dot.set_selected(on);
        }
        let syncing = ivars.syncing.replace(true);
        if let Some(color) = pick.map(|pick| pick.rgba(dark)) {
            let Hsv { h, s, v } = rgba_to_hsv(color);
            ivars.wheel.set_wheel(h, s, v);
            ivars.slider.setDoubleValue(v);
        }
        self.paint_track();
        let text = pick.map(|pick| pick.rgba(dark).to_hex()).unwrap_or_default();
        if field || ivars.field.currentEditor().is_none() {
            ivars.field.setStringValue(&ns(&text));
            ivars.field.setTextColor(Some(&ivars.text));
            *ivars.shown.borrow_mut() = text;
        }
        ivars.reset.setEnabled(pick.is_some());
        ivars.syncing.set(syncing);
    }

    fn wheel_moved(&self, wheel: &Wheel) {
        let ivars = self.ivars();
        if ivars.syncing.get() {
            return;
        }
        let color = hsv_to_rgba(Hsv { h: wheel.hue(), s: wheel.sat(), v: ivars.slider.doubleValue() });
        self.choose(Some(Pick::Hex(color)));
    }

    fn paint_track(&self) {
        let ivars = self.ivars();
        let full = hsv_to_rgba(Hsv { h: ivars.wheel.hue(), s: ivars.wheel.sat(), v: 1.0 });
        ivars.slider.setTrackFillColor(Some(&theme::ns_color(full)));
    }

    fn choose_tag(&self, position: usize) {
        if let Some((tag, _)) = self.ivars().dots.get(position) {
            self.choose(Some(Pick::Tag(*tag)));
        }
    }

    fn choose(&self, pick: Option<Pick>) {
        self.apply(pick, true);
    }

    fn apply(&self, pick: Option<Pick>, field: bool) {
        self.sync(pick, field);
        let handler = self.ivars().handler.borrow().clone();
        handler(pick);
    }

    fn validate(&self) {
        let ivars = self.ivars();
        let text = ivars.field.stringValue().to_string();
        if text.trim().is_empty() {
            ivars.field.setTextColor(Some(&ivars.text));
            return;
        }
        if ivars.syncing.get() {
            return;
        }
        let Some(color) = parse_hex(&text) else {
            ivars.field.setTextColor(Some(&ivars.error));
            return;
        };
        ivars.field.setTextColor(Some(&ivars.text));
        self.apply(Some(Pick::Hex(color)), false);
    }

    fn commit(&self) {
        let ivars = self.ivars();
        if ivars.syncing.get() {
            return;
        }
        let text = ivars.field.stringValue().to_string();
        if text == *ivars.shown.borrow() {
            return;
        }
        match parse_hex(&text) {
            Some(color) => self.choose(Some(Pick::Hex(color))),
            None => self.select(ivars.current.get()),
        }
    }
}

thread_local! {
    static CHOOSER: RefCell<Option<(Retained<NSPopover>, Retained<Panel>)>> = const { RefCell::new(None) };
}

fn chooser(mtm: MainThreadMarker, theme: &Theme) -> (Retained<NSPopover>, Retained<Panel>) {
    if let Some(existing) = CHOOSER.with_borrow(Clone::clone) {
        return existing;
    }
    let panel = Panel::new(mtm, theme, false);
    let container = NSView::new(mtm);
    container.addSubview(&panel.view());
    widgets::pin(&panel.view(), &container, metrics::SPACE_12);
    let controller = NSViewController::new(mtm);
    controller.setView(&container);
    let popover = NSPopover::new(mtm);
    popover.setBehavior(NSPopoverBehavior::Transient);
    popover.setContentViewController(Some(&controller));
    popover.setContentSize(container.fittingSize());
    CHOOSER.with_borrow_mut(|slot| *slot = Some((popover.clone(), panel.clone())));
    return (popover, panel);
}

pub fn choose(
    mtm: MainThreadMarker,
    anchor: &NSView,
    theme: &Theme,
    current: Option<Pick>,
    dark: bool,
    reset: Option<&str>,
    handler: Handler,
) {
    let (popover, panel) = chooser(mtm, theme);
    if popover.isShown() {
        popover.close();
    }
    panel.configure(current, dark, reset, handler);
    popover.showRelativeToRect_ofView_preferredEdge(anchor.bounds(), anchor, NSRectEdge::MinY);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        return (a - b).abs() < 1e-6;
    }

    #[test]
    fn hsv_matches_reference_colors() {
        let red = rgba_to_hsv(Rgba::hex(0xFF_0000));
        assert!(close(red.h, 0.0) && close(red.s, 1.0) && close(red.v, 1.0));
        for (value, hue) in [(0x00_FF_00, 120.0), (0x00_00_FF, 240.0), (0xFF_FF_00, 60.0), (0x00_FF_FF, 180.0)] {
            assert!(close(rgba_to_hsv(Rgba::hex(value)).h, hue), "{value:06X}");
        }
        assert!(close(rgba_to_hsv(Rgba::hex(0xFF_00_FF)).h, 300.0));
        let white = rgba_to_hsv(Rgba::WHITE);
        assert!(close(white.s, 0.0) && close(white.v, 1.0));
        let black = rgba_to_hsv(Rgba::BLACK);
        assert!(close(black.s, 0.0) && close(black.v, 0.0));
        let gray = rgba_to_hsv(Rgba::hex(0x80_8080));
        assert!(close(gray.s, 0.0) && close(gray.v, 128.0 / 255.0));
    }

    #[test]
    fn hsv_round_trips_every_opaque_color() {
        for value in [
            0x00_0000, 0xFF_FFFF, 0xFF_0000, 0x00_FF00, 0x00_00FF, 0x00_7AFF, 0xFF_3B30, 0x34_C759, 0x80_8080,
            0x12_3456, 0xFE_DCBA, 0xCB_30E0, 0x01_0203,
        ] {
            let color = Rgba::hex(value);
            assert_eq!(hsv_to_rgba(rgba_to_hsv(color)), color, "{color}");
        }
    }

    #[test]
    fn hue_wraps_around_the_circle() {
        assert_eq!(hsv_to_rgba(Hsv { h: 360.0, s: 1.0, v: 1.0 }), hsv_to_rgba(Hsv { h: 0.0, s: 1.0, v: 1.0 }));
        assert_eq!(hsv_to_rgba(Hsv { h: -60.0, s: 1.0, v: 1.0 }), hsv_to_rgba(Hsv { h: 300.0, s: 1.0, v: 1.0 }));
        let hue = rgba_to_hsv(hsv_to_rgba(Hsv { h: 359.0, s: 1.0, v: 1.0 })).h;
        assert!((hue - 359.0).abs() < 1.0, "{hue}");
    }
}
