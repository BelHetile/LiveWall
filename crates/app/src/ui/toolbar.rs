use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSColor, NSEvent, NSImage, NSMenu, NSMenuItem, NSMenuToolbarItem, NSSearchToolbarItem, NSToolbar,
    NSToolbarDelegate, NSToolbarFlexibleSpaceItemIdentifier, NSToolbarInspectorTrackingSeparatorItemIdentifier,
    NSToolbarItem, NSToolbarItemGroup, NSToolbarItemGroupSelectionMode, NSToolbarItemIdentifier, NSToolbarItemStyle,
    NSToolbarToggleInspectorItemIdentifier, NSView,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSArray, NSCopying, NSObject, NSObjectProtocol, NSString};
use objc2_quartz_core::{CALayer, kCAGravityResizeAspectFill};

use crate::ui::fixture::{Library, Target};
use crate::ui::palette::Theme;
use crate::ui::widgets::{self, Weight};

pub struct PillIvars {
    index: usize,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = PillIvars]
    struct PillView;

    impl PillView {
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            crate::ui::select(Some(self.ivars().index));
        }
    }
);

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

impl ToolbarDelegate {
    pub fn toolbar(&self, mtm: MainThreadMarker) -> Retained<NSToolbar> {
        let toolbar = NSToolbar::initWithIdentifier(NSToolbar::alloc(mtm), &NSString::from_str("library"));
        toolbar.setDelegate(Some(ProtocolObject::from_ref(self)));
        toolbar.setAllowsUserCustomization(false);
        toolbar.setDisplayMode(objc2_app_kit::NSToolbarDisplayMode::IconOnly);
        return toolbar;
    }
}

pub fn delegate(
    mtm: MainThreadMarker,
    library: &Library,
    theme: &Theme,
    target: &AnyObject,
) -> Retained<ToolbarDelegate> {
    let mut items: Vec<(Retained<NSString>, Retained<NSToolbarItem>)> = Vec::new();
    let mut push = |item: Retained<NSToolbarItem>| {
        items.push((item.itemIdentifier(), item));
    };

    push(menu_item(mtm, "scope", "display", "Display 1 · Space 1", &scope_menu(library, target), target));
    push(menu_item(mtm, "folder", "folder", "All Wallpapers", &folder_menu(library, target), target));
    push(pills(mtm, library));
    push(menu_item(mtm, "sort", "line.3.horizontal.decrease", "Sort & Filter", &sort_menu(target), target));
    push(layout_group(mtm, target));
    push(search(mtm));
    let add = menu_item(mtm, "add", "plus", "Add", &add_menu(target), target);
    add.setStyle(NSToolbarItemStyle::Prominent);
    add.setBackgroundTintColor(Some(&theme.fill));
    push(add);
    push(button_item(mtm, "settings", "gearshape", "Settings", target));

    let flexible = unsafe { NSToolbarFlexibleSpaceItemIdentifier };
    let separator = unsafe { NSToolbarInspectorTrackingSeparatorItemIdentifier };
    let toggle = unsafe { NSToolbarToggleInspectorItemIdentifier };
    let order: Vec<Retained<NSString>> = vec![
        NSString::from_str("scope"),
        NSString::from_str("folder"),
        flexible.copy(),
        NSString::from_str("pills"),
        flexible.copy(),
        NSString::from_str("sort"),
        NSString::from_str("layout"),
        NSString::from_str("search"),
        NSString::from_str("add"),
        separator.copy(),
        NSString::from_str("settings"),
        toggle.copy(),
    ];

    let this = ToolbarDelegate::alloc(mtm).set_ivars(DelegateIvars { items, order });
    return unsafe { msg_send![super(this), init] };
}

