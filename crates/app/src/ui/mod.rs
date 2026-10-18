mod actions;
mod controls;
mod dialogs;
mod folders;
mod inspector;
mod menus;
mod metrics;
mod picker;
mod settings;
mod sidebar;
mod state;
mod swatches;
mod theme;
mod toolbar;
mod view;
mod wall;
mod widgets;
mod windows;

use std::cell::RefCell;
use std::collections::HashSet;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

use block2::{DynBlock, RcBlock};
use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSApplication, NSApplicationActivationPolicy,
    NSSplitViewController, NSSplitViewItem, NSSystemColorsDidChangeNotification, NSWindow, NSWorkspace,
    NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
};
use objc2_foundation::{
    NSAppleEventDescriptor, NSAppleEventManager, NSNotification, NSNotificationCenter, NSObjectProtocol,
    NSOperationQueue, NSTimer,
};
use proto::{Request, Response, ResponseBody};
use serde_json::Value;

use crate::app::{self, App};
use crate::i18n::{Arg, tr, trf};
use crate::model::ItemId;
use crate::platform::{cancel_timer, schedule_block};
use crate::style::StyleError;
use crate::ui::actions::Actions;
use crate::ui::inspector::Inspector;
use crate::ui::sidebar::Sidebar;
use crate::ui::state::{Filter, Scope, Sort};
use crate::ui::theme::{Look, LookChange};
use crate::ui::toolbar::Toolbar;
use crate::ui::view::{Library, Thumbs};
use crate::ui::wall::WallView;
use crate::ui::widgets::ns;

pub use crate::ui::theme::invalidate;

const PROGRESS_TICK: f64 = 0.5;

type Subscription = (Retained<NSNotificationCenter>, Retained<ProtocolObject<dyn NSObjectProtocol>>);

struct Ui {
    library: Library,
    thumbs: Thumbs,
    filter: Filter,
    look: Look,
    selected: Option<usize>,
    actions: Retained<Actions>,
    wall: Retained<WallView>,
    inspector: Inspector,
    inspector_item: Retained<NSSplitViewItem>,
    sidebar: Sidebar,
    sidebar_item: Retained<NSSplitViewItem>,
    toolbar: Toolbar,
    settings: Option<settings::Settings>,
    values: Value,
    hotkeys: proto::Hotkeys,
    location: String,
    awaiting: bool,
    ticker: Option<Retained<NSTimer>>,
    window: Retained<NSWindow>,
    split: Retained<NSSplitViewController>,
    _observers: Vec<Subscription>,
}

impl Ui {
    fn reload(&mut self, mtm: MainThreadMarker) {
        let thumbs = &mut self.thumbs;
        let Some((library, values, hotkeys, appearance, styles)) = app::with(|app| {
            let settings = app.settings();
            return (
                view::build(app, thumbs),
                serde_json::to_value(settings).unwrap_or(Value::Null),
                settings.hotkeys.clone(),
                settings.appearance.clone(),
                app.styles().clone(),
            );
        }) else {
            return;
        };
        let selected = self.selected.and_then(|index| self.library.items.get(index)).map(|item| item.id);
        let folder = self.filter.folder.and_then(|index| self.library.folder_ids.get(index)).copied();
        let known: HashSet<ItemId> = self.library.items.iter().map(|item| item.id).collect();
        self.library = library;
        self.values = values;
        self.hotkeys = hotkeys;
        self.selected = selected.and_then(|id| self.library.position(id));
        self.filter.folder = folder
            .and_then(|id| self.library.folder_ids.iter().position(|other| *other == id))
            .filter(|index| !self.library.sealed(*index));
        self.filter.list = appearance.list;
        if let Scope::Space(space) = self.filter.scope
            && space >= self.library.desktops
        {
            self.filter.scope = Scope::Space(self.library.desktops.saturating_sub(1));
        }
        let fresh =
            if self.awaiting { self.library.items.iter().position(|item| !known.contains(&item.id)) } else { None };
        if let Some(index) = fresh {
            self.awaiting = false;
            if self.library.items[index].folder != self.filter.folder {
                self.filter.folder = None;
            }
            self.filter.query.clear();
            self.toolbar.search.setStringValue(&ns(""));
            self.selected = Some(index);
        }
        let change = self.look.update(&appearance, &styles);
        self.looked(mtm, change);
        self.refresh(mtm);
        if let Some(index) = fresh {
            self.wall.reveal(index);
        }
        self.fit_inspector();
        if let Some(settings) = &self.settings {
            settings.sync_look(&self.look);
            settings.sync(&self.values);
            settings.sync_shortcuts(&self.hotkeys);
        }
        self.tick();
    }

