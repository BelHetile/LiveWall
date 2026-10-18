use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, available, define_class, msg_send, sel};
use objc2_app_kit::{
    NSColor, NSControl, NSControlSize, NSControlTextEditingDelegate, NSFont, NSImage, NSImageSymbolConfiguration,
    NSImageSymbolScale, NSImageView, NSLayoutAttribute, NSLineBreakMode, NSMenu, NSMenuItem, NSMenuToolbarItem,
    NSPopUpButton, NSSearchField, NSSearchFieldDelegate, NSSearchToolbarItem, NSStackView, NSTextField,
    NSTextFieldDelegate, NSTextView, NSToolbar, NSToolbarDelegate, NSToolbarDisplayMode,
    NSToolbarFlexibleSpaceItemIdentifier, NSToolbarInspectorTrackingSeparatorItemIdentifier, NSToolbarItem,
    NSToolbarItemGroup, NSToolbarItemGroupControlRepresentation, NSToolbarItemGroupSelectionMode,
    NSToolbarItemIdentifier, NSToolbarItemStyle, NSToolbarItemVisibilityPriorityHigh,
    NSToolbarItemVisibilityPriorityLow, NSToolbarSidebarTrackingSeparatorItemIdentifier, NSToolbarSpaceItemIdentifier,
    NSToolbarToggleInspectorItemIdentifier, NSToolbarToggleSidebarItemIdentifier, NSUserInterfaceLayoutOrientation,
    NSView,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_foundation::{NSArray, NSCopying, NSObject, NSObjectProtocol, NSString};

use crate::app::{PlayState, Sound};
use crate::i18n::{self, Arg, tr, trf};
use crate::ui::state::{Filter, KINDS, Layout, Scope, Sort, showing};
use crate::ui::view::{Download, Library, Stage, Status};
use crate::ui::{dialogs, ns};

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

        #[unsafe(method(sort:))]
        fn sort(&self, sender: &NSMenuItem) {
            if let Some((sort, _)) = usize::try_from(sender.tag()).ok().and_then(|index| Sort::ALL.get(index)) {
                crate::ui::set_sort(*sort);
            }
        }

        #[unsafe(method(kind:))]
        fn kind(&self, sender: &NSMenuItem) {
            if let Some((kind, _)) = usize::try_from(sender.tag()).ok().and_then(|index| KINDS.get(index)) {
                crate::ui::set_kind(*kind);
            }
        }

        #[unsafe(method(search:))]
        fn search(&self, sender: &NSSearchField) {
            crate::ui::set_query(sender.stringValue().to_string());
        }

        #[unsafe(method(addFiles:))]
        fn add_files(&self, _sender: Option<&NSMenuItem>) {
            dialogs::add_files(self.mtm());
        }

        #[unsafe(method(downloadURL:))]
        fn download_url(&self, _sender: Option<&NSMenuItem>) {
            dialogs::download_sheet(self.mtm());
        }

        #[unsafe(method(showDesktop:))]
        fn show_desktop(&self, _sender: Option<&NSToolbarItem>) {
            crate::ui::reveal_desktop();
        }

        #[unsafe(method(showLock:))]
        fn show_lock(&self, _sender: Option<&NSToolbarItem>) {
            crate::ui::reveal_lock();
        }
    }

    unsafe impl NSControlTextEditingDelegate for ToolbarDelegate {
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn command(&self, _control: &NSControl, _editor: &NSTextView, selector: Sel) -> Bool {
            if selector == sel!(cancelOperation:) {
                let search = &self.ivars().search;
                search.searchField().setStringValue(&ns(""));
                search.endSearchInteraction();
                crate::ui::set_query(String::new());
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
    layout: Retained<NSToolbarItemGroup>,
    search: Retained<NSSearchToolbarItem>,
    sorts: Vec<Retained<NSMenuItem>>,
    kinds: Vec<Retained<NSMenuItem>>,
    desktop: Retained<NSToolbarItem>,
    lock: Retained<NSToolbarItem>,
    status: StatusBadge,
    _delegate: Retained<ToolbarDelegate>,
}

impl Toolbar {
    #[allow(clippy::too_many_lines)]
    pub fn new(mtm: MainThreadMarker) -> Self {
        let (scope, popup) = scope_item(mtm);
        let sort = NSMenuToolbarItem::initWithItemIdentifier(NSMenuToolbarItem::alloc(mtm), &ns("sort"));
        sort.setShowsIndicator(false);
        sort.setImage(symbol("arrow.up.arrow.down").as_deref());
        sort.setLabel(&ns(&tr("Sort & Filter")));
        sort.setPaletteLabel(&ns(&tr("Sort & Filter")));
        sort.setToolTip(Some(&ns(&tr("Sort & Filter"))));
        let sorts = choices(mtm, Sort::ALL.iter().map(|(_, title)| *title), sel!(sort:));
        let kinds = choices(mtm, KINDS.iter().map(|(_, title)| *title), sel!(kind:));
        sort.setMenu(&sort_menu(mtm, &sorts, &kinds));
        let layout = layout_group(mtm);
        let search = NSSearchToolbarItem::initWithItemIdentifier(NSSearchToolbarItem::alloc(mtm), &ns("search"));
        search.setLabel(&ns(&tr("Search")));
        search.setPaletteLabel(&ns(&tr("Search")));
        search.setToolTip(Some(&ns(&tr("Search"))));
        search.searchField().setPlaceholderString(Some(&ns(&tr("Search"))));
        search.searchField().setSendsSearchStringImmediately(true);
        let add = NSMenuToolbarItem::initWithItemIdentifier(NSMenuToolbarItem::alloc(mtm), &ns("add"));
        add.setShowsIndicator(false);
        add.setImage(symbol("plus").as_deref());
        add.setLabel(&ns(&tr("Add")));
        add.setPaletteLabel(&ns(&tr("Add")));
        add.setToolTip(Some(&ns(&tr("Add wallpapers from files, a folder or a video link"))));
        let adds = [(tr("Add Files…"), sel!(addFiles:)), (tr("Download from URL…"), sel!(downloadURL:))].map(
            |(title, action)| {
                let item = menu_item(mtm, &title);
                unsafe { item.setAction(Some(action)) };
                return item;
            },
        );
        add.setMenu(&add_menu(mtm, &adds));
        if available!(macos = 26.0) {
            add.setStyle(NSToolbarItemStyle::Prominent);
        }
        let desktop = preview_item(mtm, "desktop", "Desktop", sel!(showDesktop:));
        let lock = preview_item(mtm, "lock", "Lock Screen", sel!(showLock:));
        let status = StatusBadge::new(mtm);
        let (low, high) = (NSToolbarItemVisibilityPriorityLow, NSToolbarItemVisibilityPriorityHigh);
        for item in [&status.item, &desktop, &lock] {
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
            status.item.clone(),
            desktop.clone(),
            lock.clone(),
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
            for item in sorts.iter().chain(&kinds).chain(&adds) {
                item.setTarget(Some(&delegate));
            }
            for item in [&desktop, &lock] {
                item.setTarget(Some(&delegate));
            }
            search.searchField().setTarget(Some(&delegate));
            search.searchField().setAction(Some(sel!(search:)));
        }
        unsafe { search.searchField().setDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
        let toolbar = NSToolbar::initWithIdentifier(NSToolbar::alloc(mtm), &ns("library"));
        toolbar.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        toolbar.setAllowsUserCustomization(false);
        toolbar.setAllowsDisplayModeCustomization(false);
        toolbar.setDisplayMode(NSToolbarDisplayMode::IconOnly);
        return Self {
            bar: toolbar,
            scope: popup,
            layout,
            search,
            sorts,
            kinds,
            desktop,
            lock,
            status,
            _delegate: delegate,
        };
    }
}

impl Toolbar {
    pub fn show_layout(&self, layout: Layout) {
        self.layout.setSelectedIndex(layout.index());
    }

    pub fn show_query(&self, query: &str) {
        self.search.searchField().setStringValue(&ns(query));
    }

    pub fn find(&self) {
        if !self.bar.isVisible() {
            self.bar.setVisible(true);
        }
        self.search.beginSearchInteraction();
    }

    pub fn reload(&self, library: &Library, filter: &Filter) {
        let mtm = self.scope.mtm();
        let menu = NSMenu::new(mtm);
        let entry = |scope: Scope| {
            let item = menu_item(mtm, &scope.title());
            item.setTag(scope.tag());
            menu.addItem(&item);
        };
        entry(Scope::All);
        if library.desktops > 0 {
            let separator = NSMenuItem::separatorItem(mtm);
            separator.setTag(isize::MIN);
            menu.addItem(&separator);
        }
        for space in 0..library.desktops {
            entry(Scope::Space(space));
        }
        self.scope.setMenu(Some(&menu));
        self.scope.selectItemWithTag(filter.scope.tag());
        self.scope.sizeToFit();
        for (item, (sort, _)) in self.sorts.iter().zip(Sort::ALL) {
            item.setState(isize::from(filter.sort == sort));
        }
        for (item, (kind, _)) in self.kinds.iter().zip(KINDS) {
            item.setState(isize::from(filter.kind == kind));
        }
        let desktop = desktop_item(library, filter.scope);
        show_wallpaper(&self.desktop, "desktopcomputer", "Desktop", library, desktop, None);
        let lock = lock_item(library);
        let still = library.lock.as_ref().and_then(|lock| lock.image.clone());
        show_wallpaper(&self.lock, "lock", "Lock Screen", library, lock, still);
        self.status.update(library);
    }
}

pub fn desktop_item(library: &Library, scope: Scope) -> Option<usize> {
    return open_item(library, showing(library, scope).main);
}

pub fn lock_item(library: &Library) -> Option<usize> {
    return open_item(library, library.lock());
}

fn open_item(library: &Library, index: Option<usize>) -> Option<usize> {
    return index.filter(|index| {
        library.items.get(*index).is_some_and(|item| item.folder.is_none_or(|folder| !library.sealed(folder)))
    });
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

const PREVIEW_GLYPH: CGFloat = 15.0;
const PREVIEW_GAP: CGFloat = 6.0;
const PREVIEW_THUMBNAIL: CGSize = CGSize::new(42.0, 24.0);
const PREVIEW_PLACEHOLDER: CGFloat = 14.0;
const STATUS_TEXT_WIDTH: CGFloat = 150.0;

fn glyph_and_thumbnail(symbol_name: &str, thumbnail: Option<Retained<NSImage>>) -> Option<Retained<NSImage>> {
    let tint = |color: &NSColor| NSImageSymbolConfiguration::configurationWithHierarchicalColor(color);
    let glyph = symbol(symbol_name)?.imageWithSymbolConfiguration(&tint(&NSColor::labelColor()))?;
    let placeholder = symbol("photo")?.imageWithSymbolConfiguration(&tint(&NSColor::secondaryLabelColor()))?;
    let size = CGSize::new(PREVIEW_GLYPH + PREVIEW_GAP + PREVIEW_THUMBNAIL.width, PREVIEW_THUMBNAIL.height);
    let draw = RcBlock::new(move |_bounds: CGRect| -> Bool {
        let glyph_rect = CGRect::new(
            CGPoint::new(0.0, (size.height - PREVIEW_GLYPH) / 2.0),
            CGSize::new(PREVIEW_GLYPH, PREVIEW_GLYPH),
        );
        glyph.drawInRect(glyph_rect);
        let thumbnail_rect = CGRect::new(CGPoint::new(PREVIEW_GLYPH + PREVIEW_GAP, 0.0), PREVIEW_THUMBNAIL);
        if let Some(thumbnail) = &thumbnail {
            thumbnail.drawInRect(thumbnail_rect);
        } else {
            let rect = CGRect::new(
                CGPoint::new(
                    thumbnail_rect.origin.x + (PREVIEW_THUMBNAIL.width - PREVIEW_PLACEHOLDER) / 2.0,
                    (size.height - PREVIEW_PLACEHOLDER) / 2.0,
                ),
                CGSize::new(PREVIEW_PLACEHOLDER, PREVIEW_PLACEHOLDER),
            );
            placeholder.drawInRect(rect);
        }
        return Bool::YES;
    });
    return Some(NSImage::imageWithSize_flipped_drawingHandler(size, false, &draw));
}

fn preview_item(mtm: MainThreadMarker, identifier: &str, title: &str, action: Sel) -> Retained<NSToolbarItem> {
    let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), &ns(identifier));
    item.setLabel(&ns(&tr(title)));
    item.setPaletteLabel(&ns(&tr(title)));
    item.setAutovalidates(false);
    unsafe { item.setAction(Some(action)) };
    return item;
}

fn show_wallpaper(
    item: &NSToolbarItem,
    glyph: &str,
    title: &str,
    library: &Library,
    index: Option<usize>,
    still: Option<Retained<NSImage>>,
) {
    let entry = index.and_then(|index| library.items.get(index));
    let thumbnail = entry.and_then(|entry| still.or_else(|| entry.image.clone()));
    item.setImage(glyph_and_thumbnail(glyph, thumbnail).as_deref());
    item.setEnabled(entry.is_some());
    let tip = entry.map_or_else(|| tr(title), |entry| entry.title.clone());
    item.setToolTip(Some(&ns(&tip)));
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
    fn color(self) -> Retained<NSColor> {
        return match self {
            Self::Off => NSColor::tertiaryLabelColor(),
            Self::Green => NSColor::systemGreenColor(),
            Self::Yellow => NSColor::systemYellowColor(),
            Self::Red => NSColor::systemRedColor(),
            Self::Accent => NSColor::controlAccentColor(),
        };
    }
}

struct StatusBadge {
    item: Retained<NSToolbarItem>,
    stack: Retained<NSStackView>,
    light: Retained<NSImageView>,
    state: Retained<NSTextField>,
    reason: Retained<NSTextField>,
    sound: Retained<NSImageView>,
}

impl StatusBadge {
    fn new(mtm: MainThreadMarker) -> Self {
        let small = NSImageSymbolConfiguration::configurationWithScale(NSImageSymbolScale::Small);
        let light = NSImageView::new(mtm);
        light.setSymbolConfiguration(Some(&small));
        light.setImage(symbol("circle.fill").as_deref());
        let state = NSTextField::labelWithString(&ns(""), mtm);
        state.setFont(Some(&NSFont::systemFontOfSize(NSFont::smallSystemFontSize())));
        let reason = NSTextField::labelWithString(&ns(""), mtm);
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
        sound.setSymbolConfiguration(Some(&small));
        let views: [&NSView; 3] = [&light, &text, &sound];
        let stack = NSStackView::stackViewWithViews(&NSArray::from_slice(&views), mtm);
        let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), &ns("status"));
        item.setLabel(&ns(&tr("Playback")));
        item.setPaletteLabel(&ns(&tr("Playback")));
        item.setView(Some(&stack));
        item.setBordered(false);
        return Self { item, stack, light, state, reason, sound };
    }

    fn update(&self, library: &Library) {
        let (light, state, brief, tip) = match &library.status {
            None => (Light::Off, tr("Nothing playing"), None, tr("No wallpaper on this desktop")),
            Some(status) => describe(status),
        };
        let progress = progress(library);
        self.light.setContentTintColor(Some(&light.color()));
        self.state.setStringValue(&ns(&state));
        let line = progress.clone().or(brief);
        self.reason.setStringValue(&ns(line.as_deref().unwrap_or("")));
        self.reason.setHidden(line.is_none());
        let tip = match progress {
            Some(progress) => format!("{tip}\n{progress}"),
            None => tip,
        };
        self.stack.setToolTip(Some(&ns(&tip)));
        let sound = library.status.as_ref().and_then(|status| status.sound);
        self.sound.setHidden(sound.is_none());
        if let Some(sound) = sound {
            let (name, light, tip) = match sound {
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
                    tr("Listening to the inspector preview; this desktop’s copy is muted until you stop"),
                ),
                Sound::Preview { here: false } => {
                    ("headphones", Light::Accent, tr("Listening to a preview of another wallpaper in the inspector"))
                }
            };
            self.sound.setImage(symbol(name).as_deref());
            self.sound.setContentTintColor(Some(&light.color()));
            self.sound.setToolTip(Some(&ns(&tip)));
        }
    }
}

