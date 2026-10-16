mod actions;
mod dialogs;
mod inspector;
mod menus;
mod palette;
mod settings;
mod state;
mod toolbar;
mod view;
mod wall;
mod widgets;
mod windows;

use std::cell::RefCell;
use std::collections::HashSet;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSApplication, NSSplitViewController, NSSplitViewItem, NSWindow};
use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager, NSTimer};
use proto::{Request, Response, ResponseBody};
use serde_json::{Value, json};

use crate::app::{self, App};
use crate::model::ItemId;
use crate::platform::{cancel_timer, schedule_block};
use crate::ui::actions::Actions;
use crate::ui::inspector::Inspector;
use crate::ui::palette::Look;
use crate::ui::state::{Filter, Scope, Sort};
use crate::ui::toolbar::Toolbar;
use crate::ui::view::{Library, Thumbs};
use crate::ui::wall::WallView;
use crate::ui::widgets::ns;

const PROGRESS_TICK: f64 = 0.5;

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
    toolbar: Toolbar,
    settings: Option<settings::Settings>,
    values: Value,
    hotkeys: proto::Hotkeys,
    location: String,
    awaiting: bool,
    ticker: Option<Retained<NSTimer>>,
    window: Retained<NSWindow>,
    split: Retained<NSSplitViewController>,
}

impl Ui {
    fn reload(&mut self, mtm: MainThreadMarker) {
        let thumbs = &mut self.thumbs;
        let Some((library, values, hotkeys, appearance)) = app::with(|app| {
            let settings = app.settings();
            return (
                view::build(app, thumbs),
                serde_json::to_value(settings).unwrap_or(Value::Null),
                settings.hotkeys.clone(),
                settings.appearance.clone(),
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
            self.filter.folder = None;
            self.filter.query.clear();
            self.toolbar.search.setStringValue(&ns(""));
            self.selected = Some(index);
        }
        let look = Look::new(&appearance);
        let restyle = self.look.differs(&look);
        if restyle {
            self.look = look;
            self.restyle(mtm);
        } else {
            self.refresh(mtm);
        }
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
        let theme = &self.look.colors;
        self.wall.reload(&self.library, &visible, self.selected, self.filter.scope, self.filter.list, theme);
        self.toolbar.sync(mtm, &self.library, &self.filter, &self.look, &self.actions);
        self.inspector.refresh(&self.library, &self.filter, theme, self.selected, &self.actions);
    }

    fn select(&mut self, index: Option<usize>) {
        if self.selected == index {
            return;
        }
        self.selected = index;
        self.wall.set_selected(index);
        if index.is_some() || self.inspector_item.isCollapsed() {
            self.inspector.refresh(&self.library, &self.filter, &self.look.colors, index, &self.actions);
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
        let item = self.inspector_item.clone();
        let view = self.split.view();
        let changes = RcBlock::new(move |context: NonNull<objc2_app_kit::NSAnimationContext>| {
            let context = unsafe { context.as_ref() };
            context.setDuration(0.34);
            context.setTimingFunction(Some(&objc2_quartz_core::CAMediaTimingFunction::functionWithName(unsafe {
                objc2_quartz_core::kCAMediaTimingFunctionEaseInEaseOut
            })));
            context.setAllowsImplicitAnimation(true);
            item.setCollapsed(collapsed);
            view.layoutSubtreeIfNeeded();
        });
        let done = RcBlock::new(|| {
            later(|ui, _| {
                if ui.selected.is_none() {
                    ui.inspector.refresh(&ui.library, &ui.filter, &ui.look.colors, None, &ui.actions);
                }
            });
        });
        objc2_app_kit::NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&done));
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

    fn restyle(&mut self, mtm: MainThreadMarker) {
        windows::set_appearance(mtm, self.look.mode);
        self.wall.paint(&self.look.colors);
        self.inspector.restyle(&self.look.colors);
        self.refresh(mtm);
    }

    #[allow(deprecated)]
    fn open(&self, mtm: MainThreadMarker) {
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

pub fn download_failed(reason: &str) {
    changed();
    let visible = UI.with_borrow(|ui| ui.as_ref().is_some_and(|ui| ui.window.isVisible()));
    if visible {
        dialogs::notice(&format!("Couldn’t download the video: {reason}"));
    }
}

pub fn changed() {
    if PENDING.swap(true, Ordering::SeqCst) {
        return;
    }
    DispatchQueue::main().exec_async(|| {
        if busy() {
            PENDING.store(false, Ordering::SeqCst);
            changed();
            return;
        }
        PENDING.store(false, Ordering::SeqCst);
        update(Ui::reload);
    });
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

pub fn pick_theme(index: usize) {
    later(move |ui, _| {
        if let Some((name, _)) = ui.look.themes.get(index) {
            patch(json!({ "appearance": { "theme": name } }));
        }
    });
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
    let response = app::with(|app| app.set_hotkey(action, combo)).ok_or_else(|| "LiveWall is busy".to_string())?;
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
    let Some((library, values, hotkeys, appearance, location)) = app::with(|app| {
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
            location,
        );
    }) else {
        return Err("LiveWall engine is not installed".to_string());
    };
    let look = Look::new(&appearance);
    windows::set_appearance(mtm, look.mode);
    let mut filter = initial_filter(&library);
    filter.list = appearance.list;

    let actions = Actions::new(mtm);
    NSApplication::sharedApplication(mtm).setMainMenu(Some(&menus::main_menu(mtm, &actions)));

    let wall = WallView::new(mtm, &look.colors);
    let inspector = Inspector::new(mtm, &look.colors);
    let windows::Panes { split, inspector: inspector_item } = windows::panes(mtm, &wall, &inspector);

    let window = windows::main_window(mtm, &split);
    let toolbar = Toolbar::new(mtm, &actions);
    toolbar.search.setStringValue(&ns(&filter.query));
    window.setToolbar(Some(&toolbar.bar));
    window.center();

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
        toolbar,
        settings: None,
        values,
        hotkeys,
        location,
        awaiting: false,
        ticker: None,
        window,
        split,
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
            ui.settings(mtm).show(Some(tab));
        }
        if let Some(path) = env("LW_UI_WINDOW_FILE") {
            let shown = ui.settings.as_ref().map_or(&ui.window, |settings| &settings.window);
            if let Err(error) = std::fs::write(&path, shown.windowNumber().to_string()) {
                tracing::warn!(%error, %path, "cannot write window number");
            }
        }
    });
}

pub fn reopen() {
    update(|ui, mtm| ui.open(mtm));
}