    fn tick(&mut self) {
        let busy = self.library.items.iter().any(|item| item.progress.is_some()) || !self.library.downloads.is_empty();
        match (busy, self.ticker.is_some()) {
            (true, false) => self.ticker = Some(schedule_block(PROGRESS_TICK, true, changed)),
            (false, true) => cancel_timer(self.ticker.take()),
            _ => {}
        }
    }

    fn refresh(&mut self, mtm: MainThreadMarker) {
        if self.selected.is_some_and(|index| index >= self.library.items.len()) {
            self.selected = None;
        }
        let visible = self.filter.visible(&self.library);
        let theme = &self.look.theme;
        self.wall.reload(&self.library, &visible, self.selected, self.filter.scope, self.filter.list, theme);
        self.toolbar.sync(mtm, &self.library, &self.filter, &self.look, &self.actions);
        self.sidebar.sync(&self.library, &self.filter);
        self.inspector.refresh(&self.library, &self.filter, theme, self.selected, &self.actions);
    }

    fn select(&mut self, index: Option<usize>) {
        if self.selected == index {
            return;
        }
        self.selected = index;
        self.wall.set_selected(index);
        later(Ui::settle);
    }

    fn settle(&mut self, _mtm: MainThreadMarker) {
        let index = self.selected;
        if index.is_some() || self.inspector_item.isCollapsed() {
            self.inspector.refresh(&self.library, &self.filter, &self.look.theme, index, &self.actions);
        } else {
            self.inspector.pause();
        }
        self.fit_inspector();
    }

    fn fit_inspector(&self) {
        let collapsed = self.selected.is_none();
        if self.inspector_item.isCollapsed() == collapsed {
            return;
        }
        animate_collapse(&self.inspector_item, collapsed);
    }

    fn show_sidebar(&self) {
        if self.sidebar_item.isCollapsed() {
            self.sidebar_item.setCollapsed(false);
            self.split.view().layoutSubtreeIfNeeded();
        }
    }

    fn settings(&mut self, mtm: MainThreadMarker) -> &settings::Settings {
        return self.settings.get_or_insert_with(|| {
            let settings = settings::Settings::new(mtm, &self.look, &self.location, &self.actions);
            settings.window.setDelegate(Some(ProtocolObject::from_ref(&*self.actions)));
            settings.sync_look(&self.look);
            settings.sync(&self.values);
            settings.sync_shortcuts(&self.hotkeys);
            return settings;
        });
    }

    fn hide(&self, mtm: MainThreadMarker) {
        self.end_sheets();
        self.window.close();
        show_in_dock(mtm, false);
    }

    fn end_sheets(&self) {
        while let Some(sheet) = self.window.attachedSheet() {
            self.window.endSheet(&sheet);
            if self.window.attachedSheet().is_some_and(|other| other == sheet) {
                sheet.orderOut(None);
                return;
            }
        }
    }

    fn show_settings(&mut self, mtm: MainThreadMarker, tab: Option<isize>) {
        self.open(mtm);
        let window = self.window.clone();
        self.settings(mtm).show(&window, tab);
    }

