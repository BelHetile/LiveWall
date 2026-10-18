use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject, Sel};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, available, define_class, msg_send, sel};
use objc2_app_kit::{
    NSToolbarItemVisibilityPriorityHigh, NSToolbarItemVisibilityPriorityLow,
    NSColor, NSControl, NSControlSize, NSControlTextEditingDelegate, NSFont, NSImage, NSImageSymbolConfiguration,
    NSImageView, NSLayoutAttribute, NSLineBreakMode, NSMenu, NSMenuItem, NSMenuToolbarItem, NSPopUpButton,
    NSSearchFieldDelegate, NSSearchToolbarItem, NSStackView, NSTextField, NSTextFieldDelegate, NSTextView, NSToolbar,
    NSToolbarDelegate, NSToolbarDisplayMode, NSToolbarFlexibleSpaceItemIdentifier,
    NSToolbarInspectorTrackingSeparatorItemIdentifier, NSToolbarItem, NSToolbarItemGroup,
    NSToolbarItemGroupControlRepresentation, NSToolbarItemGroupSelectionMode, NSToolbarItemIdentifier,
    NSToolbarItemStyle, NSToolbarSidebarTrackingSeparatorItemIdentifier, NSToolbarSpaceItemIdentifier,
    NSToolbarToggleInspectorItemIdentifier, NSToolbarToggleSidebarItemIdentifier, NSUserInterfaceLayoutOrientation,
    NSView,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_foundation::{NSArray, NSCopying, NSObject, NSObjectProtocol, NSString};

use crate::i18n::tr;
use crate::ui::ns;
use crate::ui::state::{KINDS, Layout, Scope, Sort};

pub struct DelegateIvars {
    items: Vec<(Retained<NSString>, Retained<NSToolbarItem>)>,
    order: Vec<Retained<NSString>>,
    search: Retained<NSSearchToolbarItem>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateIvars]
    struct ToolbarDelegate;

    unsafe impl NSObjectProtocol for ToolbarDelegate {}

    impl ToolbarDelegate {
        #[unsafe(method(layout:))]
        fn layout(&self, sender: &NSToolbarItemGroup) {
            crate::ui::set_layout(Layout::from_index(sender.selectedIndex()));
        }

        #[unsafe(method(scope:))]
        fn scope(&self, sender: &NSPopUpButton) {
            crate::ui::set_scope(Scope::from_tag(sender.selectedTag()));
        }
    }

    unsafe impl NSControlTextEditingDelegate for ToolbarDelegate {
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn command(&self, _control: &NSControl, _editor: &NSTextView, selector: Sel) -> Bool {
            if selector == sel!(cancelOperation:) {
                let search = &self.ivars().search;
                search.searchField().setStringValue(&ns(""));
                search.endSearchInteraction();
                Bool::YES
            } else {
                Bool::NO
            }
        }
    }

    unsafe impl NSTextFieldDelegate for ToolbarDelegate {}

    unsafe impl NSSearchFieldDelegate for ToolbarDelegate {}

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
    fn new(
        mtm: MainThreadMarker,
        items: Vec<Retained<NSToolbarItem>>,
        order: Vec<Retained<NSString>>,
        search: Retained<NSSearchToolbarItem>,
    ) -> Retained<Self> {
        let items = items.into_iter().map(|item| (item.itemIdentifier(), item)).collect();
        let delegate = ToolbarDelegate::alloc(mtm).set_ivars(DelegateIvars { items, order, search });
        let delegate: Retained<ToolbarDelegate> = unsafe { msg_send![super(delegate), init] };
        return delegate;
    }
}

pub struct Toolbar {
    pub bar: Retained<NSToolbar>,
    scope: Retained<NSPopUpButton>,
    _delegate: Retained<ToolbarDelegate>,
}

