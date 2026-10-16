use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSButton, NSColor, NSEvent, NSImage, NSLayoutConstraint, NSMenu, NSMenuItem,
    NSMenuToolbarItem, NSPopUpButton, NSSearchField, NSTextField, NSToolbar, NSToolbarDelegate, NSToolbarDisplayMode,
    NSToolbarFlexibleSpaceItemIdentifier, NSToolbarItem, NSToolbarItemGroup, NSToolbarItemGroupSelectionMode,
    NSToolbarItemIdentifier, NSToolbarToggleInspectorItemIdentifier, NSTrackingArea, NSTrackingAreaOptions, NSView,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect};
use objc2_foundation::{NSArray, NSCopying, NSObject, NSObjectProtocol, NSString};
use objc2_quartz_core::{CATransaction, kCAGravityResizeAspectFill};

use crate::ui::palette::Look;
use crate::ui::state::{Filter, KINDS, Scope, Showing, Sort, TAG_ALL, TAG_NEW_FOLDER, lock_tip, showing};
use crate::ui::view::Library;
use crate::ui::widgets::{self, Weight};

const PILL_WIDTH: CGFloat = 124.0;
const PILL_HEIGHT: CGFloat = 26.0;
const SCOPE_WIDTH: CGFloat = 128.0;
const FOLDER_WIDTH: CGFloat = 136.0;
const SEARCH_WIDTH: CGFloat = 120.0;
const ADD_WIDTH: CGFloat = 76.0;

pub struct PillIvars {
    index: Cell<Option<usize>>,
    menu: RefCell<Option<Retained<NSMenu>>>,
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
                menu.popUpMenuPositioningItem_atLocation_inView(None, CGPoint::new(0.0, -6.0), Some(self));
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
        let fill = if on && active {
            widgets::cg(self.mtm(), &NSColor::quaternaryLabelColor())
        } else {
            widgets::cg(self.mtm(), &NSColor::clearColor())
        };
        CATransaction::begin();
        CATransaction::setAnimationDuration(0.15);
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
    folder: Retained<NSPopUpButton>,
    sort: Retained<NSMenuToolbarItem>,
    add: Retained<NSButton>,
    add_menu: Retained<NSMenu>,
    desktop: Pill,
    screen: Pill,
    scope_title: RefCell<String>,
    folder_title: RefCell<String>,
    _delegate: Retained<ToolbarDelegate>,
}

struct Pill {
    view: Retained<PillView>,
    thumb: Retained<NSView>,
    title: Retained<NSTextField>,
    badge: Retained<NSTextField>,
    chevron: Retained<NSView>,
    width: Retained<NSLayoutConstraint>,
}

