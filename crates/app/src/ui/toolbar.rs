use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSButton, NSColor, NSEvent, NSImage, NSImageView, NSLayoutConstraint,
    NSLineBreakMode, NSMenu, NSMenuItem, NSMenuToolbarItem, NSPopUpButton, NSSearchField, NSStackView, NSTextField,
    NSToolbar, NSToolbarDelegate, NSToolbarDisplayMode, NSToolbarFlexibleSpaceItemIdentifier, NSToolbarItem,
    NSToolbarItemGroup, NSToolbarItemGroupSelectionMode, NSToolbarItemIdentifier,
    NSToolbarSidebarTrackingSeparatorItemIdentifier, NSToolbarToggleInspectorItemIdentifier,
    NSToolbarToggleSidebarItemIdentifier, NSTrackingArea, NSTrackingAreaOptions, NSView,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect};
use objc2_foundation::{NSArray, NSCopying, NSObject, NSObjectProtocol, NSString};
use objc2_quartz_core::{CATransaction, kCAGravityResizeAspectFill};

use crate::app::{PlayState, Sound};
use crate::i18n::{Arg, key, tr, trf};
use crate::style::Key;
use crate::ui::metrics::{self, Text};
use crate::ui::state::{Filter, KINDS, Scope, Showing, Sort, TAG_ALL, desktop_name, lock_tip, showing};
use crate::ui::theme::{Look, Theme};
use crate::ui::view::{Library, Status};
use crate::ui::widgets::{self, Paint, Slot};

const PILL_WIDTH: CGFloat = 148.0;
const PILL_HEIGHT: CGFloat = metrics::CONTROL_HEIGHT;
const SCOPE_WIDTH: CGFloat = 128.0;
const SEARCH_WIDTH: CGFloat = 120.0;
const ADD_WIDTH: CGFloat = 76.0;
const STATUS_TEXT_WIDTH: CGFloat = 150.0;

pub struct PillIvars {
    index: Cell<Option<usize>>,
    menu: RefCell<Option<Retained<NSMenu>>>,
    hover: RefCell<Retained<NSColor>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = PillIvars]
    struct PillView;

    impl PillView {
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            let ivars = self.ivars();
            let menu = ivars.menu.borrow().clone();
            if let Some(menu) = menu {
                menu.popUpMenuPositioningItem_atLocation_inView(None, CGPoint::new(0.0, -metrics::SPACE_4), Some(self));
            } else if let Some(index) = ivars.index.get() {
                crate::ui::reveal(index);
            }
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.hover(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.hover(false);
        }
    }
);

impl PillView {
    fn hover(&self, on: bool) {
        let Some(layer) = self.layer() else {
            return;
        };
        let ivars = self.ivars();
        let active = ivars.index.get().is_some() || ivars.menu.borrow().is_some();
        let fill = if on && active { ivars.hover.borrow().clone() } else { NSColor::clearColor() };
        let fill = widgets::cg(self, &fill);
        CATransaction::begin();
        CATransaction::setAnimationDuration(metrics::MOTION_QUICK);
        layer.setBackgroundColor(Some(&fill));
        CATransaction::commit();
    }
}