    fn looked(&self, mtm: MainThreadMarker, change: LookChange) {
        if change.mode {
            windows::set_appearance(mtm, self.look.mode);
            invalidate();
        }
        if change.repaint {
            if let Some(settings) = &self.settings {
                settings.repaint(&self.look.theme);
            }
            invalidate();
        }
    }

    fn restyle(&mut self, mtm: MainThreadMarker) {
        let Some((values, appearance, styles)) = app::with(|app| {
            let settings = app.settings();
            return (
                serde_json::to_value(settings).unwrap_or(Value::Null),
                settings.appearance.clone(),
                app.styles().clone(),
            );
        }) else {
            return;
        };
        self.values = values;
        let change = self.look.update(&appearance, &styles);
        self.looked(mtm, change);
        if change.styles
            && let Some(settings) = &self.settings
        {
            settings.sync_look(&self.look);
        }
    }

    fn recapture(&mut self, accessibility: bool) {
        let changed = self.look.rebuild();
        if accessibility {
            self.wall.accessibility_changed();
            metrics::refresh_surfaces();
        }
        if changed && let Some(settings) = &self.settings {
            settings.repaint(&self.look.theme);
            settings.sync_look(&self.look);
        }
        invalidate();
    }

    #[allow(deprecated)]
    fn open(&self, mtm: MainThreadMarker) {
        show_in_dock(mtm, true);
        if self.window.isMiniaturized() {
            self.window.deminiaturize(None);
        }
        self.window.makeKeyAndOrderFront(None);
        self.window.makeFirstResponder(Some(&self.wall));
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    }
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

static PENDING: AtomicBool = AtomicBool::new(false);
static RESTYLE: AtomicBool = AtomicBool::new(false);
static RECAPTURE: AtomicBool = AtomicBool::new(false);
static ACCESSIBILITY: AtomicBool = AtomicBool::new(false);

fn drain(flag: &'static AtomicBool, run: fn(&mut Ui, MainThreadMarker)) {
    if flag.swap(true, Ordering::SeqCst) {
        return;
    }
    DispatchQueue::main().exec_async(move || {
        if busy() {
            flag.store(false, Ordering::SeqCst);
            drain(flag, run);
            return;
        }
        flag.store(false, Ordering::SeqCst);
        update(run);
    });
}

fn animate_collapse(item: &Retained<NSSplitViewItem>, collapsed: bool) {
    let item = item.clone();
    let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
        let context = unsafe { context.as_ref() };
        context.setDuration(metrics::transform(metrics::MOTION_LAYOUT));
        context.setTimingFunction(Some(&metrics::ease_in_out()));
        context.setAllowsImplicitAnimation(true);
        item.animator().setCollapsed(collapsed);
    });
    NSAnimationContext::runAnimationGroup(&changes);
}

fn update(change: impl FnOnce(&mut Ui, MainThreadMarker)) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    UI.with(|ui| {
        if let Ok(mut ui) = ui.try_borrow_mut()
            && let Some(ui) = ui.as_mut()
        {
            change(ui, mtm);
        }
    });
}

fn commit(change: impl FnOnce(&mut Ui)) {
    update(|ui, _| change(ui));
    DispatchQueue::main().exec_async(|| update(Ui::refresh));
}

fn busy() -> bool {
    return UI.with(|ui| ui.try_borrow_mut().is_err());
}

fn later(change: impl FnOnce(&mut Ui, MainThreadMarker) + Send + 'static) {
    DispatchQueue::main().exec_async(|| update(change));
}

fn submit(work: impl FnOnce(&mut App) -> Option<Response> + Send + 'static) {
    DispatchQueue::main().exec_async(move || {
        if let Some(response) = app::with(work).flatten() {
            report(&response);
        }
    });
}

pub fn show_in_dock(mtm: MainThreadMarker, visible: bool) {
    let policy =
        if visible { NSApplicationActivationPolicy::Regular } else { NSApplicationActivationPolicy::Accessory };
    let application = NSApplication::sharedApplication(mtm);
    if application.activationPolicy() != policy {
        let _ = application.setActivationPolicy(policy);
    }
}