impl Toolbar {
    pub fn new(mtm: MainThreadMarker, target: &AnyObject) -> Self {
        let scope = popup(mtm, SCOPE_WIDTH);
        let folder = popup(mtm, FOLDER_WIDTH);
        let sort = NSMenuToolbarItem::initWithItemIdentifier(NSMenuToolbarItem::alloc(mtm), &widgets::ns("sort"));
        sort.setShowsIndicator(false);
        if let Some(image) = widgets::symbol("arrow.up.arrow.down", 13.0) {
            sort.setImage(Some(&image));
        }
        sort.setLabel(&widgets::ns("Sort & Filter"));
        sort.setToolTip(Some(&widgets::ns("Sort & Filter")));
        let layout = layout_group(mtm, target);
        let search = NSSearchField::new(mtm);
        search.setControlSize(objc2_app_kit::NSControlSize::Large);
        search.setPlaceholderString(Some(&widgets::ns("Search")));
        search.setSendsSearchStringImmediately(true);
        unsafe {
            search.setTarget(Some(target));
            search.setAction(Some(sel!(search:)));
        }
        let search_box = NSView::new(mtm);
        search_box.addSubview(&search);
        search.setTranslatesAutoresizingMaskIntoConstraints(false);
        search.leadingAnchor().constraintEqualToAnchor_constant(&search_box.leadingAnchor(), 4.0).setActive(true);
        search.trailingAnchor().constraintEqualToAnchor_constant(&search_box.trailingAnchor(), -4.0).setActive(true);
        search.centerYAnchor().constraintEqualToAnchor(&search_box.centerYAnchor()).setActive(true);
        search_box.widthAnchor().constraintEqualToConstant(SEARCH_WIDTH).setActive(true);
        search_box.heightAnchor().constraintEqualToConstant(PILL_HEIGHT).setActive(true);
        let add = widgets::button(mtm, "Add", Some("plus"), sel!(showAdd:), target);
        add.setFont(Some(&widgets::font(13.0, &Weight::Semibold)));
        add.setToolTip(Some(&widgets::ns("Add wallpapers from files, a folder or a video link")));
        add.widthAnchor().constraintEqualToConstant(ADD_WIDTH).setActive(true);
        let settings = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), &widgets::ns("settings"));
        if let Some(image) = widgets::symbol("gearshape", 13.0) {
            settings.setImage(Some(&image));
        }
        settings.setLabel(&widgets::ns("Settings"));
        settings.setToolTip(Some(&widgets::ns("Settings")));
        unsafe {
            settings.setTarget(Some(target));
            settings.setAction(Some(sel!(showSettings:)));
        }
        let desktop = Pill::new(mtm, "Desktop", "display");
        let screen = Pill::new(mtm, "Lock Screen", "lock.fill");
        let line = widgets::card(mtm, &NSColor::separatorColor(), 0.0);
        line.widthAnchor().constraintEqualToConstant(1.0).setActive(true);
        line.heightAnchor().constraintEqualToConstant(16.0).setActive(true);
        let row = widgets::stack(mtm, &[&desktop.view, &line, &screen.view], false, 3.0);
        row.setEdgeInsets(widgets::insets(0.0, 5.0, 0.0, 5.0));
        let showing = view_item(mtm, "showing", "Now Showing", &row);

        let items: Vec<Retained<NSToolbarItem>> = vec![
            view_item(mtm, "scope", "Desktop", &scope),
            view_item(mtm, "folder", "Folder", &folder),
            showing.clone(),
            Retained::into_super(sort.clone()),
            Retained::into_super(layout.clone()),
            view_item(mtm, "search", "Search", &search_box),
            view_item(mtm, "add", "Add", &add),
            settings,
        ];
        let flexible = unsafe { NSToolbarFlexibleSpaceItemIdentifier };
        let toggle = unsafe { NSToolbarToggleInspectorItemIdentifier };
        let order: Vec<Retained<NSString>> = vec![
            widgets::ns("scope"),
            widgets::ns("folder"),
            flexible.copy(),
            widgets::ns("showing"),
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
            folder,
            sort,
            add,
            add_menu: add_menu(mtm, target),
            desktop,
            screen,
            scope_title: RefCell::new(String::new()),
            folder_title: RefCell::new(String::new()),
            _delegate: delegate,
        };
    }

    pub fn sync(&self, mtm: MainThreadMarker, library: &Library, filter: &Filter, look: &Look, target: &AnyObject) {
        let title = filter.scope.title();
        if self.scope_title.borrow().as_str() != title {
            widgets::fade(&self.scope);
            self.scope_title.borrow_mut().clone_from(&title);
        }
        self.scope.setMenu(Some(&scope_menu(mtm, library, filter.scope, &title, sel!(scope:), target)));
        let folder_title =
            filter.folder.and_then(|index| library.folders.get(index)).map_or("All Wallpapers", |name| name);
        if self.folder_title.borrow().as_str() != folder_title {
            widgets::fade(&self.folder);
            *self.folder_title.borrow_mut() = folder_title.to_string();
        }
        self.folder.setMenu(Some(&folder_menu(mtm, library, filter.folder, folder_title, target)));
        self.sort.setMenu(&sort_menu(mtm, filter, target));
        self.layout.setSelectedIndex(isize::from(filter.list));
        widgets::prominent(&self.add, &look.colors);
        let summary = showing(library, filter.scope);
        let menu = (summary.others > 0).then(|| desktops_menu(mtm, library, &summary, target));
        let item =
            summary.main.map(|index| &library.items[index]).map(|entry| (entry.title.clone(), entry.image.as_deref()));
        let tip = if summary.others > 0 {
            format!("{} · {} different wallpapers", filter.scope.title(), summary.others + 1)
        } else {
            filter.scope.title()
        };
        self.desktop.update(summary.main, item, "Desktop", summary.others, menu);
        self.desktop.view.setToolTip(Some(&widgets::ns(&tip)));
        let lock_index = library.lock();
        let screen = lock_index.map(|index| &library.items[index]).map(|entry| {
            let image = library.lock.as_ref().and_then(|lock| lock.image.as_deref()).or(entry.image.as_deref());
            return (entry.title.clone(), image);
        });
        self.screen.update(lock_index, screen, "Lock Screen", 0, None);
        self.screen.view.setToolTip(Some(&widgets::ns(&lock_tip(library))));
    }
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
        objc2_core_foundation::CGPoint::new(0.0, height + 6.0),
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