pub struct DelegateIvars {
    items: Vec<(Retained<NSString>, Retained<NSToolbarItem>)>,
    order: Vec<Retained<NSString>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateIvars]
    pub struct ToolbarDelegate;

    unsafe impl NSObjectProtocol for ToolbarDelegate {}

    unsafe impl NSToolbarDelegate for ToolbarDelegate {
        #[unsafe(method_id(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
        fn item(
            &self,
            _toolbar: &NSToolbar,
            identifier: &NSToolbarItemIdentifier,
            _insert: bool,
        ) -> Option<Retained<NSToolbarItem>> {
            self.ivars().items.iter().find(|(id, _)| **id == *identifier).map(|(_, item)| item.clone())
        }

        #[unsafe(method_id(toolbarDefaultItemIdentifiers:))]
        fn defaults(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSToolbarItemIdentifier>> {
            NSArray::from_retained_slice(&self.ivars().order)
        }

        #[unsafe(method_id(toolbarAllowedItemIdentifiers:))]
        fn allowed(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSToolbarItemIdentifier>> {
            NSArray::from_retained_slice(&self.ivars().order)
        }
    }
);

pub struct Toolbar {
    pub bar: Retained<NSToolbar>,
    pub layout: Retained<NSToolbarItemGroup>,
    pub search: Retained<NSSearchField>,
    scope: Retained<NSPopUpButton>,
    sort: Retained<NSMenuToolbarItem>,
    add: Retained<NSButton>,
    add_menu: Retained<NSMenu>,
    desktop: Pill,
    screen: Pill,
    status: StatusBadge,
    scope_title: RefCell<String>,
    paint: Paint,
    _delegate: Retained<ToolbarDelegate>,
}

struct Pill {
    view: Retained<PillView>,
    thumb: Retained<NSView>,
    title: Retained<NSTextField>,
    badge: Retained<NSTextField>,
    chevron: Retained<NSView>,
    width: Retained<NSLayoutConstraint>,
    titles: RefCell<(Retained<NSColor>, Retained<NSColor>)>,
}

impl Toolbar {
    pub fn new(mtm: MainThreadMarker, target: &AnyObject, theme: &Theme) -> Self {
        let mut paint = Paint::new();
        let scope = popup(mtm, SCOPE_WIDTH);
        let sort = NSMenuToolbarItem::initWithItemIdentifier(NSMenuToolbarItem::alloc(mtm), &widgets::ns("sort"));
        sort.setShowsIndicator(false);
        if let Some(image) = widgets::symbol("arrow.up.arrow.down", metrics::ICON_REGULAR) {
            sort.setImage(Some(&image));
        }
        sort.setLabel(&widgets::ns(&tr("Sort & Filter")));
        sort.setToolTip(Some(&widgets::ns(&tr("Sort & Filter"))));
        let layout = layout_group(mtm, target);
        let (search, search_box) = search_field(mtm, target);
        let add = widgets::button(mtm, &tr("Add"), Some("plus"), sel!(showAdd:), target);
        add.setFont(Some(&metrics::font(Text::Headline)));
        add.setToolTip(Some(&widgets::ns(&tr("Add wallpapers from files, a folder or a video link"))));
        add.widthAnchor().constraintEqualToConstant(ADD_WIDTH).setActive(true);
        let settings = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), &widgets::ns("settings"));
        if let Some(image) = widgets::symbol("gearshape", metrics::ICON_REGULAR) {
            settings.setImage(Some(&image));
        }
        settings.setLabel(&widgets::ns(&tr("Settings")));
        settings.setToolTip(Some(&widgets::ns(&tr("Settings"))));
        unsafe {
            settings.setTarget(Some(target));
            settings.setAction(Some(sel!(showSettings:)));
        }
        let desktop = Pill::new(mtm, &tr("Desktop"), "display", theme, &mut paint);
        let screen = Pill::new(mtm, &tr("Lock Screen"), "lock.fill", theme, &mut paint);
        let line = paint.fill(mtm, Key::ToolbarSeparator, 0.0, theme);
        line.widthAnchor().constraintEqualToConstant(1.0).setActive(true);
        line.heightAnchor().constraintEqualToConstant(metrics::ICON_ROW).setActive(true);
        let row = widgets::stack(mtm, &[&desktop.view, &line, &screen.view], false, metrics::SPACE_4);
        row.setEdgeInsets(widgets::insets(0.0, metrics::SPACE_4, 0.0, metrics::SPACE_4));
        let showing = view_item(mtm, "showing", &tr("Now Showing"), &row);
        let status = StatusBadge::new(mtm, theme, &mut paint);

        let items: Vec<Retained<NSToolbarItem>> = vec![
            view_item(mtm, "scope", &tr("Desktop"), &scope),
            showing.clone(),
            status.toolbar_item(mtm),
            Retained::into_super(sort.clone()),
            Retained::into_super(layout.clone()),
            view_item(mtm, "search", &tr("Search"), &search_box),
            view_item(mtm, "add", &tr("Add"), &add),
            settings,
        ];
        let flexible = unsafe { NSToolbarFlexibleSpaceItemIdentifier };
        let toggle = unsafe { NSToolbarToggleInspectorItemIdentifier };
        let order: Vec<Retained<NSString>> = vec![
            unsafe { NSToolbarToggleSidebarItemIdentifier }.copy(),
            unsafe { NSToolbarSidebarTrackingSeparatorItemIdentifier }.copy(),
            widgets::ns("scope"),
            flexible.copy(),
            widgets::ns("showing"),
            widgets::ns("status"),
            flexible.copy(),
            widgets::ns("sort"),
            widgets::ns("layout"),
            widgets::ns("search"),
            widgets::ns("add"),
            widgets::ns("settings"),
            toggle.copy(),
        ];
        let items = items.into_iter().map(|item| (item.itemIdentifier(), item)).collect();
        let delegate = ToolbarDelegate::alloc(mtm).set_ivars(DelegateIvars { items, order });
        let delegate: Retained<ToolbarDelegate> = unsafe { msg_send![super(delegate), init] };
        let toolbar = NSToolbar::initWithIdentifier(NSToolbar::alloc(mtm), &widgets::ns("library"));
        toolbar.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        toolbar.setAllowsUserCustomization(false);
        toolbar.setAllowsDisplayModeCustomization(false);
        toolbar.setDisplayMode(NSToolbarDisplayMode::IconOnly);
        return Self {
            bar: toolbar,
            layout,
            search,
            scope,
            sort,
            add,
            add_menu: add_menu(mtm, target),
            desktop,
            screen,
            status,
            scope_title: RefCell::new(String::new()),
            paint,
            _delegate: delegate,
        };
    }

    pub fn restyle(&self, theme: &Theme) {
        self.paint.apply(theme);
        for pill in [&self.desktop, &self.screen] {
            pill.restyle(theme);
        }
    }

    pub fn sync(&self, mtm: MainThreadMarker, library: &Library, filter: &Filter, look: &Look, target: &AnyObject) {
        let title = filter.scope.title();
        if self.scope_title.borrow().as_str() != title {
            widgets::fade(&self.scope);
            self.scope_title.borrow_mut().clone_from(&title);
        }
        self.scope.setMenu(Some(&scope_menu(mtm, library, filter.scope, &title, sel!(scope:), target)));
        self.sort.setMenu(&sort_menu(mtm, filter, target));
        self.layout.setSelectedIndex(isize::from(filter.list));
        widgets::prominent(&self.add, &look.theme);
        let summary = showing(library, filter.scope);
        let menu = (summary.others > 0).then(|| desktops_menu(mtm, library, &summary, target));
        let item =
            summary.main.map(|index| &library.items[index]).map(|entry| (entry.title.clone(), entry.image.as_deref()));
        let tip = if summary.others > 0 {
            trf(
                "%@ · %ld different wallpapers",
                &[Arg::Text(filter.scope.title()), Arg::Int(i64::try_from(summary.others + 1).unwrap_or(0))],
            )
        } else {
            filter.scope.title()
        };
        self.desktop.update(summary.main, item, &tr("Desktop"), summary.others, menu);
        self.desktop.view.setToolTip(Some(&widgets::ns(&tip)));
        let lock_index = library.lock();
        let screen = lock_index.map(|index| &library.items[index]).map(|entry| {
            let image = library.lock.as_ref().and_then(|lock| lock.image.as_deref()).or(entry.image.as_deref());
            return (entry.title.clone(), image);
        });
        self.screen.update(lock_index, screen, &tr("Lock Screen"), 0, None);
        self.screen.view.setToolTip(Some(&widgets::ns(&lock_tip(library))));
        self.status.update(library.status.as_ref(), &look.theme);
    }
}

