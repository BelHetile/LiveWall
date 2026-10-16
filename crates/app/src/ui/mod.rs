mod inspector;
mod palette;
mod settings;
mod state;
mod toolbar;
mod view;
mod wall;
mod widgets;

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSAppearance, NSAppearanceNameAqua, NSApplication,
    NSBackingStoreType, NSBeep, NSControlStateValueOn, NSEventModifierFlags, NSEventType, NSImage, NSMenu, NSMenuItem,
    NSModalResponse, NSModalResponseOK, NSOpenPanel, NSPopUpButton, NSSearchField, NSSegmentedControl, NSSlider,
    NSSplitViewController, NSSplitViewItem, NSSwitch, NSTextField, NSTitlebarSeparatorStyle, NSViewController,
    NSWindow, NSWindowDelegate, NSWindowStyleMask, NSWindowTitleVisibility, NSWindowToolbarStyle,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{
    NSAppleEventDescriptor, NSAppleEventManager, NSData, NSNotification, NSObject, NSObjectProtocol, NSString, NSTimer,
};
use proto::{ApplyTarget, Geometry, HotkeyAction, Request, Response, ResponseBody};
use serde_json::{Value, json};

use crate::app::{self, Event};
use crate::model::ItemId;
use crate::ui::inspector::Inspector;
use crate::ui::palette::Look;
use crate::ui::settings::{Change, Input};
use crate::ui::state::{Filter, KINDS, Scope, Sort};
use crate::ui::toolbar::Toolbar;
use crate::ui::view::{Library, Thumbs};
use crate::ui::wall::WallView;
use crate::ui::widgets::NumberField;