impl Toolbar {
    pub fn new(mtm: MainThreadMarker, target: &AnyObject) -> Self {
        let (scope, popup) = scope_item(mtm);
        let sort = NSMenuToolbarItem::initWithItemIdentifier(NSMenuToolbarItem::alloc(mtm), &ns("sort"));
        sort.setShowsIndicator(false);
        sort.setImage(symbol("arrow.up.arrow.down").as_deref());
        sort.setLabel(&ns(&tr("Sort & Filter")));
        sort.setPaletteLabel(&ns(&tr("Sort & Filter")));
        sort.setToolTip(Some(&ns(&tr("Sort & Filter"))));
        sort.setMenu(&sort_menu(mtm));
        let layout = layout_group(mtm);
        let search = NSSearchToolbarItem::initWithItemIdentifier(NSSearchToolbarItem::alloc(mtm), &ns("search"));
        search.setLabel(&ns(&tr("Search")));
        search.setPaletteLabel(&ns(&tr("Search")));
        search.setToolTip(Some(&ns(&tr("Search"))));
        search.searchField().setPlaceholderString(Some(&ns(&tr("Search"))));
        let add = NSMenuToolbarItem::initWithItemIdentifier(NSMenuToolbarItem::alloc(mtm), &ns("add"));
        add.setShowsIndicator(false);
        add.setImage(symbol("plus").as_deref());
        add.setLabel(&ns(&tr("Add")));
        add.setPaletteLabel(&ns(&tr("Add")));
        add.setToolTip(Some(&ns(&tr("Add wallpapers from files, a folder or a video link"))));
        add.setMenu(&add_menu(mtm));
        if available!(macos = 26.0) {
            add.setStyle(NSToolbarItemStyle::Prominent);
        }
        let desktop = preview_item(
            mtm,
            "desktop",
            "desktopcomputer",
            "Desktop",
            HARDCODED_DESKTOP_WALLPAPER,
            target,
            sel!(showDesktopWallpaper:),
        );
        let lock = preview_item(
            mtm,
            "lock",
            "lock",
            "Lock Screen",
            HARDCODED_LOCK_WALLPAPER,
            target,
            sel!(showLockWallpaper:),
        );
        let status = status_item(mtm);
        let (low, high) = (NSToolbarItemVisibilityPriorityLow, NSToolbarItemVisibilityPriorityHigh);
        for item in [&status, &desktop, &lock] {
            item.setVisibilityPriority(low);
        }
        sort.setVisibilityPriority(high);
        layout.setVisibilityPriority(high);
        add.setVisibilityPriority(high);
        let order = vec![
            unsafe { NSToolbarToggleSidebarItemIdentifier }.copy(),
            unsafe { NSToolbarSidebarTrackingSeparatorItemIdentifier }.copy(),
            ns("scope"),
            unsafe { NSToolbarFlexibleSpaceItemIdentifier }.copy(),
            ns("status"),
            ns("desktop"),
            ns("lock"),
            unsafe { NSToolbarSpaceItemIdentifier }.copy(),
            ns("sort"),
            ns("layout"),
            ns("search"),
            unsafe { NSToolbarSpaceItemIdentifier }.copy(),
            ns("add"),
            unsafe { NSToolbarInspectorTrackingSeparatorItemIdentifier }.copy(),
            unsafe { NSToolbarToggleInspectorItemIdentifier }.copy(),
        ];
        let items = vec![
            scope,
            status,
            desktop,
            lock,
            Retained::into_super(sort),
            Retained::into_super(layout.clone()),
            Retained::into_super(search.clone()),
            Retained::into_super(add),
        ];
        let delegate = ToolbarDelegate::new(mtm, items, order, search.clone());
        unsafe {
            layout.setTarget(Some(&delegate));
            layout.setAction(Some(sel!(layout:)));
            popup.setTarget(Some(&delegate));
            popup.setAction(Some(sel!(scope:)));
        }
        unsafe { search.searchField().setDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
        let toolbar = NSToolbar::initWithIdentifier(NSToolbar::alloc(mtm), &ns("library"));
        toolbar.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        toolbar.setAllowsUserCustomization(false);
        toolbar.setAllowsDisplayModeCustomization(false);
        toolbar.setDisplayMode(NSToolbarDisplayMode::IconOnly);
        return Self { bar: toolbar, scope: popup, _delegate: delegate };
    }
}

impl Toolbar {
    pub fn reload(&self, desktops: usize, current: Scope) {
        let mtm = self.scope.mtm();
        let menu = NSMenu::new(mtm);
        let entry = |scope: Scope| {
            let item = menu_item(mtm, &scope.title());
            item.setTag(scope.tag());
            menu.addItem(&item);
        };
        entry(Scope::All);
        if desktops > 0 {
            menu.addItem(&NSMenuItem::separatorItem(mtm));
        }
        for space in 0..desktops {
            entry(Scope::Space(space));
        }
        self.scope.setMenu(Some(&menu));
        self.scope.selectItemWithTag(current.tag());
        self.scope.sizeToFit();
    }
}

fn symbol(name: &str) -> Option<Retained<NSImage>> {
    return NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(name), None);
}