fn view_item(mtm: MainThreadMarker, id: &str, title: &str, view: &NSView) -> Retained<NSToolbarItem> {
    let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), &widgets::ns(id));
    item.setView(Some(view));
    item.setLabel(&widgets::ns(title));
    return item;
}

fn layout_group(mtm: MainThreadMarker, target: &AnyObject) -> Retained<NSToolbarItemGroup> {
    let images: Vec<Retained<NSImage>> =
        ["square.grid.2x2", "list.bullet"].iter().filter_map(|name| widgets::symbol(name, 13.0)).collect();
    let labels = [widgets::ns("Grid"), widgets::ns("List")];
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
        let title = format!("Desktop {} — {}", space + 1, wallpaper.map_or("None", |entry| entry.title.as_str()));
        let tag = item.and_then(|index| isize::try_from(index).ok()).unwrap_or(TAG_ALL);
        let entry = entry(mtm, &title, sel!(reveal:), tag, false, target);
        entry.setEnabled(item.is_some());
        if let Some(image) = wallpaper.and_then(|entry| entry.image.as_ref()) {
            let thumb: Retained<NSImage> = image.copy();
            thumb.setSize(objc2_foundation::NSSize::new(32.0, 20.0));
            entry.setImage(Some(&thumb));
        }
        items.push(entry);
    }
    return menu(mtm, &items);
}