struct StatusBadge {
    view: Retained<NSStackView>,
    icon: Retained<NSImageView>,
    text: Retained<NSTextField>,
    sound: Retained<NSImageView>,
}

impl StatusBadge {
    fn new(mtm: MainThreadMarker, theme: &Theme, paint: &mut Paint) -> Self {
        let icon = NSImageView::new(mtm);
        icon.setImage(widgets::symbol("circle.fill", metrics::ICON_BADGE).as_deref());
        let text = paint.label(mtm, "", Text::SubheadlineMedium, Key::TextSecondary, theme);
        text.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        text.widthAnchor().constraintLessThanOrEqualToConstant(STATUS_TEXT_WIDTH).setActive(true);
        let sound = NSImageView::new(mtm);
        let view = widgets::stack(mtm, &[&icon, &text, &sound], false, metrics::SPACE_4);
        view.setCustomSpacing_afterView(metrics::SPACE_8, &text);
        view.setEdgeInsets(widgets::insets(0.0, metrics::SPACE_4, 0.0, metrics::SPACE_4));
        view.heightAnchor().constraintEqualToConstant(PILL_HEIGHT).setActive(true);
        return Self { view, icon, text, sound };
    }

    fn toolbar_item(&self, mtm: MainThreadMarker) -> Retained<NSToolbarItem> {
        let item = view_item(mtm, "status", &tr("Playback"), &self.view);
        item.setBordered(false);
        return item;
    }