const VOLUME_ON: f64 = 0.6;
const PROGRESS_TICK: f64 = 0.5;
const ICON: &[u8] = include_bytes!("../../../../resources/AppIcon.iconset/icon_512x512@2x.png");

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
        self.filter.folder = folder.and_then(|id| self.library.folder_ids.iter().position(|other| *other == id));
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
            self.toolbar.search.setStringValue(&NSString::new());
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
            (true, false) => {
                let block = RcBlock::new(|_timer: NonNull<NSTimer>| changed());
                self.ticker =
                    Some(unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(PROGRESS_TICK, true, &block) });
            }
            (false, true) => {
                if let Some(timer) = self.ticker.take() {
                    timer.invalidate();
                }
            }
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
        let application = NSApplication::sharedApplication(mtm);
        let appearance = match self.look.mode {
            1 => NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }),
            2 => widgets::dark(),
            _ => None,
        };
        application.setAppearance(appearance.as_deref());
        self.wall.paint(&self.look.colors);
        self.inspector.restyle(&self.look.colors);
        self.refresh(mtm);
    }

    fn open(&self, mtm: MainThreadMarker) {
        if self.window.isMiniaturized() {
            self.window.deminiaturize(None);
        }
        self.window.makeKeyAndOrderFront(None);
        self.window.makeFirstResponder(Some(&self.wall));
        #[allow(deprecated)]
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

fn selected_item(ui: &mut Ui) -> Option<&mut view::Item> {
    return ui.selected.and_then(|index| ui.library.items.get_mut(index));
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

fn tag_index(sender: &NSMenuItem) -> usize {
    return usize::try_from(sender.tag()).unwrap_or(0);
}

fn dragging() -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let event = NSApplication::sharedApplication(mtm).currentEvent();
    return event
        .is_some_and(|event| matches!(event.r#type(), NSEventType::LeftMouseDown | NSEventType::LeftMouseDragged));
}

fn send(request: &Request) {
    if let Some(response) = app::with(|app| app.dispatch(request)) {
        report(&response);
    }
}

fn report(response: &Response) {
    if let ResponseBody::Err(body) = &response.body {
        let message = body.message.clone();
        DispatchQueue::main().exec_async(move || notice(&message));
    }
}

fn configure(tag: isize, input: &Input) {
    let request = match settings::change(tag, input) {
        Some(Change::Patch(patch)) => Request::SettingsSet { patch },
        Some(Change::Login(enabled)) => Request::LoginItemSet { enabled },
        None => return,
    };
    DispatchQueue::main().exec_async(move || send(&request));
}

fn patch(value: Value) {
    DispatchQueue::main().exec_async(move || send(&Request::SettingsSet { patch: value }));
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct Actions;

    unsafe impl NSObjectProtocol for Actions {}

    impl Actions {
        #[unsafe(method(scope:))]
        fn scope(&self, sender: &NSMenuItem) {
            let scope = Scope::from_tag(sender.tag());
            commit(|ui| ui.filter.scope = scope);
        }

        #[unsafe(method(folder:))]
        fn folder(&self, sender: &NSMenuItem) {
            let tag = sender.tag();
            if tag == -2 {
                new_folder(None);
                return;
            }
            commit(|ui| ui.filter.folder = usize::try_from(tag).ok());
        }

        #[unsafe(method(sort:))]
        fn sort(&self, sender: &NSMenuItem) {
            let sort = Sort::ALL.get(tag_index(sender)).map_or(Sort::NameAscending, |(sort, _)| *sort);
            commit(|ui| ui.filter.sort = sort);
        }

        #[unsafe(method(kind:))]
        fn kind(&self, sender: &NSMenuItem) {
            let kind = KINDS.get(tag_index(sender)).and_then(|(kind, _)| *kind);
            commit(|ui| ui.filter.kind = kind);
        }

        #[unsafe(method(layout:))]
        fn layout(&self, _sender: Option<&AnyObject>) {
            let list = UI.with_borrow(|ui| ui.as_ref().is_some_and(|ui| ui.toolbar.layout.selectedIndex() == 1));
            patch(json!({ "appearance": { "list": list } }));
        }

        #[unsafe(method(showGrid:))]
        fn show_grid(&self, _sender: Option<&AnyObject>) {
            patch(json!({ "appearance": { "list": false } }));
        }

        #[unsafe(method(showList:))]
        fn show_list(&self, _sender: Option<&AnyObject>) {
            patch(json!({ "appearance": { "list": true } }));
        }

        #[unsafe(method(find:))]
        fn find(&self, _sender: Option<&AnyObject>) {
            update(|ui, _| {
                ui.window.makeFirstResponder(Some(&ui.toolbar.search));
            });
        }

        #[unsafe(method(deselect:))]
        fn deselect(&self, _sender: Option<&AnyObject>) {
            update(|ui, _| ui.select(None));
        }

        #[unsafe(method(search:))]
        fn search(&self, sender: &NSSearchField) {
            let query = sender.stringValue().to_string();
            commit(|ui| ui.filter.query = query);
        }

        #[unsafe(method(showSettings:))]
        fn show_settings(&self, _sender: Option<&AnyObject>) {
            update(|ui, mtm| ui.settings(mtm).show(None));
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            app::post(Event::Quit);
        }

        #[unsafe(method(settingSlide:))]
        fn setting_slide(&self, sender: &NSSlider) {
            let (tag, value, live) = (sender.tag(), sender.doubleValue(), dragging());
            later(move |ui, mtm| {
                ui.settings(mtm).slid(tag, value);
                if !live {
                    configure(tag, &Input::Number(value.round()));
                }
            });
        }

        #[unsafe(method(settingTyped:))]
        fn setting_typed(&self, sender: &NumberField) {
            let tag = sender.tag();
            if let Some(value) = sender.value().filter(|_| !busy()) {
                later(move |ui, mtm| {
                    let value = ui.settings(mtm).typed(tag, value);
                    configure(tag, &Input::Number(value));
                });
            }
        }

        #[unsafe(method(restoreShortcuts:))]
        fn restore_shortcuts(&self, _sender: Option<&AnyObject>) {
            DispatchQueue::main().exec_async(|| {
                app::with(app::App::restore_hotkeys);
            });
        }

        #[unsafe(method(mode:))]
        fn mode(&self, sender: &NSSegmentedControl) {
            let mode = ["system", "light", "dark"].get(usize::try_from(sender.selectedSegment()).unwrap_or(0));
            patch(json!({ "appearance": { "mode": mode } }));
        }

        #[unsafe(method(toggle:))]
        fn toggle(&self, sender: &NSSwitch) {
            configure(sender.tag(), &Input::Flag(sender.state() == NSControlStateValueOn));
        }

        #[unsafe(method(choose:))]
        fn choose(&self, sender: &NSPopUpButton) {
            let index = usize::try_from(sender.indexOfSelectedItem()).unwrap_or(0);
            configure(sender.tag(), &Input::Index(index));
        }

        #[unsafe(method(customColor:))]
        fn custom_color(&self, _sender: Option<&AnyObject>) {
            later(|ui, _| {
                let Some((accent, canvas)) = ui.settings.as_ref().and_then(settings::Settings::custom_colors) else {
                    return;
                };
                patch(json!({ "appearance": { "theme": "Custom", "accent": accent.to_hex(), "canvas": canvas.to_hex() } }));
            });
        }

        #[unsafe(method(setWallpaper:))]
        fn set_wallpaper(&self, _sender: Option<&AnyObject>) {
            apply_selection();
        }

        #[unsafe(method(applyTo:))]
        fn apply_to(&self, sender: &NSMenuItem) {
            let item = UI.with_borrow(|ui| ui.as_ref().and_then(|ui| ui.selected));
            place(item, Scope::from_tag(sender.tag()));
        }

        #[unsafe(method(reveal:))]
        fn reveal_item(&self, sender: &NSMenuItem) {
            if let Ok(index) = usize::try_from(sender.tag()) {
                reveal(index);
            }
        }

        #[unsafe(method(resetSettings:))]
        fn reset_settings(&self, _sender: Option<&AnyObject>) {
            if let Some(id) = selected_id() {
                DispatchQueue::main().exec_async(move || {
                    if let Some(response) = app::with(|app| app.reset_item(id)) {
                        report(&response);
                    }
                });
            }
        }

        #[unsafe(method(optimize:))]
        fn optimize(&self, _sender: Option<&AnyObject>) {
            if let Some(id) = selected_id() {
                let request = Request::Optimize { id: id.to_string() };
                DispatchQueue::main().exec_async(move || send(&request));
            }
        }

        #[unsafe(method(typed:))]
        fn typed(&self, sender: &NumberField) {
            if busy() {
                return;
            }
            match sender.value() {
                Some(value) => adjust(sender.tag(), value, false),
                None => later(|ui, _| sync_inspector(ui)),
            }
        }

        #[unsafe(method(target:))]
        fn target(&self, sender: &NSMenuItem) {
            let tag = sender.tag();
            commit(|ui| {
                ui.inspector.target = (tag != inspector::RESET).then(|| Scope::from_tag(tag));
            });
        }

        #[unsafe(method(setLock:))]
        fn set_lock(&self, _sender: Option<&AnyObject>) {
            lock(false);
        }

        #[unsafe(method(lockFrame:))]
        fn lock_frame(&self, _sender: Option<&AnyObject>) {
            lock(true);
        }

        #[unsafe(method(delete:))]
        fn delete(&self, _sender: Option<&AnyObject>) {
            confirm_delete();
        }

        #[unsafe(method(moveFolder:))]
        fn move_folder(&self, sender: &NSPopUpButton) {
            let folder = usize::try_from(sender.indexOfSelectedItem()).ok().and_then(|index| index.checked_sub(1));
            move_selected(folder);
        }

        #[unsafe(method(moveTo:))]
        fn move_to(&self, sender: &NSMenuItem) {
            let tag = sender.tag();
            if tag == -2 {
                new_folder(UI.with_borrow(|ui| ui.as_ref().and_then(|ui| ui.selected)));
                return;
            }
            move_selected(usize::try_from(tag).ok());
        }

        #[unsafe(method(addFiles:))]
        fn add_files(&self, sender: &NSMenuItem) {
            add_files(sender.tag() == 1);
        }

        #[unsafe(method(showAdd:))]
        fn show_add(&self, _sender: Option<&AnyObject>) {
            let parts = UI.with_borrow(|ui| ui.as_ref().map(|ui| ui.toolbar.add_parts()));
            if let Some((button, menu)) = parts {
                toolbar::drop_down(&button, &menu);
            }
        }

        #[unsafe(method(download:))]
        fn download(&self, _sender: Option<&AnyObject>) {
            download_sheet();
        }

        #[unsafe(method(run:))]
        fn run_command(&self, sender: &NSMenuItem) {
            if let Some(action) = HotkeyAction::ALL.get(tag_index(sender)) {
                let action = *action;
                DispatchQueue::main().exec_async(move || {
                    app::with(|app| app.run_hotkey(action));
                });
            }
        }

        #[unsafe(method(play:))]
        fn play(&self, _sender: Option<&AnyObject>) {
            toggle_play();
        }

        #[unsafe(method(mute:))]
        fn mute(&self, _sender: Option<&AnyObject>) {
            update(|ui, _| {
                let Some(item) = selected_item(ui) else {
                    return;
                };
                let settings = &mut item.settings;
                settings.muted = !settings.muted;
                if !settings.muted && settings.volume <= 0.0 {
                    settings.volume = VOLUME_ON;
                }
                commit_item(item.id, Some(settings.volume), None, Some(settings.muted));
                sync_inspector(ui);
            });
        }

        #[unsafe(method(fill:))]
        fn fill(&self, sender: &NSSegmentedControl) {
            let mode = usize::try_from(sender.selectedSegment()).ok().and_then(|index| inspector::FILLS.get(index));
            let Some(mode) = mode.copied() else {
                return;
            };
            update(|ui, _| {
                let Some(item) = selected_item(ui) else {
                    return;
                };
                let geometry = item.modes.get(&mode).copied().unwrap_or_default();
                item.settings.mode = mode;
                (item.settings.x, item.settings.y, item.settings.zoom) = (geometry.x, geometry.y, geometry.zoom);
                send(&Request::SetGeometry { item: item.id.to_string(), mode, geometry });
                sync_inspector(ui);
            });
        }

        #[unsafe(method(slide:))]
        fn slide(&self, sender: &NSSlider) {
            let (tag, value, live) = (sender.tag(), sender.doubleValue(), dragging());
            adjust(tag, if tag == inspector::SPEED { (value * 20.0).round() / 20.0 } else { value }, live);
        }
    }

    unsafe impl NSWindowDelegate for Actions {
        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _notification: &NSNotification) {
            later(|ui, _| {
                if let Some(settings) = &ui.settings {
                    settings.window.makeFirstResponder(None);
                }
            });
        }
    }
);

#[allow(clippy::cast_possible_truncation)]
fn commit_item(id: ItemId, volume: Option<f64>, speed: Option<f64>, muted: Option<bool>) {
    send(&Request::SetItem {
        item: id.to_string(),
        volume: volume.map(|value| value as f32),
        speed: speed.map(|value| value as f32),
        muted,
        still_time: None,
    });
}

fn adjust(tag: isize, value: f64, live: bool) {
    later(move |ui, _| {
        if tag == inspector::SCRUB {
            ui.inspector.seek(value.max(0.0));
            return;
        }
        let Some(item) = selected_item(ui) else {
            return;
        };
        let value = inspector::stored(item, tag, value);
        let id = item.id;
        let mode = item.settings.mode;
        let settings = &mut item.settings;
        match tag {
            inspector::VOLUME => {
                settings.volume = value;
                settings.muted = value <= 0.0;
            }
            inspector::SPEED => settings.speed = value,
            inspector::HORIZONTAL => settings.x = value,
            inspector::VERTICAL => settings.y = value,
            inspector::ZOOM => settings.zoom = value,
            _ => {}
        }
        let geometry = Geometry { x: settings.x, y: settings.y, zoom: settings.zoom };
        let (volume, speed, muted) = (settings.volume, settings.speed, settings.muted);
        if !live {
            match tag {
                inspector::VOLUME => commit_item(id, Some(volume), None, Some(muted)),
                inspector::SPEED => commit_item(id, None, Some(speed), None),
                _ => send(&Request::SetGeometry { item: id.to_string(), mode, geometry }),
            }
        }
        sync_inspector(ui);
    });
}

pub fn focus() {
    update(|ui, _| {
        ui.window.makeFirstResponder(Some(&ui.wall));
    });
}

fn assign(ui: &Ui, scope: Scope, index: usize) {
    let Some(item) = ui.library.items.get(index) else {
        return;
    };
    let id = item.id.to_string();
    let response = match scope {
        Scope::All => app::with(|app| app.apply(Some(&id), &ApplyTarget::Default, true, None)),
        Scope::Space(space) => {
            let Some(space) = ui.library.spaces.get(space).cloned() else {
                return;
            };
            app::with(|app| app.apply(Some(&id), &ApplyTarget::Space { space }, false, None))
        }
    };
    if let Some(response) = response {
        report(&response);
    }
}

fn lock(frame: bool) {
    update(|ui, _| {
        let Some(item) = ui.selected.and_then(|index| ui.library.items.get(index)) else {
            return;
        };
        let id = item.id;
        #[allow(clippy::cast_possible_truncation)]
        let time = frame.then(|| ui.inspector.time()).flatten().map(|time| time as f32);
        if let Some(response) = app::with(|app| app.set_lock(Some((id, time)))) {
            report(&response);
        }
    });
}

pub fn apply_selection() {
    update(|ui, _| {
        if let Some(index) = ui.selected {
            assign(ui, ui.inspector.target.unwrap_or(ui.filter.scope), index);
        }
    });
}

pub fn place(item: Option<usize>, scope: Scope) {
    let valid = UI.with_borrow(|ui| {
        let Some(ui) = ui.as_ref() else {
            return false;
        };
        return match scope {
            Scope::All => true,
            Scope::Space(space) => space < ui.library.desktops,
        };
    });
    let Some(item) = item.filter(|_| valid) else {
        NSBeep();
        return;
    };
    update(|ui, _| {
        ui.filter.scope = scope;
        assign(ui, scope, item);
    });
}

pub fn rebind(action: HotkeyAction, combo: Option<String>) -> Result<(), String> {
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

impl Actions {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        return unsafe { msg_send![super(this), init] };
    }
}

pub fn select(index: Option<usize>) {
    update(|ui, _| ui.select(index));
}

pub fn apply(index: usize) {
    update(|ui, _| assign(ui, ui.filter.scope, index));
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
                consent_sheet(url.clone(), &body.message);
            }
            _ => notice(&body.message),
        }
    });
}