fn describe(status: &Status) -> (Light, String, Option<String>, String) {
    let reason = status.reason.as_ref();
    let brief = reason.map(|(brief, _)| brief.clone()).filter(|brief| !brief.is_empty());
    let full = reason.map_or_else(String::new, |(_, full)| full.clone());
    return match status.state {
        PlayState::Still => (Light::Green, tr("Image"), None, tr("This desktop shows a still image")),
        PlayState::Playing => (Light::Green, tr("Playing"), None, tr("The wallpaper on this desktop is playing")),
        PlayState::Paused if full.is_empty() => (Light::Yellow, tr("Paused"), brief, tr("Paused")),
        PlayState::Paused => (Light::Yellow, tr("Paused"), brief, trf("Paused: %@", &[Arg::Text(full)])),
        PlayState::Unloaded => {
            let lead =
                if full.is_empty() { String::new() } else { format!("{}. ", trf("Paused: %@", &[Arg::Text(full)])) };
            (
                Light::Red,
                tr("Unloaded"),
                brief,
                format!("{lead}{}", tr("Unloaded to free memory; the desktop shows the paused frame.")),
            )
        }
    };
}

fn progress(library: &Library) -> Option<String> {
    let downloads = &library.downloads;
    if let [only] = downloads.as_slice() {
        return Some(download_line(only));
    }
    if !downloads.is_empty() {
        let count = i64::try_from(downloads.len()).unwrap_or(0);
        return Some(trf(
            "Downloading %ld videos · %@",
            &[Arg::Int(count), Arg::Text(i18n::percent(overall(downloads)))],
        ));
    }
    let running: Vec<f64> = library.items.iter().filter_map(|item| item.progress).collect();
    if running.is_empty() {
        return None;
    }
    let mean = running.iter().sum::<f64>() / f64::from(u32::try_from(running.len()).unwrap_or(u32::MAX));
    return Some(format!("{} {}", tr("Optimizing…"), i18n::percent(mean)));
}

