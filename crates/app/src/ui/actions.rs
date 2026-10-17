use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBeep, NSButton, NSColorWell, NSControlStateValueOn, NSEventType, NSMenuItem, NSMenuItemValidation,
    NSPopUpButton, NSSearchField, NSSegmentedControl, NSSlider, NSSwitch, NSWindow, NSWindowDelegate,
};
use objc2_foundation::{NSNotification, NSNumber, NSObject, NSObjectProtocol, NSTimer};
use proto::{ApplyTarget, Geometry, HotkeyAction, Request};
use serde_json::json;

use crate::app::{self, Event, post};
use crate::i18n::{key, tr};
use crate::model::ItemId;
use crate::platform::{cancel_timer, schedule_block};
use crate::style::{ACCENT, Accent, Key, Pair, Rgba, TINT, Variant};
use crate::ui::settings::{self, Change, Input};
use crate::ui::state::{
    KINDS, Scope, Sort, TAG_DELETE_FOLDER, TAG_HIDE_FOLDER, TAG_LOCK_FOLDER, TAG_NEW_FOLDER, TAG_RENAME_FOLDER,
    TAG_RESET, TAG_STYLE_FOLDER, privacy_title,
};
use crate::ui::theme;
use crate::ui::widgets::{NumberField, ns};
use crate::ui::{
    UI, Ui, busy, commit, dialogs, inspector, later, patch, report, reveal, selected_id, selected_index, send, submit,
    sync_inspector, toolbar, update, view,
};

const VOLUME_ON: f64 = 0.6;
const WELL_DELAY: f64 = 0.12;