pub fn dismiss_sheets() {
    update(|ui, _| ui.end_sheets());
}

pub fn av1_notice(titles: &[String], converting: bool) {
    let visible = UI.with_borrow(|ui| ui.as_ref().is_some_and(|ui| ui.window.isVisible()));
    if !visible {
        return;
    }
    let names = titles.iter().map(|title| format!("“{title}”")).collect::<Vec<_>>().join(", ");
    let next = if converting {
        tr("LiveWall converts it to HEVC; it plays once the conversion finishes.")
    } else {
        tr("Use Optimize for This Mac to convert it to HEVC.")
    };
    let message = format!("{} {}", trf("This Mac’s chip can’t decode AV1 video: %@.", &[Arg::Text(names)]), next);
    DispatchQueue::main().exec_async(move || dialogs::notice(&message));
}

pub fn download_failed(reason: &str) {
    changed();
    let visible = UI.with_borrow(|ui| ui.as_ref().is_some_and(|ui| ui.window.isVisible()));
    if visible {
        dialogs::notice(&trf("Couldn’t download the video: %@", &[Arg::Text(reason.to_string())]));
    }
}

pub fn changed() {
    drain(&PENDING, Ui::reload);
}

fn selected_index() -> Option<usize> {
    return UI.with_borrow(|ui| ui.as_ref().and_then(|ui| ui.selected));
}

fn selected_id() -> Option<ItemId> {
    return UI.with_borrow(|ui| {
        let ui = ui.as_ref()?;
        return ui.selected.and_then(|index| ui.library.items.get(index)).map(|item| item.id);
    });
}

fn sync_inspector(ui: &Ui) {
    if let Some(item) = ui.selected.and_then(|index| ui.library.items.get(index)) {
        ui.inspector.sync(item);
    }
}

fn style_message(error: &StyleError) -> String {
    let (key, args) = error.message();
    let args: Vec<Arg> = args.into_iter().map(Arg::Text).collect();
    return trf(key, &args);
}

pub fn style(work: impl FnOnce(&mut App) -> Result<String, StyleError> + Send + 'static) {
    DispatchQueue::main().exec_async(move || {
        if let Some(Err(error)) = app::with(work) {
            dialogs::notice(&style_message(&error));
        }
    });
}

fn patch(value: Value) {
    let request = Request::SettingsSet { patch: value };
    submit(move |app| Some(app.dispatch(&request)));
}

fn send(request: &Request) {
    if let Some(response) = app::with(|app| app.dispatch(request)) {
        report(&response);
    }
}

fn report(response: &Response) {
    if let ResponseBody::Err(body) = &response.body {
        let message = body.message.clone();
        DispatchQueue::main().exec_async(move || dialogs::notice(&message));
    }
}

pub fn focus() {
    update(|ui, _| {
        ui.window.makeFirstResponder(Some(&ui.wall));
    });
}

pub fn select(index: Option<usize>) {
    update(|ui, _| ui.select(index));
}

pub fn reveal(index: usize) {
    update(|ui, _| {
        ui.select(Some(index));
        ui.wall.reveal(index);
    });
}

pub fn pick_style(index: usize) {
    later(move |ui, _| {
        if let Some(name) = ui.look.styles.get(index).map(|style| style.name.clone()) {
            style(move |app| app.style_apply(&name));
        }
    });
}

pub fn restyled() {
    drain(&RESTYLE, Ui::restyle);
}

fn recaptured(accessibility: bool) {
    if accessibility {
        ACCESSIBILITY.store(true, Ordering::SeqCst);
    }
    drain(&RECAPTURE, |ui, _| ui.recapture(ACCESSIBILITY.swap(false, Ordering::SeqCst)));
}