    fn update(&self, status: Option<&Status>, theme: &Theme) {
        let (light, text, tip) = match status {
            None => (Light::Off, tr("Nothing playing"), tr("No wallpaper on this desktop")),
            Some(status) => describe(status),
        };
        self.icon.setContentTintColor(Some(&light.color(theme)));
        if self.text.stringValue().to_string() != text {
            widgets::fade(&self.text);
            self.text.setStringValue(&widgets::ns(&text));
        }
        self.view.setToolTip(Some(&widgets::ns(&tip)));
        let sound = status.and_then(|status| status.sound);
        self.sound.setHidden(sound.is_none());
        if let Some(sound) = sound {
            let (symbol, light, tip) = match sound {
                Sound::On => ("speaker.wave.2.fill", Light::Green, tr("Sound is on")),
                Sound::AllMuted => {
                    ("speaker.slash.fill", Light::Red, tr("All wallpapers are muted from the menu bar or hotkey"))
                }
                Sound::ItemMuted => {
                    ("speaker.slash.fill", Light::Yellow, tr("This wallpaper is muted in the inspector"))
                }
                Sound::Preview { here: true } => (
                    "headphones",
                    Light::Accent,
                    tr("Listening to the inspector preview; this desktop's copy is muted until you stop"),
                ),
                Sound::Preview { here: false } => {
                    ("headphones", Light::Accent, tr("Listening to a preview of another wallpaper in the inspector"))
                }
            };
            self.sound.setImage(widgets::symbol(symbol, metrics::ICON_SMALL).as_deref());
            self.sound.setContentTintColor(Some(&light.color(theme)));
            self.sound.setToolTip(Some(&widgets::ns(&tip)));
        }
    }
}

#[derive(Clone, Copy)]
enum Light {
    Off,
    Green,
    Yellow,
    Red,
    Accent,
}

impl Light {
    fn color(self, theme: &Theme) -> Retained<NSColor> {
        return theme.color(match self {
            Self::Off => Key::StatusOff,
            Self::Green => Key::StatusOk,
            Self::Yellow => Key::StatusWarn,
            Self::Red => Key::StatusError,
            Self::Accent => Key::Selection,
        });
    }
}

fn describe(status: &Status) -> (Light, String, String) {
    let reason = status.reason.as_ref();
    let brief = |word: &str| {
        let word = tr(word);
        return reason.map_or_else(|| word.clone(), |(brief, _)| format!("{word} · {brief}"));
    };
    let full = reason.map_or_else(String::new, |(_, full)| full.clone());
    return match status.state {
        PlayState::Still => (Light::Green, tr("Image"), tr("This desktop shows a still image")),
        PlayState::Playing => (Light::Green, tr("Playing"), tr("The wallpaper on this desktop is playing")),
        PlayState::Paused if full.is_empty() => (Light::Yellow, brief(key("Paused")), tr("Paused")),
        PlayState::Paused => (Light::Yellow, brief(key("Paused")), trf("Paused: %@", &[Arg::Text(full)])),
        PlayState::Unloaded => {
            let reason =
                if full.is_empty() { String::new() } else { format!("{}. ", trf("Paused: %@", &[Arg::Text(full)])) };
            (
                Light::Red,
                brief(key("Unloaded")),
                format!("{reason}{}", tr("Unloaded to free memory; the desktop shows the paused frame.")),
            )
        }
    };
}

impl Toolbar {
    pub fn add_parts(&self) -> (Retained<NSButton>, Retained<NSMenu>) {
        return (self.add.clone(), self.add_menu.clone());
    }
}

pub fn drop_down(button: &NSButton, menu: &NSMenu) {
    let height = button.bounds().size.height;
    menu.popUpMenuPositioningItem_atLocation_inView(
        None,
        objc2_core_foundation::CGPoint::new(0.0, height + metrics::SPACE_4),
        Some(button),
    );
}