pub fn context_menu(index: usize) -> Option<Retained<NSMenu>> {
    let mtm = MainThreadMarker::new()?;
    return UI.with_borrow(|ui| {
        let ui = ui.as_ref()?;
        let target: &AnyObject = &ui.actions;
        let item = |title: &str, action: Sel, tag: isize| {
            let entry = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(title),
                    Some(action),
                    &NSString::new(),
                )
            };
            unsafe { entry.setTarget(Some(target)) };
            entry.setTag(tag);
            return entry;
        };
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        let destinations = toolbar::scope_menu(&ui.library, ui.filter.scope, "", sel!(applyTo:), target);
        destinations.removeItemAtIndex(0);
        let wallpaper = NSMenuItem::new(mtm);
        wallpaper.setTitle(&NSString::from_str("Set as Wallpaper"));
        wallpaper.setSubmenu(Some(&destinations));
        wallpaper.setEnabled(ui.library.items.get(index).is_some_and(|entry| entry.playable));
        menu.addItem(&wallpaper);
        menu.addItem(&item("Set as Lock Screen", sel!(setLock:), 0));
        if let Some(entry) = ui.library.items.get(index).filter(|entry| entry.optimizable) {
            let optimize = item("Optimize…", sel!(optimize:), 0);
            optimize.setEnabled(entry.progress.is_none());
            menu.addItem(&optimize);
        }
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        let folders = NSMenu::new(mtm);
        let current = ui.library.items.get(index).and_then(|item| item.folder);
        let unfiled = item("Unfiled", sel!(moveTo:), -1);
        unfiled.setState(isize::from(current.is_none()));
        folders.addItem(&unfiled);
        folders.addItem(&NSMenuItem::separatorItem(mtm));
        for (position, name) in ui.library.folders.iter().enumerate() {
            let entry = item(name, sel!(moveTo:), isize::try_from(position).unwrap_or(-1));
            entry.setState(isize::from(current == Some(position)));
            folders.addItem(&entry);
        }
        folders.addItem(&NSMenuItem::separatorItem(mtm));
        folders.addItem(&item("New Folder…", sel!(moveTo:), -2));
        let move_to = NSMenuItem::new(mtm);
        move_to.setTitle(&NSString::from_str("Move to"));
        move_to.setSubmenu(Some(&folders));
        menu.addItem(&move_to);
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&item("Remove from Library…", sel!(delete:), 0));
        return Some(menu);
    });
}