impl Pill {
    fn new(mtm: MainThreadMarker, caption: &str, icon: &str) -> Self {
        let glyph = widgets::symbol_view(mtm, icon, 11.0, &NSColor::secondaryLabelColor());
        let thumb = NSView::new(mtm);
        thumb.setWantsLayer(true);
        if let Some(layer) = thumb.layer() {
            layer.setCornerRadius(4.0);
            layer.setMasksToBounds(true);
            layer.setBorderWidth(0.5);
            layer.setBorderColor(Some(&widgets::cg(mtm, &NSColor::separatorColor())));
            layer.setContentsGravity(unsafe { kCAGravityResizeAspectFill });
            layer.setBackgroundColor(Some(&widgets::cg(mtm, &NSColor::quaternaryLabelColor())));
        }
        thumb.widthAnchor().constraintEqualToConstant(29.0).setActive(true);
        thumb.heightAnchor().constraintEqualToConstant(18.0).setActive(true);
        let title = widgets::label(mtm, caption, 12.0, &Weight::Medium, &NSColor::tertiaryLabelColor());
        title.setWantsLayer(true);
        let badge = widgets::label(mtm, "", 10.0, &Weight::Semibold, &NSColor::secondaryLabelColor());
        badge.setContentCompressionResistancePriority_forOrientation(
            1000.0,
            objc2_app_kit::NSLayoutConstraintOrientation::Horizontal,
        );
        badge.setHidden(true);
        let chevron = widgets::symbol_view(mtm, "chevron.down", 8.0, &NSColor::tertiaryLabelColor());
        chevron.setHidden(true);
        let row = widgets::stack(mtm, &[&glyph, &thumb, &title, &badge, &chevron], false, 6.0);
        row.setEdgeInsets(widgets::insets(0.0, 8.0, 0.0, 8.0));
        row.setCustomSpacing_afterView(3.0, &badge);
        let view = PillView::alloc(mtm).set_ivars(PillIvars { index: Cell::new(None), menu: RefCell::new(None) });
        let view: Retained<PillView> = unsafe { msg_send![super(view), init] };
        view.setWantsLayer(true);
        if let Some(layer) = view.layer() {
            layer.setCornerRadius(PILL_HEIGHT / 2.0);
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
        return Self { view, thumb, title, badge, chevron, width };
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
        let color = if name.is_some() { NSColor::labelColor() } else { NSColor::tertiaryLabelColor() };
        self.title.setStringValue(&widgets::ns(name.as_deref().unwrap_or(caption)));
        self.title.setTextColor(Some(&color));
        let expanded = others > 0;
        self.badge.setStringValue(&widgets::ns(&format!("+{others}")));
        if self.badge.isHidden() == expanded {
            widgets::animate(0.2, || {
                self.badge.animator().setHidden(!expanded);
                self.chevron.animator().setHidden(!expanded);
                self.width.setConstant(PILL_WIDTH + if expanded { 26.0 } else { 0.0 });
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
    if let Some(image) = widgets::symbol(icon, 12.0) {
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
        entry(mtm, "All Desktops", action, Scope::All.tag(), current == Scope::All, target),
        separator(mtm),
    ];
    for space in 0..library.desktops {
        let scope = Scope::Space(space);
        items.push(entry(mtm, &format!("Desktop {}", space + 1), action, scope.tag(), current == scope, target));
    }
    return menu(mtm, &items);
}

fn folder_menu(
    mtm: MainThreadMarker,
    library: &Library,
    current: Option<usize>,
    title: &str,
    target: &AnyObject,
) -> Retained<NSMenu> {
    let action = sel!(folder:);
    let mut items = vec![
        title_entry(mtm, title, "folder"),
        entry(mtm, "All Wallpapers", action, TAG_ALL, current.is_none(), target),
        separator(mtm),
    ];
    for (index, name) in library.folders.iter().enumerate() {
        let tag = isize::try_from(index).unwrap_or(TAG_ALL);
        items.push(entry(mtm, name, action, tag, current == Some(index), target));
    }
    items.push(separator(mtm));
    items.push(entry(mtm, "New Folder", action, TAG_NEW_FOLDER, false, target));
    return menu(mtm, &items);
}

fn sort_menu(mtm: MainThreadMarker, filter: &Filter, target: &AnyObject) -> Retained<NSMenu> {
    let mut items = vec![header(mtm, "Sort By")];
    for (position, (sort, title)) in Sort::ALL.iter().enumerate() {
        let tag = isize::try_from(position).unwrap_or(0);
        items.push(entry(mtm, title, sel!(sort:), tag, filter.sort == *sort, target));
    }
    items.push(separator(mtm));
    items.push(header(mtm, "Show"));
    for (position, (kind, title)) in KINDS.iter().enumerate() {
        let tag = isize::try_from(position).unwrap_or(0);
        items.push(entry(mtm, title, sel!(kind:), tag, filter.kind == *kind, target));
    }
    return menu(mtm, &items);
}

fn add_menu(mtm: MainThreadMarker, target: &AnyObject) -> Retained<NSMenu> {
    let items = [
        entry(mtm, "From Files…", sel!(addFiles:), 0, false, target),
        entry(mtm, "From Folder…", sel!(addFiles:), 1, false, target),
        entry(mtm, "From Video Link…", sel!(download:), 0, false, target),
    ];
    for (item, icon) in items.iter().zip(["doc.badge.plus", "folder.badge.plus", "link.badge.plus"]) {
        if let Some(image) = widgets::symbol(icon, 13.0) {
            item.setImage(Some(&image));
        }
    }
    return menu(
        mtm,
        &[header(mtm, "Add Wallpapers"), items[0].clone(), items[1].clone(), separator(mtm), items[2].clone()],
    );
}