type ColorEdit = (&'static str, String, Option<String>);

thread_local! {
    static EDIT: RefCell<Option<ColorEdit>> = const { RefCell::new(None) };
    static EDIT_TIMER: RefCell<Option<Retained<NSTimer>>> = const { RefCell::new(None) };
}
const MODE_NAMES: [&str; 3] = ["system", "light", "dark"];
pub const PRIVACY_KEY: &str = "x";

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    pub struct Actions;

    unsafe impl NSObjectProtocol for Actions {}

    impl Actions {
        #[unsafe(method(scope:))]
        fn scope(&self, sender: &NSMenuItem) {
            let scope = Scope::from_tag(sender.tag());
            commit(|ui| ui.filter.scope = scope);
        }

        #[unsafe(method(folder:))]
        fn folder(&self, sender: &AnyObject) {
            if tag_of(sender) == TAG_NEW_FOLDER {
                dialogs::new_folder(self.mtm(), None);
            }
        }

        #[unsafe(method(folderAction:))]
        fn folder_action(&self, sender: &NSMenuItem) {
            folder_action(sender);
        }

        #[unsafe(method(togglePrivacy:))]
        fn toggle_privacy(&self, _sender: Option<&AnyObject>) {
            submit(|app| {
                if app.is_unlocked() {
                    app.lock();
                } else {
                    app.request_unlock();
                }
                return None;
            });
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

        #[unsafe(method(search:))]
        fn search(&self, sender: &NSSearchField) {
            let query = sender.stringValue().to_string();
            commit(|ui| ui.filter.query = query);
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
    }

    impl Actions {
        #[unsafe(method(addFiles:))]
        fn add_files(&self, sender: &NSMenuItem) {
            dialogs::add_files(self.mtm(), sender.tag() == 1);
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
            dialogs::download_sheet(self.mtm());
        }

        #[unsafe(method(moveFolder:))]
        fn move_folder(&self, sender: &NSPopUpButton) {
            let folder = usize::try_from(sender.indexOfSelectedItem()).ok().and_then(|index| index.checked_sub(1));
            move_selected(folder);
        }

        #[unsafe(method(newFolderPrivate:))]
        fn new_folder_private(&self, sender: &NSButton) {
            let on = sender.state() == NSControlStateValueOn;
            let hide = unsafe { sender.superview() }
                .and_then(|row| row.viewWithTag(dialogs::PRIVATE_HIDE_TAG))
                .and_then(|view| view.downcast::<NSButton>().ok());
            if let Some(hide) = hide {
                hide.setEnabled(on);
                if !on {
                    hide.setState(0);
                }
            }
        }

        #[unsafe(method(moveTo:))]
        fn move_to(&self, sender: &NSMenuItem) {
            let tag = sender.tag();
            if tag == TAG_NEW_FOLDER {
                dialogs::new_folder(self.mtm(), selected_index());
                return;
            }
            move_selected(usize::try_from(tag).ok());
        }

        #[unsafe(method(delete:))]
        fn delete(&self, _sender: Option<&AnyObject>) {
            dialogs::confirm_delete(self.mtm(), None);
        }
    }

    impl Actions {
        #[unsafe(method(showSettings:))]
        fn show_settings(&self, _sender: Option<&AnyObject>) {
            update(|ui, mtm| ui.show_settings(mtm, None));
        }

        #[unsafe(method(closeSettings:))]
        fn close_settings(&self, _sender: Option<&AnyObject>) {
            later(|ui, _| {
                if let Some(settings) = &ui.settings {
                    settings.close();
                }
            });
        }

        #[unsafe(method(listen:))]
        fn listen(&self, _sender: Option<&AnyObject>) {
            update(|ui, _| {
                if let Some(item) = ui.selected.and_then(|index| ui.library.items.get(index)) {
                    ui.inspector.toggle_listen(item);
                }
            });
        }

        #[unsafe(method(hideLibrary:))]
        fn hide_library(&self, _sender: Option<&AnyObject>) {
            later(|ui, mtm| ui.hide(mtm));
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            post(Event::Quit);
        }

        #[unsafe(method(settingSlide:))]
        fn setting_slide(&self, sender: &NSSlider) {
            let (tag, value, live) = (sender.tag(), sender.doubleValue(), dragging(self.mtm()));
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
            submit(|app| {
                app.restore_hotkeys();
                return None;
            });
        }

        #[unsafe(method(mode:))]
        fn mode(&self, sender: &NSSegmentedControl) {
            let mode = MODE_NAMES.get(usize::try_from(sender.selectedSegment()).unwrap_or(0));
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

        #[unsafe(method(styleColor:))]
        fn style_color(&self, sender: &NSColorWell) {
            let tag = sender.tag();
            if let Some(value) = theme::rgba(&sender.color()) {
                later(move |ui, _| edit_color(ui, tag, value));
            }
        }

        #[unsafe(method(styleReset:))]
        fn style_reset(&self, sender: &NSButton) {
            if let Some((field, _)) = settings::style_field(sender.tag()) {
                crate::ui::style(move |app| app.style_reset(field));
            }
        }

        #[unsafe(method(accentMode:))]
        fn accent_mode(&self, sender: &NSSegmentedControl) {
            let custom = sender.selectedSegment() == 1;
            later(move |ui, _| {
                if matches!(ui.look.style().accent, Accent::Fixed(_)) == custom {
                    return;
                }
                let light = ui.look.theme.accent_value(Variant::Light).to_hex();
                let dark = ui.look.theme.accent_value(Variant::Dark).to_hex();
                crate::ui::style(move |app| {
                    if custom {
                        return app.style_set(ACCENT, &light, Some(&dark));
                    }
                    return app.style_set(ACCENT, "system", None);
                });
            });
        }

        #[unsafe(method(toggleAdvanced:))]
        fn toggle_advanced(&self, _sender: Option<&AnyObject>) {
            later(|ui, _| {
                if let Some(settings) = &ui.settings {
                    settings.toggle_advanced();
                }
            });
        }

        #[unsafe(method(renameStyle:))]
        fn rename_style(&self, _sender: Option<&AnyObject>) {
            later(|ui, mtm| {
                if let Some(settings) = &ui.settings {
                    dialogs::rename_style(mtm, &settings.window, &ui.look.style().name);
                }
            });
        }

        #[unsafe(method(deleteStyle:))]
        fn delete_style(&self, _sender: Option<&AnyObject>) {
            later(|ui, mtm| {
                if let Some(settings) = &ui.settings {
                    dialogs::delete_style(mtm, &settings.window, &ui.look.style().name);
                }
            });
        }

        #[unsafe(method(exportStyle:))]
        fn export_style(&self, _sender: Option<&AnyObject>) {
            later(|ui, mtm| dialogs::export_style(mtm, &ui.look.style().name));
        }

        #[unsafe(method(importStyle:))]
        fn import_style(&self, _sender: Option<&AnyObject>) {
            later(|_, mtm| dialogs::import_style(mtm));
        }
    }

    impl Actions {
        #[unsafe(method(setWallpaper:))]
        fn set_wallpaper(&self, _sender: Option<&AnyObject>) {
            apply_selection();
        }

        #[unsafe(method(applyTo:))]
        fn apply_to(&self, sender: &NSMenuItem) {
            place(selected_index(), Scope::from_tag(sender.tag()));
        }

        #[unsafe(method(reveal:))]
        fn reveal_item(&self, sender: &NSMenuItem) {
            if let Ok(index) = usize::try_from(sender.tag()) {
                reveal(index);
            }
        }

        #[unsafe(method(setLock:))]
        fn set_lock(&self, _sender: Option<&AnyObject>) {
            lock(false);
        }

        #[unsafe(method(lockFrame:))]
        fn lock_frame(&self, _sender: Option<&AnyObject>) {
            lock(true);
        }

        #[unsafe(method(run:))]
        fn run_command(&self, sender: &NSMenuItem) {
            if let Some(action) = HotkeyAction::ALL.get(tag_index(sender)) {
                let action = *action;
                submit(move |app| {
                    app.run_hotkey(action);
                    return None;
                });
            }
        }

        #[unsafe(method(resetSettings:))]
        fn reset_settings(&self, _sender: Option<&AnyObject>) {
            if let Some(id) = selected_id() {
                submit(move |app| Some(app.reset_item(id)));
            }
        }

        #[unsafe(method(optimize:))]
        fn optimize(&self, _sender: Option<&AnyObject>) {
            if let Some(id) = selected_id() {
                let request = Request::Optimize { id: id.to_string() };
                submit(move |app| Some(app.dispatch(&request)));
            }
        }
    }

    impl Actions {
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
                ui.inspector.target = (tag != TAG_RESET).then(|| Scope::from_tag(tag));
            });
        }

        #[unsafe(method(play:))]
        fn play(&self, _sender: Option<&AnyObject>) {
            crate::ui::toggle_play();
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
            let (tag, value, live) = (sender.tag(), sender.doubleValue(), dragging(self.mtm()));
            adjust(tag, if tag == inspector::SPEED { (value * 20.0).round() / 20.0 } else { value }, live);
        }
    }

    unsafe impl NSMenuItemValidation for Actions {
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            validate(item)
        }
    }

    unsafe impl NSWindowDelegate for Actions {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, sender: &NSWindow) -> bool {
            sender.sheetParent().is_none_or(|parent| {
                parent.endSheet(sender);
                return false;
            })
        }

        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            later(|ui, mtm| {
                if !ui.window.isVisible() {
                    ui.inspector.stop_listening();
                    crate::ui::show_in_dock(mtm, false);
                }
            });
        }

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

