use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBeep, NSControlStateValueOn, NSEventType, NSMenuItem, NSMenuItemValidation, NSPopUpButton,
    NSSearchField, NSSegmentedControl, NSSlider, NSSwitch, NSWindowDelegate,
};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol};
use proto::{ApplyTarget, Geometry, HotkeyAction, Request};
use serde_json::json;

use crate::app::{self, Event, post};
use crate::model::ItemId;
use crate::ui::settings::{self, Change, Input};
use crate::ui::state::{
    KINDS, Scope, Sort, TAG_LOCK_FOLDER, TAG_NEW_FOLDER, TAG_RESET, TAG_UNLOCK_FOLDER, privacy_title,
};
use crate::ui::widgets::{NumberField, ns};
use crate::ui::{
    UI, Ui, busy, commit, dialogs, inspector, later, patch, report, reveal, selected_id, selected_index, send, submit,
    sync_inspector, toolbar, update, view,
};

const VOLUME_ON: f64 = 0.6;
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
        fn folder(&self, sender: &NSMenuItem) {
            let tag = sender.tag();
            if tag == TAG_NEW_FOLDER {
                dialogs::new_folder(self.mtm(), None);
                return;
            }
            let folder = usize::try_from(tag).ok();
            if folder.is_some_and(sealed) {
                unlock();
                return;
            }
            commit(|ui| ui.filter.folder = folder);
        }

        #[unsafe(method(folderLock:))]
        fn folder_lock(&self, sender: &NSMenuItem) {
            let tag = sender.tag();
            let Some((id, lock)) = UI.with_borrow(|ui| {
                let ui = ui.as_ref()?;
                let index = ui.filter.folder?;
                return Some((*ui.library.folder_ids.get(index)?, *ui.library.locks.get(index)?));
            }) else {
                return;
            };
            let (locked, hidden) = match tag {
                TAG_LOCK_FOLDER => (Some(true), None),
                TAG_UNLOCK_FOLDER => (Some(false), None),
                _ => (None, Some(!lock.hidden)),
            };
            submit(move |app| Some(app.set_folder_lock(id, locked, hidden)));
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
            dialogs::confirm_delete(self.mtm());
        }
    }

    impl Actions {
        #[unsafe(method(showSettings:))]
        fn show_settings(&self, _sender: Option<&AnyObject>) {
            update(|ui, mtm| ui.settings(mtm).show(None));
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

        #[unsafe(method(customColor:))]
        fn custom_color(&self, _sender: Option<&AnyObject>) {
            later(|ui, _| {
                let Some((accent, canvas)) = ui.settings.as_ref().and_then(settings::Settings::custom_colors) else {
                    return;
                };
                patch(json!({ "appearance": { "theme": "Custom", "accent": accent.to_hex(), "canvas": canvas.to_hex() } }));
            });
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
    if item.action() != Some(sel!(togglePrivacy:)) {
        return true;
    }
    if let Some((unlocked, guarded)) =
        app::with(|app| (app.is_unlocked(), app.library().folders.iter().any(|folder| folder.locked)))
    {
        item.setTitle(&ns(privacy_title(unlocked)));
        item.setKeyEquivalent(&ns(if unlocked { PRIVACY_KEY } else { "" }));
        item.setHidden(!guarded);
    }
    return true;
}

fn sealed(folder: usize) -> bool {
    return UI.with_borrow(|ui| ui.as_ref().is_some_and(|ui| ui.library.sealed(folder)));
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
