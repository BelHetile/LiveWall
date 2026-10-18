mod context;
mod dialogs;
mod folders;
mod grid;
mod inspector;
mod menus;
mod picker;
mod settings;
mod sidebar;
mod state;
mod toolbar;
mod view;
mod windows;

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSSplitViewItem, NSSplitViewController, NSWindow};
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::{NSSplitViewDidResizeSubviewsNotification, NSWindowDidChangeOcclusionStateNotification, NSWindowOcclusionState};
use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager, NSNotification, NSNotificationCenter, NSNotificationName, NSObjectProtocol, NSString};
use proto::{ErrorCode, Request, Response, ResponseBody};

use crate::app::{self, App};
use crate::i18n::{Arg, tr, trf};
use crate::model::ItemId;
use crate::style::StyleError;
use crate::ui::state::{Filter, Layout, Scope, Sort};
use crate::ui::view::{Library, Thumbs};

static PENDING: AtomicBool = AtomicBool::new(false);
static REFOCUS: AtomicBool = AtomicBool::new(false);

struct Ui {
    library: Library,
    thumbs: Thumbs,
    filter: Filter,
    sidebar: sidebar::Sidebar,
    grid: grid::Grid,
    inspector: inspector::Inspector,
    selected: Option<ItemId>,
    window: Retained<NSWindow>,
    _split: Retained<NSSplitViewController>,
    _sidebar: Retained<NSSplitViewItem>,
    _inspector: Retained<NSSplitViewItem>,
    toolbar: toolbar::Toolbar,
    _observers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
    _menus: menus::Menus,
}

impl Ui {
    fn reload(&mut self, mtm: MainThreadMarker) {
        let thumbs = &mut self.thumbs;
        let Some(library) = app::with(|app| view::build(app, thumbs)) else {
            return;
        };
        let folder = self.filter.folder.and_then(|index| self.library.folder_ids.get(index)).copied();
        self.library = library;
        if self.selected.is_some_and(|id| self.library.position(id).is_none()) {
            self.selected = None;
        }
        self.filter.folder = folder
            .and_then(|id| self.library.folder_ids.iter().position(|other| *other == id))
            .filter(|index| !self.library.sealed(*index));
        if let Scope::Space(space) = self.filter.scope
            && space >= self.library.desktops
        {
            self.filter.scope = Scope::Space(self.library.desktops.saturating_sub(1));
        }
        self.refresh(mtm);
        settings::sync();
    }

    fn sync_visibility(&mut self, _mtm: MainThreadMarker) {
        let visible = !self._inspector.isCollapsed()
            && self.window.isVisible()
            && self.window.occlusionState().contains(NSWindowOcclusionState::Visible);
        let selected = self.selected_index();
        self.inspector.set_visible(visible, &self.library, selected, self.filter.scope);
    }

    fn selected_index(&self) -> Option<usize> {
        return self.selected.and_then(|id| self.library.position(id));
    }

    fn refresh(&mut self, _mtm: MainThreadMarker) {
        self.toolbar.reload(self.library.desktops, self.filter.scope);
        self.sidebar.reload(&self.library, self.filter.folder);
        self.grid.reload(&self.library, &self.filter, self.selected);
        self.inspector.show(&self.library, self.selected_index(), self.filter.scope);
    }

    #[allow(deprecated)]
    fn open(&self, mtm: MainThreadMarker) {
        show_in_dock(mtm, true);
        if self.window.isMiniaturized() {
            self.window.deminiaturize(None);
        }
        if crate::dev::isolated() {
            self.window.orderFrontRegardless();
        } else {
            self.window.makeKeyAndOrderFront(None);
            NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        }
        self.focus();
    }

    fn closed(&mut self, mtm: MainThreadMarker) {
        self.sync_visibility(mtm);
        let shown = self.window.isVisible() || self.window.isMiniaturized() || settings::visible();
        show_in_dock(mtm, shown);
    }

    fn hide(&mut self, mtm: MainThreadMarker) {
        self.end_sheets();
        self.window.close();
        self.closed(mtm);
    }

    fn end_sheets(&self) {
        windows::end_sheets(&self.window);
        if let Some(window) = settings::window() {
            windows::end_sheets(&window);
        }
    }

    fn focus(&self) {
        let view = self.grid.responder(self.filter.layout);
        self.window.makeFirstResponder(Some(&view));
    }

    fn item_id(&self, index: Option<usize>) -> Option<ItemId> {
        return index.and_then(|index| self.library.items.get(index)).map(|item| item.id);
    }

