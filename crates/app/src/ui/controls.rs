use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityCheckBoxRole, NSApplication, NSBezierPath, NSColor, NSControlStateValue,
    NSControlStateValueOff, NSControlStateValueOn, NSEvent, NSTrackingArea, NSTrackingAreaOptions, NSView,
    NSViewLayerContentsRedrawPolicy,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_foundation::{NSNumber, NSPointInRect};
use objc2_quartz_core::{CAAutoresizingMask, CALayer, CATransaction};

use crate::style::Key;
use crate::ui::metrics;
use crate::ui::theme::Theme;
use crate::ui::widgets::{self, resolving};

const SPACE_KEY: u16 = 49;
const RETURN_KEY: u16 = 36;
const ENTER_KEY: u16 = 76;
const SWITCH_WIDTH: CGFloat = 26.0;
const SWITCH_HEIGHT: CGFloat = 15.0;
const KNOB_INSET: CGFloat = 2.0;
const KNOB_DIAMETER: CGFloat = SWITCH_HEIGHT - KNOB_INSET * 2.0;
const KNOB_SHADOW_OPACITY: f32 = 0.3;
const KNOB_SHADOW_RADIUS: CGFloat = 1.0;
const KNOB_SHADOW_LIFT: CGFloat = 1.0;

pub struct Control {
    hovered: Cell<bool>,
    pressed: Cell<bool>,
    enabled: Cell<bool>,
    selected: Cell<bool>,
}

impl Default for Control {
    fn default() -> Self {
        return Self {
            hovered: Cell::new(false),
            pressed: Cell::new(false),
            enabled: Cell::new(true),
            selected: Cell::new(false),
        };
    }
}

impl Control {
    pub fn new() -> Self {
        return Self::default();
    }

    pub fn hovered(&self) -> bool {
        return self.hovered.get();
    }

    pub fn pressed(&self) -> bool {
        return self.pressed.get();
    }

    pub fn enabled(&self) -> bool {
        return self.enabled.get();
    }

    pub fn selected(&self) -> bool {
        return self.selected.get();
    }
}

pub trait ControlView {
    fn view(&self) -> &NSView;
    fn control(&self) -> &Control;
    fn apply(&self, immediate: bool);
    fn activate(&self);
}

pub fn track(view: &NSView, owner: &AnyObject) {
    let options = NSTrackingAreaOptions::MouseEnteredAndExited
        | NSTrackingAreaOptions::ActiveInKeyWindow
        | NSTrackingAreaOptions::InVisibleRect;
    let area = unsafe {
        NSTrackingArea::initWithRect_options_owner_userInfo(
            NSTrackingArea::alloc(),
            CGRect::ZERO,
            options,
            Some(owner),
            None,
        )
    };
    view.addTrackingArea(&area);
}

pub fn set_selected<V: ControlView>(view: &V, selected: bool) {
    let control = view.control();
    if control.selected.replace(selected) == selected {
        return;
    }
    view.view().setAccessibilitySelected(selected);
    view.apply(false);
}

pub fn set_enabled<V: ControlView>(view: &V, enabled: bool) {
    let control = view.control();
    if control.enabled.replace(enabled) == enabled {
        return;
    }
    widgets::alpha(view.view(), if enabled { 1.0 } else { metrics::DIMMED });
    view.view().setAccessibilityEnabled(enabled);
    if !enabled {
        control.pressed.set(false);
    }
    view.apply(false);
}

pub fn entered<V: ControlView>(view: &V) {
    view.control().hovered.set(true);
    view.apply(false);
}

pub fn exited<V: ControlView>(view: &V) {
    view.control().hovered.set(false);
    view.apply(false);
}

pub fn mouse_down<V: ControlView>(view: &V) {
    let control = view.control();
    if !control.enabled.get() {
        return;
    }
    control.pressed.set(true);
    view.apply(true);
}

pub fn mouse_dragged<V: ControlView>(view: &V, event: &NSEvent) {
    let control = view.control();
    if !control.enabled.get() {
        return;
    }
    let inside = hit(view, event);
    if control.pressed.replace(inside) != inside {
        view.apply(inside);
    }
}

pub fn mouse_up<V: ControlView>(view: &V, event: &NSEvent) {
    let control = view.control();
    let was = control.pressed.replace(false);
    view.apply(false);
    if was && hit(view, event) && control.enabled.get() {
        view.activate();
    }
}