fn sheet(alert: &NSAlert, done: impl Fn(NSModalResponse) + 'static) {
    let Some(window) = UI.with_borrow(|ui| ui.as_ref().map(|ui| ui.window.clone())).filter(|window| window.isVisible())
    else {
        done(alert.runModal());
        return;
    };
    let block = RcBlock::new(done);
    alert.beginSheetModalForWindow_completionHandler(&window, Some(&block));
}

fn notice(message: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.setMessageText(&NSString::from_str(message));
    alert.addButtonWithTitle(&NSString::from_str("OK"));
    sheet(&alert, |_| {});
}

fn consent_sheet(url: String, message: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str("Install download tools?"));
    alert.setInformativeText(&NSString::from_str(&format!(
        "{message}. LiveWall can download yt-dlp, ffmpeg and deno into its own folder."
    )));
    alert.addButtonWithTitle(&NSString::from_str("Install and Download"));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    sheet(&alert, move |response| {
        if response == NSAlertFirstButtonReturn {
            let request = Request::Download { url: url.clone(), consent: true };
            DispatchQueue::main().exec_async(move || {
                app::with(|app| app.submit(request));
            });
        }
    });
}

fn confirm_delete() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(title) = UI.with_borrow(|ui| {
        let ui = ui.as_ref()?;
        return ui.selected.and_then(|index| ui.library.items.get(index)).map(|item| item.title.clone());
    }) else {
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.setMessageText(&NSString::from_str(&format!("Remove “{title}” from the library?")));
    alert.setInformativeText(&NSString::from_str("The original file stays where it is."));
    let remove = alert.addButtonWithTitle(&NSString::from_str("Remove"));
    remove.setHasDestructiveAction(true);
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    sheet(&alert, |response| {
        if response == NSAlertFirstButtonReturn
            && let Some(id) = selected_id()
        {
            DispatchQueue::main().exec_async(move || send(&Request::LibraryRemove { ids: vec![id.to_string()] }));
        }
    });
}