impl Actions {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        return unsafe { msg_send![super(this), init] };
    }
}

fn validate(item: &NSMenuItem) -> bool {
    let action = item.action();
    if action == Some(sel!(moveTo:)) || action == Some(sel!(moveFolder:)) {
        return usize::try_from(item.tag()).map_or(true, |folder| !sealed(folder));
    }
    if action == Some(sel!(folderAction:)) {
        return validate_folder(item);
    }
    if action != Some(sel!(togglePrivacy:)) {
        return true;
    }
    if let Some((unlocked, guarded)) =
        app::with(|app| (app.is_unlocked(), app.library().folders.iter().any(|folder| folder.locked)))
    {
        item.setTitle(&ns(&tr(privacy_title(unlocked))));
        item.setKeyEquivalent(&ns(if unlocked { PRIVACY_KEY } else { "" }));
        item.setHidden(!guarded);
    }
    return true;
}

fn tag_of(sender: &AnyObject) -> isize {
    if let Some(item) = sender.downcast_ref::<NSMenuItem>() {
        return item.tag();
    }
    return sender.downcast_ref::<NSButton>().map_or(0, |button| button.tag());
}

fn target_folder(item: &NSMenuItem) -> Option<usize> {
    let represented = item.representedObject().and_then(|object| object.downcast::<NSNumber>().ok());
    if let Some(number) = represented {
        return Some(number.as_usize());
    }
    return UI.with(|ui| ui.try_borrow().ok()?.as_ref()?.filter.folder);
}