fn popup(mtm: MainThreadMarker, width: CGFloat) -> Retained<NSPopUpButton> {
    let popup = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), CGRect::ZERO, true);
    popup.setBezelStyle(objc2_app_kit::NSBezelStyle::Glass);
    popup.setControlSize(objc2_app_kit::NSControlSize::Large);
    popup.setLineBreakMode(objc2_app_kit::NSLineBreakMode::ByTruncatingTail);
    popup.widthAnchor().constraintEqualToConstant(width).setActive(true);
    return popup;
}

fn search_field(mtm: MainThreadMarker, target: &AnyObject) -> (Retained<NSSearchField>, Retained<NSView>) {
    let search = NSSearchField::new(mtm);
    search.setControlSize(objc2_app_kit::NSControlSize::Large);
    search.setPlaceholderString(Some(&widgets::ns(&tr("Search"))));
    search.setSendsSearchStringImmediately(true);
    unsafe {
        search.setTarget(Some(target));
        search.setAction(Some(sel!(search:)));
    }
    let search_box = NSView::new(mtm);
    search_box.addSubview(&search);
    search.setTranslatesAutoresizingMaskIntoConstraints(false);
    search
        .leadingAnchor()
        .constraintEqualToAnchor_constant(&search_box.leadingAnchor(), metrics::SPACE_4)
        .setActive(true);
    search
        .trailingAnchor()
        .constraintEqualToAnchor_constant(&search_box.trailingAnchor(), -metrics::SPACE_4)
        .setActive(true);
    search.centerYAnchor().constraintEqualToAnchor(&search_box.centerYAnchor()).setActive(true);
    search_box.widthAnchor().constraintEqualToConstant(SEARCH_WIDTH).setActive(true);
    search_box.heightAnchor().constraintEqualToConstant(PILL_HEIGHT).setActive(true);
    return (search, search_box);
}

fn view_item(mtm: MainThreadMarker, id: &str, title: &str, view: &NSView) -> Retained<NSToolbarItem> {
    let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), &widgets::ns(id));
    item.setView(Some(view));
    item.setLabel(&widgets::ns(title));
    return item;
}

fn layout_group(mtm: MainThreadMarker, target: &AnyObject) -> Retained<NSToolbarItemGroup> {
    let images: Vec<Retained<NSImage>> = ["square.grid.2x2", "list.bullet"]
        .iter()
        .filter_map(|name| widgets::symbol(name, metrics::ICON_REGULAR))
        .collect();
    let labels = [widgets::ns(&tr("Grid")), widgets::ns(&tr("List"))];
    let group = unsafe {
        NSToolbarItemGroup::groupWithItemIdentifier_images_selectionMode_labels_target_action(
            &widgets::ns("layout"),
            &NSArray::from_retained_slice(&images),
            NSToolbarItemGroupSelectionMode::SelectOne,
            Some(&NSArray::from_retained_slice(&labels)),
            Some(target),
            Some(sel!(layout:)),
            mtm,
        )
    };
    group.setSelectedIndex(0);
    return group;
}

fn desktops_menu(mtm: MainThreadMarker, library: &Library, summary: &Showing, target: &AnyObject) -> Retained<NSMenu> {
    let mut items = Vec::new();
    for &(space, item) in &summary.desktops {
        let wallpaper = item.map(|index| &library.items[index]);
        let title = format!("{} — {}", desktop_name(space), wallpaper.map_or(tr("None"), |entry| entry.title.clone()));
        let tag = item.and_then(|index| isize::try_from(index).ok()).unwrap_or(TAG_ALL);
        let entry = entry(mtm, &title, sel!(reveal:), tag, false, target);
        entry.setEnabled(item.is_some());
        if let Some(image) = wallpaper.and_then(|entry| entry.image.as_ref()) {
            let thumb: Retained<NSImage> = image.copy();
            thumb.setSize(objc2_foundation::NSSize::new(metrics::ICON_HERO, metrics::ICON_LARGE));
            entry.setImage(Some(&thumb));
        }
        items.push(entry);
    }
    return menu(mtm, &items);
}