fn move_selected(folder: Option<usize>) {
    update(|ui, _| {
        let Some(item) = ui.selected.and_then(|index| ui.library.items.get(index)) else {
            return;
        };
        let folder = folder.and_then(|index| ui.library.folder_ids.get(index)).map(ToString::to_string);
        send(&Request::LibraryMove { ids: vec![item.id.to_string()], folder });
    });
}

fn new_folder(item: Option<usize>) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str("New Folder"));
    alert.setInformativeText(&NSString::from_str("Folders only organise the library; files are not moved."));
    let field =
        NSTextField::initWithFrame(NSTextField::alloc(mtm), CGRect::new(CGPoint::ZERO, CGSize::new(260.0, 24.0)));
    field.setPlaceholderString(Some(&NSString::from_str("Folder name")));
    alert.setAccessoryView(Some(&field));
    alert.addButtonWithTitle(&NSString::from_str("Create"));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    alert.window().setInitialFirstResponder(Some(&field));
    sheet(&alert, move |response| {
        let name = field.stringValue().to_string().trim().to_string();
        if response != NSAlertFirstButtonReturn || name.is_empty() {
            return;
        }
        later(move |ui, mtm| {
            let id = item.and_then(|index| ui.library.items.get(index)).map(|entry| entry.id);
            let Some(folder) = app::with(|app| app.create_folder(name, id)) else {
                return;
            };
            ui.reload(mtm);
            if id.is_none() {
                ui.filter.folder = ui.library.folder_ids.iter().position(|other| *other == folder);
                ui.refresh(mtm);
            }
        });
    });
}