fn observe() -> Vec<Subscription> {
    let queue = NSOperationQueue::mainQueue();
    let colors = RcBlock::new(|_notification: NonNull<NSNotification>| recaptured(false));
    let accessibility = RcBlock::new(|_notification: NonNull<NSNotification>| recaptured(true));
    let observe = |center: Retained<NSNotificationCenter>, name, block: &DynBlock<dyn Fn(NonNull<NSNotification>)>| {
        let token = unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, Some(&queue), block) };
        return (center, token);
    };
    return vec![
        observe(NSNotificationCenter::defaultCenter(), unsafe { NSSystemColorsDidChangeNotification }, &colors),
        observe(
            NSWorkspace::sharedWorkspace().notificationCenter(),
            unsafe { NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification },
            &accessibility,
        ),
    ];
}

pub fn toggle_play() {
    update(|ui, _| {
        ui.inspector.toggle_play();
        sync_inspector(ui);
    });
}

pub fn frame(count: isize) {
    update(|ui, _| {
        ui.inspector.frame(count);
        sync_inspector(ui);
    });
}

pub fn rebind(action: proto::HotkeyAction, combo: Option<String>) -> Result<(), String> {
    let response = app::with(|app| app.set_hotkey(action, combo)).ok_or_else(|| tr("LiveWall is busy"))?;
    return match response.body {
        ResponseBody::Ok { .. } => Ok(()),
        ResponseBody::Err(body) => Err(body.message),
    };
}

pub fn recording(on: bool) {
    later(move |ui, _| {
        app::with(|app| app.record_hotkey(on));
        if !on && let Some(settings) = &ui.settings {
            settings.sync_shortcuts(&ui.hotkeys);
        }
    });
}

pub fn reply(request: &Request, response: &Response) {
    let (request, response) = (request.clone(), response.clone());
    DispatchQueue::main().exec_async(move || {
        if matches!(request, Request::LibraryAdd { .. }) {
            update(|ui, _| ui.awaiting = false);
        }
        let ResponseBody::Err(body) = &response.body else {
            return;
        };
        match &request {
            Request::Download { url, .. } if body.code == proto::ErrorCode::ConsentRequired => {
                dialogs::consent_sheet(url.clone(), &body.message);
            }
            _ => dialogs::notice(&body.message),
        }
    });
}

fn env(name: &str) -> Option<String> {
    return std::env::var(name).ok().filter(|value| !value.is_empty());
}

fn initial_filter(library: &Library) -> Filter {
    let scope =
        match env("LW_UI_SCOPE").and_then(|value| value.parse::<usize>().ok()).and_then(|number| number.checked_sub(1))
        {
            Some(space) if space < library.desktops => Scope::Space(space),
            _ => Scope::All,
        };
    let folder = env("LW_UI_FOLDER").and_then(|name| library.folders.iter().position(|folder| *folder == name));
    return Filter {
        scope,
        folder,
        sort: Sort::NameAscending,
        kind: None,
        query: env("LW_UI_SEARCH").unwrap_or_default(),
        list: false,
    };
}

fn launched_as_login_item() -> bool {
    let key_prop_data = u32::from_be_bytes(*b"prdt");
    let login_item = u32::from_be_bytes(*b"lgit");
    let manager = NSAppleEventManager::sharedAppleEventManager();
    let Some(event) = manager.currentAppleEvent() else {
        return false;
    };
    let descriptor: Option<Retained<NSAppleEventDescriptor>> =
        unsafe { objc2::msg_send![&*event, paramDescriptorForKeyword: key_prop_data] };
    return descriptor.is_some_and(|descriptor| descriptor.enumCodeValue() == login_item);
}