pub fn window_changed<V: ControlView>(view: &V) {
    let control = view.control();
    let stale = control.hovered.replace(false) | control.pressed.replace(false);
    if stale {
        view.apply(true);
    }
}

pub fn key_event(event: &NSEvent) -> bool {
    return matches!(event.keyCode(), SPACE_KEY | RETURN_KEY | ENTER_KEY);
}

fn hit<V: ControlView>(view: &V, event: &NSEvent) -> bool {
    let host = view.view();
    let point = host.convertPoint_fromView(event.locationInWindow(), None);
    return NSPointInRect(point, host.bounds());
}

pub struct Edge {
    layer: Retained<CALayer>,
    ink: Retained<NSColor>,
}

impl Edge {
    pub fn mount(host: &NSView, radius: CGFloat, ink: Retained<NSColor>) -> Option<Self> {
        let host_layer = host.layer()?;
        let layer = CALayer::new();
        layer.setFrame(host.bounds());
        layer.setAutoresizingMask(CAAutoresizingMask::LayerWidthSizable | CAAutoresizingMask::LayerHeightSizable);
        layer.setCornerRadius(radius);
        layer.setBorderWidth(metrics::BORDER_EMPHASIS);
        layer.setOpacity(0.0);
        host_layer.addSublayer(&layer);
        return Some(Self { layer, ink });
    }

    pub fn show(&self, host: &NSView, level: f32, animated: bool) {
        resolving(host, || self.layer.setBorderColor(Some(&self.ink.CGColor())));
        CATransaction::begin();
        if animated {
            CATransaction::setAnimationDuration(metrics::MOTION_QUICK);
            CATransaction::setAnimationTimingFunction(Some(&metrics::ease_out()));
        } else {
            CATransaction::setDisableActions(true);
        }
        self.layer.setOpacity(level);
        CATransaction::commit();
    }
}