fn menu_item(
    mtm: MainThreadMarker,
    id: &str,
    icon: &str,
    title: &str,
    menu: &NSMenu,
    target: &AnyObject,
) -> Retained<NSToolbarItem> {
    let item = NSMenuToolbarItem::initWithItemIdentifier(NSMenuToolbarItem::alloc(mtm), &NSString::from_str(id));
    item.setMenu(menu);
    item.setShowsIndicator(id != "add" && id != "sort");
    if let Some(image) = widgets::symbol(icon, 13.0) {
        item.setImage(Some(&image));
    }
    if id == "scope" || id == "folder" {
        item.setTitle(&NSString::from_str(title));
    }
    item.setLabel(&NSString::from_str(title));
    item.setToolTip(Some(&NSString::from_str(title)));
    unsafe {
        item.setTarget(Some(target));
        item.setAction(Some(sel!(noop:)));
    }
    return Retained::into_super(item);
}

fn button_item(
    mtm: MainThreadMarker,
    id: &str,
    icon: &str,
    title: &str,
    target: &AnyObject,
) -> Retained<NSToolbarItem> {
    let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), &NSString::from_str(id));
    if let Some(image) = widgets::symbol(icon, 13.0) {
        item.setImage(Some(&image));
    }
    item.setLabel(&NSString::from_str(title));
    item.setToolTip(Some(&NSString::from_str(title)));
    unsafe {
        item.setTarget(Some(target));
        item.setAction(Some(sel!(noop:)));
    }
    return item;
}

fn layout_group(mtm: MainThreadMarker, target: &AnyObject) -> Retained<NSToolbarItem> {
    let images: Vec<Retained<NSImage>> =
        ["square.grid.2x2", "list.bullet"].iter().filter_map(|name| widgets::symbol(name, 13.0)).collect();
    let labels = [NSString::from_str("Grid"), NSString::from_str("List")];
    let group = unsafe {
        NSToolbarItemGroup::groupWithItemIdentifier_images_selectionMode_labels_target_action(
            &NSString::from_str("layout"),
            &NSArray::from_retained_slice(&images),
            NSToolbarItemGroupSelectionMode::SelectOne,
            Some(&NSArray::from_retained_slice(&labels)),
            Some(target),
            Some(sel!(noop:)),
            mtm,
        )
    };
    group.setSelectedIndex(0);
    return Retained::into_super(group);
}

fn search(mtm: MainThreadMarker) -> Retained<NSToolbarItem> {
    let item =
        NSSearchToolbarItem::initWithItemIdentifier(NSSearchToolbarItem::alloc(mtm), &NSString::from_str("search"));
    item.searchField().setPlaceholderString(Some(&NSString::from_str("Search")));
    item.setPreferredWidthForSearchField(200.0);
    return Retained::into_super(item);
}

fn pills(mtm: MainThreadMarker, library: &Library) -> Retained<NSToolbarItem> {
    let mut views: Vec<Retained<NSView>> = Vec::new();
    if let Some(index) = library.desktop() {
        views.push(pill(mtm, library, index, "Desktop", "menubar.dock.rectangle"));
    }
    if let Some(index) = library.lock() {
        views.push(pill(mtm, library, index, "Lock Screen", "lock.fill"));
    }
    let refs: Vec<&NSView> = views.iter().map(|view| &**view).collect();
    let stack = widgets::stack(mtm, &refs, false, 6.0);
    let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), &NSString::from_str("pills"));
    item.setView(Some(&stack));
    item.setLabel(&NSString::from_str("Now Showing"));
    return item;
}

fn pill(mtm: MainThreadMarker, library: &Library, index: usize, caption: &str, icon: &str) -> Retained<NSView> {
    let item = &library.items[index];
    let thumb = NSView::new(mtm);
    thumb.setWantsLayer(true);
    let layer = CALayer::new();
    layer.setCornerRadius(5.0);
    layer.setMasksToBounds(true);
    layer.setContentsGravity(unsafe { kCAGravityResizeAspectFill });
    if let Some(path) = &item.thumb
        && let Some(image) =
            NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(&path.to_string_lossy()))
    {
        unsafe { layer.setContents(Some(&image)) };
    }
    thumb.setLayer(Some(&layer));
    thumb.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(36.0, 22.5)));
    thumb.widthAnchor().constraintEqualToConstant(36.0).setActive(true);
    thumb.heightAnchor().constraintEqualToConstant(22.5).setActive(true);
    let glyph = widgets::symbol_view(mtm, icon, 10.0, &NSColor::secondaryLabelColor());
    let top = widgets::label(mtm, caption, 10.0, &Weight::Medium, &NSColor::secondaryLabelColor());
    let caption_row = widgets::stack(mtm, &[&glyph, &top], false, 3.0);
    let title = widgets::label(mtm, &item.title, 12.0, &Weight::Semibold, &NSColor::labelColor());
    title.widthAnchor().constraintLessThanOrEqualToConstant(120.0).setActive(true);
    let text = widgets::stack(mtm, &[&caption_row, &title], true, 0.0);
    text.setAlignment(objc2_app_kit::NSLayoutAttribute::Leading);
    let row = widgets::stack(mtm, &[&thumb, &text], false, 8.0);
    row.setEdgeInsets(widgets::insets(0.0, 4.0, 0.0, 10.0));
    let view = PillView::alloc(mtm).set_ivars(PillIvars { index });
    let view: Retained<PillView> = unsafe { msg_send![super(view), init] };
    view.addSubview(&row);
    widgets::pin(&row, &view, 0.0);
    return Retained::into_super(view);
}