fn overall(downloads: &[Download]) -> f64 {
    let count = f64::from(u32::try_from(downloads.len()).unwrap_or(u32::MAX));
    return downloads.iter().map(|download| download.percent).sum::<f64>() / count;
}

fn download_line(download: &Download) -> String {
    return match download.stage {
        Stage::Preparing => tr("Getting video info…"),
        Stage::Merging => tr("Merging video and audio…"),
        Stage::Importing => tr("Adding to library…"),
        Stage::Downloading => {
            let mut parts = vec![trf("Downloading %@", &[Arg::Text(i18n::percent(download.percent))])];
            if let Some(speed) = download.speed.filter(|speed| *speed > 0.0) {
                #[allow(clippy::cast_possible_truncation)]
                let rate = i18n::bytes(speed as i64);
                parts.push(trf("%@/s", &[Arg::Text(rate)]));
            }
            if let Some(eta) = download.eta.filter(|eta| eta.is_finite() && *eta >= 0.0) {
                parts.push(trf("%@ left", &[Arg::Text(clock(eta))]));
            }
            parts.join(" · ")
        }
    };
}

fn clock(seconds: f64) -> String {
    let total = std::time::Duration::try_from_secs_f64(seconds.round()).map_or(0, |duration| duration.as_secs());
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        return format!("{hours}:{minutes:02}:{seconds:02}");
    }
    return format!("{minutes}:{seconds:02}");
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

fn choices<'a>(mtm: MainThreadMarker, titles: impl Iterator<Item = &'a str>, action: Sel) -> Vec<Retained<NSMenuItem>> {
    return titles
        .enumerate()
        .map(|(index, title)| {
            let item = menu_item(mtm, &tr(title));
            item.setTag(isize::try_from(index).unwrap_or(0));
            unsafe { item.setAction(Some(action)) };
            return item;
        })
        .collect();
}

fn sort_menu(
    mtm: MainThreadMarker,
    sorts: &[Retained<NSMenuItem>],
    kinds: &[Retained<NSMenuItem>],
) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    menu.addItem(&NSMenuItem::sectionHeaderWithTitle(&ns(&tr("Sort By")), mtm));
    for item in sorts {
        menu.addItem(item);
    }
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&NSMenuItem::sectionHeaderWithTitle(&ns(&tr("Show")), mtm));
    for item in kinds {
        menu.addItem(item);
    }
    return menu;
}

fn add_menu(mtm: MainThreadMarker, items: &[Retained<NSMenuItem>]) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    for item in items {
        menu.addItem(item);
    }
    return menu;
}