fn scope_item(mtm: MainThreadMarker) -> (Retained<NSToolbarItem>, Retained<NSPopUpButton>) {
    let popup = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), CGRect::ZERO, false);
    popup.setControlSize(NSControlSize::Regular);
    let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), &ns("scope"));
    item.setLabel(&ns(&tr("Desktop")));
    item.setPaletteLabel(&ns(&tr("Desktop")));
    item.setToolTip(Some(&ns(&tr("Desktop"))));
    item.setView(Some(&popup));
    return (item, popup);
}

// !!! HARDCODED PLACEHOLDER !!! These two wallpaper files are NOT read from the library or the engine.
// !!! They are fixed files from ~/Pictures/wallpapers so the preview can be reviewed.
// !!! Real data comes from `view::Library::desktops(scope)` and `view::Library::lock()` once the Ui state exists.
const HARDCODED_DESKTOP_WALLPAPER: &str = "kimi no wa.jpg";
const HARDCODED_LOCK_WALLPAPER: &str = "raiden.jpg";
const PREVIEW_GLYPH: CGFloat = 15.0;
const PREVIEW_GAP: CGFloat = 6.0;
const PREVIEW_THUMBNAIL: CGSize = CGSize::new(42.0, 24.0);

fn wallpaper(file: &str) -> Option<Retained<NSImage>> {
    let home = std::env::var_os("HOME")?;
    let path = std::path::Path::new(&home).join("Pictures/wallpapers").join(file);
    return NSImage::initWithContentsOfFile(NSImage::alloc(), &ns(&path.to_string_lossy()));
}

fn glyph_and_thumbnail(symbol_name: &str, file: &str) -> Option<Retained<NSImage>> {
    let glyph = symbol(symbol_name)?;
    let tinted = NSImageSymbolConfiguration::configurationWithHierarchicalColor(&NSColor::labelColor());
    let glyph = glyph.imageWithSymbolConfiguration(&tinted)?;
    let thumbnail = wallpaper(file)?;
    let size = CGSize::new(PREVIEW_GLYPH + PREVIEW_GAP + PREVIEW_THUMBNAIL.width, PREVIEW_THUMBNAIL.height);
    let draw = RcBlock::new(move |_bounds: CGRect| -> Bool {
        let glyph_rect = CGRect::new(
            CGPoint::new(0.0, (size.height - PREVIEW_GLYPH) / 2.0),
            CGSize::new(PREVIEW_GLYPH, PREVIEW_GLYPH),
        );
        glyph.drawInRect(glyph_rect);
        let thumbnail_rect = CGRect::new(CGPoint::new(PREVIEW_GLYPH + PREVIEW_GAP, 0.0), PREVIEW_THUMBNAIL);
        thumbnail.drawInRect(thumbnail_rect);
        return Bool::YES;
    });
    return Some(NSImage::imageWithSize_flipped_drawingHandler(size, false, &draw));
}

// !!! HARDCODED PLACEHOLDER !!! The status is fixed to the LONGEST baseline text ("Unloaded · Low Power Mode", red)
// !!! and sound "all muted", to test toolbar space. It is NOT engine state.
// !!! Real values: `view::Status` (PlayState + reason + Sound) from `Engine::wallpaper_status`, as in baseline 562123b.
const STATUS_TEXT_WIDTH: CGFloat = 150.0;