fn entry(title: &str, target: &AnyObject, checked: bool, indent: isize) -> Retained<NSMenuItem> {
    let mtm = MainThreadMarker::new().unwrap_or_else(|| unreachable!());
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(sel!(noop:)),
            &NSString::new(),
        )
    };
    unsafe { item.setTarget(Some(target)) };
    item.setState(isize::from(checked));
    item.setIndentationLevel(indent);
    return item;
}

fn header(title: &str) -> Retained<NSMenuItem> {
    let mtm = MainThreadMarker::new().unwrap_or_else(|| unreachable!());
    return NSMenuItem::sectionHeaderWithTitle(&NSString::from_str(title), mtm);
}

fn menu(items: &[Retained<NSMenuItem>]) -> Retained<NSMenu> {
    let mtm = MainThreadMarker::new().unwrap_or_else(|| unreachable!());
    let menu = NSMenu::new(mtm);
    for item in items {
        menu.addItem(item);
    }
    return menu;
}

fn separator() -> Retained<NSMenuItem> {
    let mtm = MainThreadMarker::new().unwrap_or_else(|| unreachable!());
    return NSMenuItem::separatorItem(mtm);
}

fn scope_menu(library: &Library, target: &AnyObject) -> Retained<NSMenu> {
    let mut items = vec![entry("All Displays", target, false, 0)];
    for (display, info) in library.displays.iter().enumerate() {
        items.push(separator());
        items.push(header(&format!("Display {} — {}", display + 1, info.name)));
        items.push(entry("All Spaces", target, false, 1));
        for space in 0..info.spaces {
            let checked = display == 0 && space == 0;
            let used = library.assignments.iter().any(|(target, _)| *target == Target::Space { display, space });
            let title = if used { format!("Space {} ·", space + 1) } else { format!("Space {}", space + 1) };
            items.push(entry(title.trim_end_matches(" ·"), target, checked, 1));
        }
    }
    return menu(&items);
}

fn folder_menu(library: &Library, target: &AnyObject) -> Retained<NSMenu> {
    let mut items = vec![entry("All Wallpapers", target, true, 0), separator()];
    items.extend(library.folders.iter().map(|name| entry(name, target, false, 0)));
    items.push(separator());
    items.push(entry("New Folder…", target, false, 0));
    return menu(&items);
}

fn sort_menu(target: &AnyObject) -> Retained<NSMenu> {
    return menu(&[
        header("Sort By"),
        entry("Name (A–Z)", target, true, 0),
        entry("Name (Z–A)", target, false, 0),
        entry("Date Added (Newest)", target, false, 0),
        entry("Date Added (Oldest)", target, false, 0),
        separator(),
        header("Show"),
        entry("All Kinds", target, true, 0),
        entry("Videos", target, false, 0),
        entry("GIFs", target, false, 0),
        entry("Images", target, false, 0),
    ]);
}

fn add_menu(target: &AnyObject) -> Retained<NSMenu> {
    return menu(&[
        entry("Add Files…", target, false, 0),
        entry("Add Folder…", target, false, 0),
        separator(),
        entry("Download from URL…", target, false, 0),
    ]);
}