fn media_in(path: &Path) -> Vec<String> {
    if !path.is_dir() {
        return vec![path.to_string_lossy().into_owned()];
    }
    let mut files: Vec<String> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|file| file.extension().and_then(|value| value.to_str()).and_then(crate::import::kind_for).is_some())
        .map(|file| file.to_string_lossy().into_owned())
        .collect();
    files.sort();
    return files;
}

fn add_files(directories: bool) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(window) = UI.with_borrow(|ui| ui.as_ref().map(|ui| ui.window.clone())) else {
        return;
    };
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(!directories);
    panel.setCanChooseDirectories(directories);
    panel.setAllowsMultipleSelection(true);
    panel.setPrompt(Some(&NSString::from_str("Add")));
    let chosen = panel.clone();
    let block = RcBlock::new(move |response: NSModalResponse| {
        if response != NSModalResponseOK {
            return;
        }
        let urls = chosen.URLs();
        let paths: Vec<String> = (0..urls.count())
            .filter_map(|position| urls.objectAtIndex(position).path())
            .flat_map(|path| media_in(Path::new(&path.to_string())))
            .collect();
        if paths.is_empty() {
            return;
        }
        later(move |ui, _| {
            ui.awaiting = true;
            app::with(|app| app.submit(Request::LibraryAdd { paths }));
        });
    });
    panel.beginSheetModalForWindow_completionHandler(&window, &block);
}

fn download_sheet() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str("Add from Video Link"));
    alert.setInformativeText(&NSString::from_str(
        "Paste a link from YouTube, Vimeo or any site yt-dlp supports. The video is downloaded into your library.",
    ));
    let field =
        NSTextField::initWithFrame(NSTextField::alloc(mtm), CGRect::new(CGPoint::ZERO, CGSize::new(340.0, 24.0)));
    field.setPlaceholderString(Some(&NSString::from_str("https://youtu.be/…")));
    field.widthAnchor().constraintEqualToConstant(340.0).setActive(true);
    let quality = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), CGRect::ZERO, false);
    let titles: Vec<Retained<NSString>> =
        ["Best Available", "4K", "1440p", "1080p", "720p"].iter().map(|title| NSString::from_str(title)).collect();
    quality.addItemsWithTitles(&objc2_foundation::NSArray::from_retained_slice(&titles));
    quality.selectItemAtIndex(3);
    quality.setEnabled(false);
    let label = NSTextField::labelWithString(&NSString::from_str("Quality"), mtm);
    let options = widgets::stack(mtm, &[&label, &quality], false, 8.0);
    let accessory = widgets::stack(mtm, &[&field, &options], true, 10.0);
    accessory.setAlignment(objc2_app_kit::NSLayoutAttribute::Leading);
    accessory.setFrameSize(accessory.fittingSize());
    alert.setAccessoryView(Some(&accessory));
    alert.addButtonWithTitle(&NSString::from_str("Download"));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    alert.window().setInitialFirstResponder(Some(&field));
    sheet(&alert, move |response| {
        let url = field.stringValue().to_string().trim().to_string();
        if response != NSAlertFirstButtonReturn || url.is_empty() {
            return;
        }
        DispatchQueue::main().exec_async(move || {
            app::with(|app| app.submit(Request::Download { url, consent: false }));
        });
    });
}

fn env(name: &str) -> Option<String> {
    return std::env::var(name).ok().filter(|value| !value.is_empty());
}

fn initial_filter(library: &Library) -> Filter {
    let scope = match env("LW_UI_SCOPE").as_deref() {
        Some("all") => Scope::All,
        Some(value) => match value.parse::<usize>().ok().and_then(|number| number.checked_sub(1)) {
            Some(space) if space < library.desktops => Scope::Space(space),
            _ => Scope::Space(library.current),
        },
        None => Scope::Space(library.current),
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
        unsafe { msg_send![&*event, paramDescriptorForKeyword: key_prop_data] };
    return descriptor.is_some_and(|descriptor| descriptor.enumCodeValue() == login_item);
}

fn application_icon() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let bundle = objc2_foundation::NSBundle::mainBundle().bundlePath().to_string();
    if Path::new(&bundle).extension().is_some_and(|extension| extension.eq_ignore_ascii_case("app")) {
        return;
    }
    let data = NSData::with_bytes(ICON);
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
        unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
    }
}