pub fn install(mtm: MainThreadMarker) -> Result<(), String> {
    windows::application_icon(mtm);
    let mut thumbs = Thumbs::default();
    let Some((library, values, hotkeys, appearance, styles, location)) = app::with(|app| {
        let settings = app.settings();
        let home = std::env::home_dir().unwrap_or_default();
        let location = app
            .paths()
            .library_file()
            .parent()
            .map(|dir| dir.to_string_lossy().into_owned())
            .map(|text| match text.strip_prefix(&*home.to_string_lossy()) {
                Some(rest) if !home.as_os_str().is_empty() => format!("~{rest}"),
                _ => text,
            })
            .unwrap_or_default();
        return (
            view::build(app, &mut thumbs),
            serde_json::to_value(settings).unwrap_or(Value::Null),
            settings.hotkeys.clone(),
            settings.appearance.clone(),
            app.styles().clone(),
            location,
        );
    }) else {
        return Err("LiveWall engine is not installed".to_string());
    };
    let look = Look::new(&appearance, &styles);
    windows::set_appearance(mtm, look.mode);
    let mut filter = initial_filter(&library);
    filter.list = appearance.list;

    let actions = Actions::new(mtm);
    NSApplication::sharedApplication(mtm).setMainMenu(Some(&menus::main_menu(mtm, &actions)));

    let wall = WallView::new(mtm, &look.theme);
    let inspector = Inspector::new(mtm, &look.theme);
    let sidebar = Sidebar::new(mtm, &look.theme, &actions);
    let windows::Panes { split, inspector: inspector_item, sidebar: sidebar_item } =
        windows::panes(mtm, &sidebar.view, &wall, &inspector);

    let (window, restored) = windows::main_window(mtm, &split);
    window.setDelegate(Some(ProtocolObject::from_ref(&*actions)));
    let toolbar = Toolbar::new(mtm, &actions, &look.theme);
    toolbar.search.setStringValue(&ns(&filter.query));
    window.setToolbar(Some(&toolbar.bar));
    if !restored {
        window.center();
    }

    let selected = env("LW_UI_SELECT").and_then(|title| library.items.iter().position(|item| item.title == title));
    inspector_item.setCollapsed(selected.is_none());

    let mut ui = Ui {
        library,
        thumbs,
        filter,
        look,
        selected,
        actions,
        wall,
        inspector,
        inspector_item,
        sidebar,
        sidebar_item,
        toolbar,
        settings: None,
        values,
        hotkeys,
        location,
        awaiting: false,
        ticker: None,
        window,
        split,
        _observers: observe(),
    };
    ui.refresh(mtm);
    ui.tick();
    UI.with_borrow_mut(|slot| *slot = Some(ui));
    return Ok(());
}

pub fn launched() {
    let login = launched_as_login_item();
    update(|ui, mtm| {
        let open = ui.values.get("open_library_at_launch").and_then(Value::as_bool).unwrap_or(true);
        let settings_tab = env("LW_UI_SETTINGS").and_then(|tab| tab.parse::<isize>().ok());
        if open && !login || settings_tab.is_some() {
            ui.open(mtm);
        }
        if let Some(index) = ui.selected {
            ui.wall.reveal(index);
        }
        if let Some(tab) = settings_tab {
            ui.show_settings(mtm, Some(tab));
        }
        if let Some(path) = env("LW_UI_WINDOW_FILE")
            && let Err(error) = std::fs::write(&path, ui.window.windowNumber().to_string())
        {
            tracing::warn!(%error, %path, "cannot write window number");
        }
    });
}

fn item_image(index: usize) -> Option<Retained<objc2_app_kit::NSImage>> {
    return UI.with(|ui| ui.try_borrow().ok()?.as_ref()?.library.items.get(index)?.image.clone());
}

fn rename_folder(folder: usize) {
    later(move |ui, _| {
        ui.show_sidebar();
        ui.sidebar.rename(folder);
    });
}

fn style_folder(folder: usize) {
    later(move |ui, mtm| {
        ui.open(mtm);
        ui.show_sidebar();
        picker::show(mtm, ui, folder);
    });
}

pub fn reopen() {
    later(|ui, mtm| ui.open(mtm));
}