fn status_item(mtm: MainThreadMarker) -> Retained<NSToolbarItem> {
    let light = NSImageView::new(mtm);
    light.setImage(symbol("circle.fill").as_deref());
    light.setContentTintColor(Some(&NSColor::systemRedColor()));
    light.setToolTip(Some(&ns(&tr("Unloaded to free memory; the desktop shows the paused frame."))));
    let state = NSTextField::labelWithString(&ns(&tr("Unloaded")), mtm);
    state.setFont(Some(&NSFont::systemFontOfSize(NSFont::smallSystemFontSize())));
    let reason = NSTextField::labelWithString(&ns(&tr("Low Power Mode")), mtm);
    reason.setTextColor(Some(&NSColor::secondaryLabelColor()));
    reason.setFont(Some(&NSFont::systemFontOfSize(NSFont::systemFontSizeForControlSize(NSControlSize::Mini))));
    reason.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    reason.widthAnchor().constraintLessThanOrEqualToConstant(STATUS_TEXT_WIDTH).setActive(true);
    let lines: [&NSView; 2] = [&state, &reason];
    let text = NSStackView::stackViewWithViews(&NSArray::from_slice(&lines), mtm);
    text.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
    text.setAlignment(NSLayoutAttribute::Leading);
    text.setSpacing(0.0);
    let sound = NSImageView::new(mtm);
    sound.setImage(symbol("speaker.slash.fill").as_deref());
    sound.setContentTintColor(Some(&NSColor::systemRedColor()));
    sound.setToolTip(Some(&ns(&tr("All wallpapers are muted from the menu bar or hotkey"))));
    let views: [&NSView; 3] = [&light, &text, &sound];
    let stack = NSStackView::stackViewWithViews(&NSArray::from_slice(&views), mtm);
    let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), &ns("status"));
    item.setLabel(&ns(&tr("Playback")));
    item.setPaletteLabel(&ns(&tr("Playback")));
    item.setView(Some(&stack));
    item.setBordered(false);
    return item;
}

fn preview_item(
    mtm: MainThreadMarker,
    identifier: &str,
    symbol_name: &str,
    title: &str,
    file: &str,
    target: &AnyObject,
    action: Sel,
) -> Retained<NSToolbarItem> {
    let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), &ns(identifier));
    item.setImage(glyph_and_thumbnail(symbol_name, file).as_deref());
    item.setLabel(&ns(&tr(title)));
    item.setPaletteLabel(&ns(&tr(title)));
    item.setToolTip(Some(&ns(&format!("{}: {}", tr(title), file.trim_end_matches(".jpg")))));
    unsafe {
        item.setTarget(Some(target));
        item.setAction(Some(action));
    }
    return item;
}

fn layout_group(mtm: MainThreadMarker) -> Retained<NSToolbarItemGroup> {
    let images: Vec<Retained<NSImage>> = Layout::ALL.iter().filter_map(|(_, icon, _)| symbol(icon)).collect();
    let labels: Vec<Retained<NSString>> = Layout::ALL.iter().map(|(_, _, title)| ns(&tr(title))).collect();
    let group = unsafe {
        NSToolbarItemGroup::groupWithItemIdentifier_images_selectionMode_labels_target_action(
            &ns("layout"),
            &NSArray::from_retained_slice(&images),
            NSToolbarItemGroupSelectionMode::SelectOne,
            Some(&NSArray::from_retained_slice(&labels)),
            None,
            None,
            mtm,
        )
    };
    group.setControlRepresentation(NSToolbarItemGroupControlRepresentation::Collapsed);
    group.setSelectedIndex(0);
    return group;
}

fn menu_item(mtm: MainThreadMarker, title: &str) -> Retained<NSMenuItem> {
    let item = NSMenuItem::new(mtm);
    item.setTitle(&ns(title));
    return item;
}

fn sort_menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    for (_, title) in Sort::ALL {
        menu.addItem(&menu_item(mtm, &tr(title)));
    }
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    for (_, title) in KINDS {
        menu.addItem(&menu_item(mtm, &tr(title)));
    }
    return menu;
}

fn add_menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    menu.addItem(&menu_item(mtm, &tr("Add Files…")));
    menu.addItem(&menu_item(mtm, &tr("Download from URL…")));
    return menu;
}