    fn choose(&mut self, id: Option<ItemId>, reveal: bool) {
        if self.selected == id && !reveal {
            return;
        }
        self.selected = id;
        self.grid.select(id, reveal);
        self.inspector.show(&self.library, self.selected_index(), self.filter.scope);
    }
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

fn ns(value: &str) -> Retained<NSString> {
    return NSString::from_str(value);
}

fn env(name: &str) -> Option<String> {
    return std::env::var(name).ok().filter(|value| !value.is_empty());
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
    DispatchQueue::main().exec_async(move || update(change));
}

fn submit(work: impl FnOnce(&mut App) -> Option<Response> + Send + 'static) {
    DispatchQueue::main().exec_async(move || {
        if let Some(response) = app::with(work).flatten()
            && let ResponseBody::Err(body) = &response.body
        {
            notice(body.message.clone());
        }
    });
}

fn notice(message: String) {
    DispatchQueue::main().exec_async(move || dialogs::notice(&message));
}

pub fn show_in_dock(mtm: MainThreadMarker, visible: bool) {
    let policy =
        if visible { NSApplicationActivationPolicy::Regular } else { NSApplicationActivationPolicy::Accessory };
    let application = NSApplication::sharedApplication(mtm);
    if application.activationPolicy() != policy {
        let _ = application.setActivationPolicy(policy);
    }
}

fn initial_filter(library: &Library) -> Filter {
    let scope =
        match env("LW_UI_SCOPE").and_then(|value| value.parse::<usize>().ok()).and_then(|number| number.checked_sub(1))
        {
            Some(space) if space < library.desktops => Scope::Space(space),
            _ => Scope::All,
        };
    let folder = env("LW_UI_FOLDER")
        .and_then(|name| library.folders.iter().position(|folder| *folder == name))
        .filter(|index| !library.sealed(*index));
    return Filter {
        scope,
        folder,
        sort: Sort::NameAscending,
        kind: None,
        query: env("LW_UI_SEARCH").unwrap_or_default(),
        layout: match env("LW_UI_LAYOUT").as_deref() {
            Some("small") => Layout::Small,
            Some("list") => Layout::List,
            _ => Layout::Large,
        },
    };
}

fn watch_visibility(
    split: &NSSplitViewController,
    window: &NSWindow,
) -> Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>> {
    let center = NSNotificationCenter::defaultCenter();
    let block = RcBlock::new(|_: NonNull<NSNotification>| later(Ui::sync_visibility));
    let split_view = split.splitView();
    let watched: [(&NSNotificationName, &AnyObject); 2] = unsafe {
        [
            (NSSplitViewDidResizeSubviewsNotification, &split_view),
            (NSWindowDidChangeOcclusionStateNotification, window),
        ]
    };
    return watched
        .into_iter()
        .map(|(name, object)| unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(name), Some(object), None, &block)
        })
        .collect();
}