pub fn install(mtm: MainThreadMarker) -> Result<(), String> {
    let application = NSApplication::sharedApplication(mtm);
    application_icon();
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
    match look.mode {
        1 => application.setAppearance(NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).as_deref()),
        2 => application.setAppearance(widgets::dark().as_deref()),
        _ => {}
    }
    let mut filter = initial_filter(&library);
    filter.list = appearance.list;

    let actions = Actions::new(mtm);
    application.setMainMenu(Some(&main_menu(mtm, &actions)));

    let wall = WallView::new(mtm, &look.colors);
    let wall_controller = NSViewController::new(mtm);
    wall_controller.setView(&wall);
    let inspector = Inspector::new(mtm, &look.colors);
    let inspector_controller = NSViewController::new(mtm);
    inspector_controller.setView(&inspector.scroll);

    let split = NSSplitViewController::new(mtm);
    let wall_item = NSSplitViewItem::splitViewItemWithViewController(&wall_controller);
    wall_item.setAutomaticallyAdjustsSafeAreaInsets(false);
    let inspector_item = NSSplitViewItem::inspectorWithViewController(&inspector_controller);
    inspector_item.setMinimumThickness(Inspector::width());
    inspector_item.setMaximumThickness(Inspector::width());
    inspector_item.setCanCollapse(true);
    split.addSplitViewItem(&wall_item);
    split.addSplitViewItem(&inspector_item);

    let window = main_window(mtm, &split);
    let toolbar = Toolbar::new(mtm, &actions);
    toolbar.search.setStringValue(&NSString::from_str(&filter.query));
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

fn main_window(mtm: MainThreadMarker, split: &NSSplitViewController) -> Retained<NSWindow> {
    let size = env("LW_UI_SIZE")
        .and_then(|value| {
            let (width, height) = value.split_once('x')?;
            return Some(CGSize::new(width.parse().ok()?, height.parse().ok()?));
        })
        .unwrap_or(CGSize::new(1320.0, 840.0));
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable
        | NSWindowStyleMask::FullSizeContentView;
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            CGRect::new(CGPoint::ZERO, size),
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str("LiveWall"));
    window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
    window.setToolbarStyle(NSWindowToolbarStyle::Unified);
    window.setTitlebarAppearsTransparent(true);
    window.setTitlebarSeparatorStyle(NSTitlebarSeparatorStyle::None);
    window.setContentMinSize(CGSize::new(1180.0, 600.0));
    window.setContentViewController(Some(split));
    window.setContentSize(size);
    return window;
}

fn menu_item(
    mtm: MainThreadMarker,
    title: &str,
    action: Sel,
    key: &str,
    modifiers: NSEventModifierFlags,
    target: Option<&AnyObject>,
) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(key),
        )
    };
    item.setKeyEquivalentModifierMask(modifiers);
    unsafe { item.setTarget(target) };
    return item;
}

fn submenu(mtm: MainThreadMarker, bar: &NSMenu, title: &str, items: &[Retained<NSMenuItem>]) -> Retained<NSMenu> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    for item in items {
        menu.addItem(item);
    }
    let holder = NSMenuItem::new(mtm);
    holder.setSubmenu(Some(&menu));
    bar.addItem(&holder);
    return menu;
}

fn command_item(
    mtm: MainThreadMarker,
    action: HotkeyAction,
    key: &str,
    modifiers: NSEventModifierFlags,
    target: &AnyObject,
) -> Retained<NSMenuItem> {
    let item = menu_item(mtm, settings::action_title(action), sel!(run:), key, modifiers, Some(target));
    item.setTag(
        HotkeyAction::ALL
            .iter()
            .position(|other| *other == action)
            .and_then(|index| isize::try_from(index).ok())
            .unwrap_or(0),
    );
    return item;
}