impl Pill {
    fn new(mtm: MainThreadMarker, caption: &str, icon: &str, theme: &Theme, paint: &mut Paint) -> Self {
        let glyph = widgets::symbol_view(mtm, icon, metrics::ICON_SMALL, &theme.color(Key::TextSecondary));
        paint.add(&glyph, Key::TextSecondary, Slot::Tint);
        let thumb = NSView::new(mtm);
        thumb.setWantsLayer(true);
        if let Some(layer) = thumb.layer() {
            layer.setCornerRadius(metrics::RADIUS_CONTROL);
            layer.setMasksToBounds(true);
            layer.setBorderWidth(metrics::BORDER_HAIRLINE);
            layer.setContentsGravity(unsafe { kCAGravityResizeAspectFill });
        }
        thumb.widthAnchor().constraintEqualToConstant(29.0).setActive(true);
        thumb.heightAnchor().constraintEqualToConstant(18.0).setActive(true);
        let title = widgets::label(mtm, caption, Text::CalloutMedium, &theme.color(Key::TextTertiary));
        title.setWantsLayer(true);
        let badge = paint.label(mtm, "", Text::CaptionBold, Key::TextSecondary, theme);
        badge.setContentCompressionResistancePriority_forOrientation(
            1000.0,
            objc2_app_kit::NSLayoutConstraintOrientation::Horizontal,
        );
        badge.setHidden(true);
        let chevron = widgets::symbol_view(mtm, "chevron.down", metrics::ICON_BADGE, &theme.color(Key::TextTertiary));
        paint.add(&chevron, Key::TextTertiary, Slot::Tint);
        chevron.setHidden(true);
        let row = widgets::stack(mtm, &[&glyph, &thumb, &title, &badge, &chevron], false, metrics::SPACE_4);
        row.setEdgeInsets(widgets::insets(0.0, metrics::SPACE_8, 0.0, metrics::SPACE_8));
        row.setCustomSpacing_afterView(metrics::SPACE_8, &thumb);
        row.setCustomSpacing_afterView(metrics::SPACE_2, &badge);
        let view = PillView::alloc(mtm).set_ivars(PillIvars {
            index: Cell::new(None),
            menu: RefCell::new(None),
            hover: RefCell::new(theme.color(Key::ToolbarPillHover)),
        });
        let view: Retained<PillView> = unsafe { msg_send![super(view), init] };
        view.setWantsLayer(true);
        if let Some(layer) = view.layer() {
            layer.setCornerRadius(metrics::capsule(PILL_HEIGHT));
        }
        view.addSubview(&row);
        widgets::pin(&row, &view, 0.0);
        let width = view.widthAnchor().constraintEqualToConstant(PILL_WIDTH);
        width.setActive(true);
        view.heightAnchor().constraintEqualToConstant(PILL_HEIGHT).setActive(true);
        let options = NSTrackingAreaOptions::MouseEnteredAndExited
            | NSTrackingAreaOptions::ActiveInKeyWindow
            | NSTrackingAreaOptions::InVisibleRect;
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                CGRect::ZERO,
                options,
                Some(&view),
                None,
            )
        };
        view.addTrackingArea(&area);
        let titles = RefCell::new((theme.color(Key::Text), theme.color(Key::TextTertiary)));
        let pill = Self { view, thumb, title, badge, chevron, width, titles };
        pill.restyle(theme);
        return pill;
    }

    fn restyle(&self, theme: &Theme) {
        *self.view.ivars().hover.borrow_mut() = theme.color(Key::ToolbarPillHover);
        *self.titles.borrow_mut() = (theme.color(Key::Text), theme.color(Key::TextTertiary));
        if let Some(layer) = self.thumb.layer() {
            layer.setBorderColor(Some(&widgets::cg(&self.thumb, &theme.color(Key::ToolbarSeparator))));
            layer.setBackgroundColor(Some(&widgets::cg(&self.thumb, &theme.color(Key::ToolbarPill))));
        }
    }

    fn update(
        &self,
        index: Option<usize>,
        shown: Option<(String, Option<&NSImage>)>,
        caption: &str,
        others: usize,
        menu: Option<Retained<NSMenu>>,
    ) {
        let (name, image) = shown.unzip();
        widgets::fade(&self.thumb);
        if let Some(layer) = self.thumb.layer() {
            unsafe {
                match image.flatten() {
                    Some(image) => layer.setContents(Some(image)),
                    None => layer.setContents(None),
                }
            }
        }
        widgets::fade(&self.title);
        let titles = self.titles.borrow();
        self.title.setStringValue(&widgets::ns(name.as_deref().unwrap_or(caption)));
        self.title.setTextColor(Some(if name.is_some() { &titles.0 } else { &titles.1 }));
        let expanded = others > 0;
        self.badge.setStringValue(&widgets::ns(&format!("+{others}")));
        if self.badge.isHidden() == expanded {
            widgets::animate(metrics::MOTION_STANDARD, || {
                self.badge.animator().setHidden(!expanded);
                self.chevron.animator().setHidden(!expanded);
                self.width.setConstant(PILL_WIDTH + if expanded { metrics::SPACE_24 } else { 0.0 });
                if let Some(container) = unsafe { self.view.superview() } {
                    container.layoutSubtreeIfNeeded();
                }
            });
        }
        let ivars = self.view.ivars();
        ivars.index.set(index);
        *ivars.menu.borrow_mut() = menu;
    }
}