fn validate_folder(item: &NSMenuItem) -> bool {
    let state = target_folder(item).and_then(|folder| {
        return UI.with(|ui| {
            let ui = ui.try_borrow().ok()?;
            let ui = ui.as_ref()?;
            return Some((*ui.library.locks.get(folder)?, ui.library.sealed(folder)));
        });
    });
    let Some((lock, sealed)) = state else {
        return false;
    };
    match item.tag() {
        TAG_LOCK_FOLDER => item.setTitle(&ns(&tr(if lock.locked { key("Remove Lock") } else { key("Lock Folder") }))),
        TAG_HIDE_FOLDER => {
            item.setState(isize::from(lock.hidden));
            return lock.locked && !sealed;
        }
        _ => {}
    }
    return !sealed;
}

fn folder_action(item: &NSMenuItem) {
    let Some(folder) = target_folder(item) else {
        return;
    };
    let Some((id, lock, sealed)) = UI.with_borrow(|ui| {
        let ui = ui.as_ref()?;
        let library = &ui.library;
        return Some((*library.folder_ids.get(folder)?, *library.locks.get(folder)?, library.sealed(folder)));
    }) else {
        return;
    };
    if sealed {
        unlock();
        return;
    }
    let (locked, hidden) = match item.tag() {
        TAG_RENAME_FOLDER => return crate::ui::rename_folder(folder),
        TAG_DELETE_FOLDER => return delete_folder(folder),
        TAG_STYLE_FOLDER => return crate::ui::style_folder(folder),
        TAG_LOCK_FOLDER => (Some(!lock.locked), None),
        TAG_HIDE_FOLDER => (None, Some(!lock.hidden)),
        _ => return,
    };
    submit(move |app| Some(app.set_folder_lock(id, locked, hidden)));
}

pub fn delete_folder(folder: usize) {
    let id = UI.with_borrow(|ui| ui.as_ref()?.library.folder_ids.get(folder).copied());
    if let (Some(id), Some(mtm)) = (id, MainThreadMarker::new()) {
        dialogs::delete_folder(mtm, id);
    }
}

pub fn open_folder(folder: Option<usize>) {
    if folder.is_some_and(sealed) {
        unlock();
        later(Ui::refresh);
        return;
    }
    commit(|ui| ui.filter.folder = folder);
}

pub fn move_items(ids: Vec<String>, folder: Option<usize>) {
    later(move |ui, _| {
        let folder = folder.and_then(|index| ui.library.folder_ids.get(index)).map(ToString::to_string);
        send(&Request::LibraryMove { ids, folder });
    });
}

pub fn import(paths: &[String], folder: Option<usize>) {
    add_media(paths, move |ui| folder.and_then(|index| ui.library.folder_ids.get(index)).copied());
}

pub fn import_here(paths: &[String]) -> bool {
    return add_media(paths, |ui| ui.filter.folder.and_then(|index| ui.library.folder_ids.get(index)).copied());
}