fn main_menu(mtm: MainThreadMarker, actions: &Actions) -> Retained<NSMenu> {
    let command = NSEventModifierFlags::Command;
    let shift = command | NSEventModifierFlags::Shift;
    let option = command | NSEventModifierFlags::Option;
    let target: &AnyObject = actions;
    let tagged = |item: Retained<NSMenuItem>, tag: isize| {
        item.setTag(tag);
        return item;
    };
    let bar = NSMenu::new(mtm);
    submenu(
        mtm,
        &bar,
        "LiveWall",
        &[
            menu_item(mtm, "About LiveWall", sel!(orderFrontStandardAboutPanel:), "", command, None),
            NSMenuItem::separatorItem(mtm),
            menu_item(mtm, "Settings…", sel!(showSettings:), ",", command, Some(target)),
            NSMenuItem::separatorItem(mtm),
            menu_item(mtm, "Hide LiveWall", sel!(hide:), "h", command, None),
            NSMenuItem::separatorItem(mtm),
            menu_item(mtm, "Quit LiveWall", sel!(quit:), "q", command, Some(target)),
        ],
    );
    submenu(
        mtm,
        &bar,
        "File",
        &[
            tagged(menu_item(mtm, "Add Files…", sel!(addFiles:), "n", command, Some(target)), 0),
            tagged(menu_item(mtm, "Add Folder…", sel!(addFiles:), "n", option, Some(target)), 1),
            menu_item(mtm, "Download from URL…", sel!(download:), "l", command, Some(target)),
            NSMenuItem::separatorItem(mtm),
            tagged(menu_item(mtm, "New Folder…", sel!(folder:), "n", shift, Some(target)), -2),
            NSMenuItem::separatorItem(mtm),
            menu_item(mtm, "Close Window", sel!(performClose:), "w", command, None),
        ],
    );
    submenu(
        mtm,
        &bar,
        "Edit",
        &[
            menu_item(mtm, "Undo", sel!(undo:), "z", command, None),
            menu_item(mtm, "Redo", sel!(redo:), "z", shift, None),
            NSMenuItem::separatorItem(mtm),
            menu_item(mtm, "Cut", sel!(cut:), "x", command, None),
            menu_item(mtm, "Copy", sel!(copy:), "c", command, None),
            menu_item(mtm, "Paste", sel!(paste:), "v", command, None),
            menu_item(mtm, "Select All", sel!(selectAll:), "a", command, None),
            NSMenuItem::separatorItem(mtm),
            menu_item(mtm, "Find", sel!(find:), "f", command, Some(target)),
            menu_item(mtm, "Deselect", sel!(deselect:), "\u{1b}", NSEventModifierFlags::empty(), Some(target)),
        ],
    );
    submenu(
        mtm,
        &bar,
        "View",
        &[
            menu_item(mtm, "as Grid", sel!(showGrid:), "1", command, Some(target)),
            menu_item(mtm, "as List", sel!(showList:), "2", command, Some(target)),
            NSMenuItem::separatorItem(mtm),
            menu_item(mtm, "Show Inspector", sel!(toggleInspector:), "i", option, None),
            NSMenuItem::separatorItem(mtm),
            menu_item(
                mtm,
                "Enter Full Screen",
                sel!(toggleFullScreen:),
                "f",
                command | NSEventModifierFlags::Control,
                None,
            ),
        ],
    );
    wallpaper_menus(mtm, &bar, target);
    return bar;
}

fn wallpaper_menus(mtm: MainThreadMarker, bar: &NSMenu, target: &AnyObject) {
    let command = NSEventModifierFlags::Command;
    let shift = command | NSEventModifierFlags::Shift;
    let option = command | NSEventModifierFlags::Option;
    submenu(
        mtm,
        bar,
        "Wallpaper",
        &[
            menu_item(mtm, "Set as Wallpaper", sel!(setWallpaper:), "\r", command, Some(target)),
            menu_item(mtm, "Use for Lock Screen", sel!(setLock:), "l", shift, Some(target)),
            menu_item(
                mtm,
                inspector::FRAME_TO_LOCK,
                sel!(lockFrame:),
                "l",
                shift | NSEventModifierFlags::Option,
                Some(target),
            ),
            menu_item(mtm, "Reset to Defaults", sel!(resetSettings:), "r", option, Some(target)),
            menu_item(mtm, "Remove from Library…", sel!(delete:), "\u{8}", command, Some(target)),
            NSMenuItem::separatorItem(mtm),
            command_item(mtm, HotkeyAction::Next, "]", command, target),
            command_item(mtm, HotkeyAction::Previous, "[", command, target),
            command_item(mtm, HotkeyAction::Random, "r", command, target),
            NSMenuItem::separatorItem(mtm),
            command_item(mtm, HotkeyAction::TogglePause, "p", option, target),
            command_item(mtm, HotkeyAction::VolumeUp, "\u{f700}", command, target),
            command_item(mtm, HotkeyAction::VolumeDown, "\u{f701}", command, target),
            command_item(mtm, HotkeyAction::ToggleMute, "m", shift, target),
        ],
    );
    let window = submenu(
        mtm,
        bar,
        "Window",
        &[
            menu_item(mtm, "Minimize", sel!(performMiniaturize:), "m", command, None),
            menu_item(mtm, "Zoom", sel!(performZoom:), "", command, None),
            NSMenuItem::separatorItem(mtm),
            menu_item(mtm, "Bring All to Front", sel!(arrangeInFront:), "", command, None),
        ],
    );
    NSApplication::sharedApplication(mtm).setWindowsMenu(Some(&window));
}