fn entry(
    mtm: MainThreadMarker,
    title: &str,
    action: Sel,
    tag: isize,
    checked: bool,
    target: &AnyObject,
) -> Retained<NSMenuItem> {
    let item = widgets::menu_item(mtm, title, action, Some(target), tag, None);
    item.setState(isize::from(checked));
    return item;
}

fn title_entry(mtm: MainThreadMarker, title: &str, icon: &str) -> Retained<NSMenuItem> {
    let item = NSMenuItem::new(mtm);
    item.setTitle(&widgets::ns(title));
    if let Some(image) = widgets::symbol(icon, metrics::ICON_REGULAR) {
        item.setImage(Some(&image));
    }
    return item;
}

fn header(mtm: MainThreadMarker, title: &str) -> Retained<NSMenuItem> {
    return NSMenuItem::sectionHeaderWithTitle(&widgets::ns(title), mtm);
}

fn separator(mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    return NSMenuItem::separatorItem(mtm);
}

fn menu(mtm: MainThreadMarker, items: &[Retained<NSMenuItem>]) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    for item in items {
        menu.addItem(item);
    }
    return menu;
}

pub fn scope_menu(
    mtm: MainThreadMarker,
    library: &Library,
    current: Scope,
    title: &str,
    action: Sel,
    target: &AnyObject,
) -> Retained<NSMenu> {
    let mut items = vec![
        title_entry(mtm, title, "display"),
        entry(mtm, &tr("All Desktops"), action, Scope::All.tag(), current == Scope::All, target),
        separator(mtm),
    ];
    for space in 0..library.desktops {
        let scope = Scope::Space(space);
        items.push(entry(mtm, &desktop_name(space), action, scope.tag(), current == scope, target));
    }
    return menu(mtm, &items);
}

fn sort_menu(mtm: MainThreadMarker, filter: &Filter, target: &AnyObject) -> Retained<NSMenu> {
    let mut items = vec![header(mtm, &tr("Sort By"))];
    for (position, (sort, title)) in Sort::ALL.iter().enumerate() {
        let tag = isize::try_from(position).unwrap_or(0);
        items.push(entry(mtm, &tr(title), sel!(sort:), tag, filter.sort == *sort, target));
    }
    items.push(separator(mtm));
    items.push(header(mtm, &tr("Show")));
    for (position, (kind, title)) in KINDS.iter().enumerate() {
        let tag = isize::try_from(position).unwrap_or(0);
        items.push(entry(mtm, &tr(title), sel!(kind:), tag, filter.kind == *kind, target));
    }
    return menu(mtm, &items);
}

fn add_menu(mtm: MainThreadMarker, target: &AnyObject) -> Retained<NSMenu> {
    let items = [
        entry(mtm, &tr("From Files…"), sel!(addFiles:), 0, false, target),
        entry(mtm, &tr("From Folder…"), sel!(addFiles:), 1, false, target),
        entry(mtm, &tr("From Video Link…"), sel!(download:), 0, false, target),
    ];
    for (item, icon) in items.iter().zip(["doc.badge.plus", "folder.badge.plus", "link.badge.plus"]) {
        if let Some(image) = widgets::symbol(icon, metrics::ICON_REGULAR) {
            item.setImage(Some(&image));
        }
    }
    return menu(
        mtm,
        &[header(mtm, &tr("Add Wallpapers")), items[0].clone(), items[1].clone(), separator(mtm), items[2].clone()],
    );
}