fn add_media(paths: &[String], folder: impl FnOnce(&Ui) -> Option<uuid::Uuid> + Send + 'static) -> bool {
    let files: Vec<String> = paths.iter().flat_map(|path| dialogs::media_in(std::path::Path::new(path))).collect();
    if files.is_empty() {
        return false;
    }
    later(move |ui, _| {
        ui.awaiting = true;
        let folder = folder(ui).map(|id| id.to_string());
        app::with(|app| app.submit(Request::LibraryAdd { paths: files, folder }));
    });
    return true;
}

fn sealed(folder: usize) -> bool {
    return UI
        .with(|ui| ui.try_borrow().ok().is_some_and(|ui| ui.as_ref().is_some_and(|ui| ui.library.sealed(folder))));
}

fn unlock() {
    submit(|app| {
        app.request_unlock();
        return None;
    });
}

fn tag_index(sender: &NSMenuItem) -> usize {
    return usize::try_from(sender.tag()).unwrap_or(0);
}

fn dragging(mtm: MainThreadMarker) -> bool {
    let event = NSApplication::sharedApplication(mtm).currentEvent();
    return event
        .is_some_and(|event| matches!(event.r#type(), NSEventType::LeftMouseDown | NSEventType::LeftMouseDragged));
}

fn configure(tag: isize, input: &Input) {
    let request = match settings::change(tag, input) {
        Some(Change::Patch(patch)) => Request::SettingsSet { patch },
        Some(Change::Login(enabled)) => Request::LoginItemSet { enabled },
        Some(Change::Style(field, value)) => {
            crate::ui::style(move |app| app.style_set(&field, &value, None));
            return;
        }
        None => return,
    };
    submit(move |app| Some(app.dispatch(&request)));
}

fn selected_item(ui: &mut Ui) -> Option<&mut view::Item> {
    return ui.selected.and_then(|index| ui.library.items.get_mut(index));
}

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

fn move_selected(folder: Option<usize>) {
    update(|ui, _| {
        let Some(item) = ui.selected.and_then(|index| ui.library.items.get(index)) else {
            return;
        };
        let folder = folder.and_then(|index| ui.library.folder_ids.get(index)).map(ToString::to_string);
        send(&Request::LibraryMove { ids: vec![item.id.to_string()], folder });
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

pub fn apply(index: usize) {
    update(|ui, _| assign(ui, ui.filter.scope, index));
}

fn edit_color(ui: &Ui, tag: isize, value: Rgba) {
    let Some((field, dark)) = settings::style_field(tag) else {
        return;
    };
    let style = ui.look.style();
    let theme = &ui.look.theme;
    let key = Key::from_id(field);
    let current = match field {
        ACCENT => match style.accent {
            Accent::Fixed(pair) => Some(pair),
            Accent::System => None,
        },
        TINT => style.tint,
        _ => key.and_then(|key| style.pair(key)),
    };
    let shown = match (field, key) {
        (ACCENT, _) => theme.accent_value(Variant::Light),
        (_, Some(key)) => theme.value(key, Variant::Light),
        _ => theme.value(Key::Chrome, Variant::Light),
    };
    let light = current.map_or(shown, |pair| pair.light);
    let next = if dark {
        Pair { light, dark: Some(value) }
    } else {
        Pair { light: value, dark: current.and_then(|pair| pair.dark) }
    };
    if current == Some(next) {
        return;
    }
    EDIT.with_borrow_mut(|slot| *slot = Some((field, next.light.to_hex(), next.dark.map(Rgba::to_hex))));
    if EDIT_TIMER.with_borrow(Option::is_some) {
        return;
    }
    let timer = schedule_block(WELL_DELAY, false, || {
        cancel_timer(EDIT_TIMER.take());
        if let Some((field, light, dark)) = EDIT.take() {
            crate::ui::style(move |app| app.style_set(field, &light, dark.as_deref()));
        }
    });
    EDIT_TIMER.with_borrow_mut(|slot| *slot = Some(timer));
}