pub fn install(mtm: MainThreadMarker) -> Result<(), String> {
    windows::application_icon(mtm);
    let mut thumbs = Thumbs::default();
    let Some(library) = app::with(|app| view::build(app, &mut thumbs)) else {
        return Err("LiveWall engine is not installed".to_string());
    };
    let mode = app::with(|app| app.settings().appearance.mode).unwrap_or_default();
    windows::set_appearance(mtm, mode);
    let filter = initial_filter(&library);
    let sidebar = sidebar::Sidebar::new(mtm);
    sidebar.reload(&library, filter.folder);
    let mut inspector = inspector::Inspector::new(mtm);
    let selected = env("LW_UI_SELECT")
        .and_then(|title| library.items.iter().find(|item| item.title == title))
        .map(|item| item.id);
    inspector.show(&library, selected.and_then(|id| library.position(id)), filter.scope);
    let grid = grid::Grid::new(mtm);
    grid.set_layout(filter.layout);
    grid.reload(&library, &filter, selected);
    let panes = windows::panes(&sidebar.controller, &grid.controller, &inspector.controller);
    let (window, restored) = windows::main_window(mtm, &panes.split);
    let menus = menus::Menus::new(mtm);
    let toolbar = toolbar::Toolbar::new(mtm, menus.target());
    toolbar.reload(library.desktops, filter.scope);
    toolbar.show_layout(filter.layout);
    window.setToolbar(Some(&toolbar.bar));
    if !restored {
        window.center();
    }
    NSApplication::sharedApplication(mtm).setMainMenu(Some(&menus.bar));
    let observers = watch_visibility(&panes.split, &window);
    UI.with_borrow_mut(|slot| {
        *slot = Some(Ui {
            library,
            thumbs,
            filter,
            sidebar,
            grid,
            inspector,
            selected,
            window,
            _split: panes.split,
            _sidebar: panes.sidebar,
            _inspector: panes.inspector,
            toolbar,
            _observers: observers,
            _menus: menus,
        });
    });
    return Ok(());
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

pub fn launched() {
    let login = launched_as_login_item();
    update(|ui, mtm| {
        let open = app::with(|app| app.settings().open_library_at_launch).unwrap_or(true);
        let settings_tab = env("LW_UI_SETTINGS").and_then(|tab| tab.parse::<isize>().ok());
        if crate::dev::isolated() || settings_tab.is_some() || (open && !login) {
            ui.open(mtm);
        }
        later(|ui, mtm| {
            ui.sync_visibility(mtm);
            if ui.selected.is_some() {
                ui.grid.select(ui.selected, true);
            }
        });
        if let Some(tab) = settings_tab {
            ui._menus.show_settings(tab);
        }
        if let Some(path) = env("LW_UI_WINDOW_FILE")
            && let Err(error) = std::fs::write(&path, ui.window.windowNumber().to_string())
        {
            tracing::warn!(%error, %path, "cannot write window number");
        }
    });
}

pub fn auth_began() {
    let active = MainThreadMarker::new().is_some_and(|mtm| NSApplication::sharedApplication(mtm).isActive());
    REFOCUS.store(active, Ordering::Relaxed);
}

#[allow(deprecated)]
pub fn auth_ended() {
    later(|ui, mtm| {
        if REFOCUS.swap(false, Ordering::Relaxed) {
            NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
            ui.window.makeKeyAndOrderFront(None);
        }
    });
}

pub fn reopen() {
    later(|ui, mtm| ui.open(mtm));
}

fn set_scope(scope: Scope) {
    update(|ui, mtm| {
        if ui.filter.scope != scope {
            ui.filter.scope = scope;
            ui.refresh(mtm);
        }
    });
}

fn set_layout(layout: Layout) {
    update(|ui, _| {
        if ui.filter.layout != layout {
            ui.filter.layout = layout;
            ui.grid.set_layout(layout);
            ui.toolbar.show_layout(layout);
        }
    });
}

fn toggle_privacy() {
    submit(|app| {
        if app.is_unlocked() {
            app.lock();
        } else {
            app.request_unlock();
        }
        return None;
    });
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

pub fn restyled() {}

pub fn dismiss_sheets() {
    update(|ui, _| ui.end_sheets());
}

pub fn focus() {
    update(|ui, _| ui.focus());
}

pub fn select(index: Option<usize>) {
    update(|ui, _| ui.choose(ui.item_id(index), false));
}

pub fn reveal(index: usize) {
    update(|ui, _| ui.choose(ui.item_id(Some(index)), true));
}

pub fn toggle_play() {
    update(|ui, _| {
        if let Some(index) = ui.selected_index() {
            ui.inspector.toggle_play(&ui.library, index);
        }
    });
}

pub fn frame(count: isize) {
    update(|ui, _| {
        if let Some(index) = ui.selected_index() {
            ui.inspector.frame(&ui.library, index, count);
        }
    });
}

pub fn style(_work: impl FnOnce(&mut App) -> Result<String, StyleError> + Send + 'static) {}

pub fn av1_notice(titles: &[String], converting: bool) {
    let names = titles.iter().map(|title| format!("“{title}”")).collect::<Vec<_>>().join(", ");
    let next = if converting {
        tr("LiveWall converts it to HEVC; it plays once the conversion finishes.")
    } else {
        tr("Use Optimize to convert it to HEVC.")
    };
    notice(format!("{} {}", trf("This Mac’s chip can’t decode AV1 video: %@.", &[Arg::Text(names)]), next));
}

pub fn download_failed(reason: &str) {
    notice(trf("Couldn’t download the video: %@", &[Arg::Text(reason.to_string())]));
}

pub fn reply(request: &Request, response: &Response) {
    let ResponseBody::Err(body) = &response.body else {
        return;
    };
    match request {
        Request::Download { url, .. } if body.code == ErrorCode::ConsentRequired => {
            let (url, message) = (url.clone(), body.message.clone());
            DispatchQueue::main().exec_async(move || dialogs::consent_sheet(url, &message));
        }
        _ => notice(body.message.clone()),
    }
}

pub fn rebind(action: proto::HotkeyAction, combo: Option<String>) -> Result<(), String> {
    let response = crate::app::with(|app| app.set_hotkey(action, combo)).ok_or_else(|| tr("LiveWall is busy"))?;
    return match response.body {
        ResponseBody::Ok { .. } => Ok(()),
        ResponseBody::Err(body) => Err(body.message),
    };
}