pub struct SwitchIvars {
    control: Control,
    track_on: Retained<NSColor>,
    track_off: Retained<NSColor>,
    knob: Retained<CALayer>,
    on: Cell<bool>,
    tag: Cell<isize>,
    target: RefCell<Option<Retained<AnyObject>>>,
    action: Cell<Option<Sel>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = SwitchIvars]
    pub struct Switch;

    impl Switch {
        #[unsafe(method(wantsUpdateLayer))]
        fn wants_update_layer(&self) -> bool {
            true
        }

        #[unsafe(method(updateLayer))]
        fn update_layer(&self) {
            self.colorize();
        }

        #[unsafe(method(tag))]
        fn tag(&self) -> isize {
            self.ivars().tag.get()
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            self.ivars().control.enabled.get()
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            mouse_down(self);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            mouse_dragged(self, event);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            mouse_up(self, event);
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            entered(self);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            exited(self);
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn did_move_to_window(&self) {
            let () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            window_changed(self);
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if key_event(event) {
                self.activate();
                return;
            }
            let _: () = unsafe { msg_send![super(self), keyDown: event] };
        }

        #[unsafe(method(drawFocusRingMask))]
        fn draw_focus_ring_mask(&self) {
            NSBezierPath::bezierPathWithRect(self.bounds()).fill();
        }

        #[unsafe(method(focusRingMaskBounds))]
        fn focus_ring_mask_bounds(&self) -> CGRect {
            self.bounds()
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_perform_press(&self) -> bool {
            let enabled = self.ivars().control.enabled.get();
            if enabled {
                self.activate();
            }
            enabled
        }
    }
);

impl ControlView for Switch {
    fn view(&self) -> &NSView {
        self
    }

    fn control(&self) -> &Control {
        &self.ivars().control
    }

    fn apply(&self, _immediate: bool) {
        self.setNeedsDisplay(true);
    }

    fn activate(&self) {
        self.activate();
    }
}

impl Switch {
    pub fn new(mtm: MainThreadMarker, theme: &Theme) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SwitchIvars {
            control: Control::new(),
            track_on: theme.accent(),
            track_off: theme.color(Key::FieldFill),
            knob: CALayer::new(),
            on: Cell::new(false),
            tag: Cell::new(0),
            target: RefCell::new(None),
            action: Cell::new(None),
        });
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithFrame: CGRect::new(CGPoint::ZERO, CGSize::new(SWITCH_WIDTH, SWITCH_HEIGHT))]
        };
        this.setWantsLayer(true);
        this.setLayerContentsRedrawPolicy(NSViewLayerContentsRedrawPolicy::OnSetNeedsDisplay);
        let ivars = this.ivars();
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        if let Some(layer) = this.layer() {
            layer.setCornerRadius(metrics::capsule(SWITCH_HEIGHT));
            layer.addSublayer(&ivars.knob);
        }
        ivars
            .knob
            .setFrame(CGRect::new(CGPoint::new(KNOB_INSET, KNOB_INSET), CGSize::new(KNOB_DIAMETER, KNOB_DIAMETER)));
        ivars.knob.setCornerRadius(metrics::capsule(KNOB_DIAMETER));
        ivars.knob.setBackgroundColor(Some(&NSColor::whiteColor().CGColor()));
        ivars.knob.setShadowColor(Some(&NSColor::blackColor().CGColor()));
        ivars.knob.setShadowOpacity(KNOB_SHADOW_OPACITY);
        ivars.knob.setShadowRadius(KNOB_SHADOW_RADIUS);
        ivars.knob.setShadowOffset(CGSize::new(0.0, KNOB_SHADOW_LIFT));
        CATransaction::commit();
        track(&this, &this);
        this.setAccessibilityElement(true);
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityCheckBoxRole }));
        unsafe { this.setAccessibilityValue(Some(&NSNumber::numberWithBool(false))) };
        this.widthAnchor().constraintEqualToConstant(SWITCH_WIDTH).setActive(true);
        this.heightAnchor().constraintEqualToConstant(SWITCH_HEIGHT).setActive(true);
        this.setNeedsDisplay(true);
        return this;
    }

    pub fn state(&self) -> NSControlStateValue {
        return isize::from(self.ivars().on.get());
    }

    pub fn set_state(&self, state: NSControlStateValue) {
        let on = state == NSControlStateValueOn;
        let ivars = self.ivars();
        if ivars.on.replace(on) == on {
            return;
        }
        self.slide(false);
        unsafe { self.setAccessibilityValue(Some(&NSNumber::numberWithBool(on))) };
    }

    pub fn set_tag(&self, tag: isize) {
        self.ivars().tag.set(tag);
    }

    pub fn set_target(&self, target: Option<&AnyObject>) {
        *self.ivars().target.borrow_mut() = target.map(Message::retain);
    }

    pub fn set_action(&self, action: Option<Sel>) {
        self.ivars().action.set(action);
    }

    pub fn set_enabled(&self, enabled: bool) {
        set_enabled(self, enabled);
    }

    fn slide(&self, immediate: bool) {
        let ivars = self.ivars();
        let x = if ivars.on.get() { SWITCH_WIDTH - KNOB_INSET - KNOB_DIAMETER } else { KNOB_INSET };
        CATransaction::begin();
        if immediate || metrics::reduce_motion() {
            CATransaction::setDisableActions(true);
        } else {
            CATransaction::setAnimationDuration(metrics::MOTION_QUICK);
            CATransaction::setAnimationTimingFunction(Some(&metrics::ease_in_out()));
        }
        ivars.knob.setPosition(CGPoint::new(x + KNOB_DIAMETER / 2.0, KNOB_INSET + KNOB_DIAMETER / 2.0));
        CATransaction::commit();
    }

    fn activate(&self) {
        let ivars = self.ivars();
        if !ivars.control.enabled.get() {
            return;
        }
        self.set_state(if ivars.on.get() { NSControlStateValueOff } else { NSControlStateValueOn });
        self.send();
    }

    fn send(&self) {
        let ivars = self.ivars();
        let Some(action) = ivars.action.get() else {
            return;
        };
        let target = ivars.target.borrow().clone();
        unsafe {
            NSApplication::sharedApplication(self.mtm()).sendAction_to_from(action, target.as_deref(), Some(self));
        }
    }

    fn colorize(&self) {
        let Some(layer) = self.layer() else {
            return;
        };
        let ivars = self.ivars();
        let fill = if ivars.on.get() { &ivars.track_on } else { &ivars.track_off };
        resolving(self, || {
            layer.setBackgroundColor(Some(&fill.CGColor()));
        });
    }
}
