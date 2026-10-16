mod fixture;
mod hotkeys;
mod inspector;
mod palette;
mod settings;
mod state;
mod toolbar;
mod wall;
mod widgets;

use std::cell::RefCell;
use std::path::PathBuf;

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSAppearance, NSAppearanceNameAqua, NSApplication,
    NSApplicationActivationPolicy, NSApplicationDelegate, NSBackingStoreType, NSBeep, NSControlStateValueOn,
    NSEventModifierFlags, NSMenu, NSMenuItem, NSModalResponse, NSModalResponseOK, NSOpenPanel, NSPopUpButton,
    NSSearchField, NSSegmentedControl, NSSlider, NSSplitViewController, NSSplitViewItem, NSSwitch, NSTextField,
    NSTitlebarSeparatorStyle, NSViewController, NSWindow, NSWindowDelegate, NSWindowStyleMask, NSWindowTitleVisibility,
    NSWindowToolbarStyle,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSString};

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

use proto::KeyCombo;

use crate::ui::fixture::Library;
use crate::ui::hotkeys::{Command, Hotkeys};
use crate::ui::inspector::Inspector;
use crate::ui::palette::{Look, Palette};
use crate::ui::state::{Filter, KINDS, Scope, Sort};
use crate::ui::toolbar::Toolbar;
use crate::ui::wall::WallView;
use crate::ui::widgets::NumberField;

struct Ui {
    library: Library,
    filter: Filter,
    look: Look,
    selected: Option<usize>,
    actions: Retained<Actions>,
    wall: Retained<WallView>,
    inspector: Inspector,
    inspector_item: Retained<NSSplitViewItem>,
    toolbar: Toolbar,
    settings: Option<settings::Settings>,
    hotkeys: Hotkeys,
    window: Retained<NSWindow>,
    split: Retained<NSSplitViewController>,
}

impl Ui {
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
        let changes = RcBlock::new(move |context: std::ptr::NonNull<objc2_app_kit::NSAnimationContext>| {
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
            let settings = settings::Settings::new(mtm, &self.look, &self.hotkeys, &self.actions);
            settings.window.setDelegate(Some(ProtocolObject::from_ref(&*self.actions)));
            return settings;
        });
    }

    fn run(&mut self, command: Command) {
        let space = self.library.current;
        let current = self.library.effective(space);
        if matches!(command, Command::Next | Command::Previous | Command::Random) {
            let visible = self.filter.visible(&self.library);
            let count = visible.len();
            if count == 0 {
                return;
            }
            let position = current.and_then(|item| visible.iter().position(|index| *index == item));
            let next = match (command, position) {
                (Command::Next, Some(position)) => (position + 1) % count,
                (Command::Previous, Some(position)) => (position + count - 1) % count,
                (Command::Random, Some(position)) if count > 1 => {
                    let offset = RandomState::new().build_hasher().finish() % (count as u64 - 1);
                    (position + 1 + usize::try_from(offset).unwrap_or(0)) % count
                }
                (Command::Previous, None) => count - 1,
                _ => 0,
            };
            self.library.replace(space, visible[next]);
            return;
        }
        let Some(index) = current else {
            return;
        };
        if command == Command::PlayPause {
            if self.selected == Some(index) {
                self.inspector.toggle_play();
            }
            return;
        }
        let settings = &mut self.library.items[index].settings;
        match command {
            Command::VolumeUp => {
                settings.volume = (if settings.muted { 0.0 } else { settings.volume } + 0.1).min(1.0);
                settings.muted = false;
            }
            Command::VolumeDown => {
                settings.volume = (settings.volume - 0.1).max(0.0);
                settings.muted = settings.muted || settings.volume <= 0.0;
            }
            _ => {
                settings.muted = !settings.muted;
                if !settings.muted && settings.volume <= 0.0 {
                    settings.volume = 0.6;
                }
            }
        }
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
        if let Some(settings) = &self.settings {
            settings.sync(&self.look);
        }
        self.refresh(mtm);
    }
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

fn update(change: impl FnOnce(&mut Ui, MainThreadMarker)) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    UI.with_borrow_mut(|ui| {
        if let Some(ui) = ui.as_mut() {
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

fn selected_item(ui: &mut Ui) -> Option<&mut fixture::Item> {
    return ui.selected.and_then(|index| ui.library.items.get_mut(index));
}

fn sync_inspector(ui: &Ui) {
    if let Some(item) = ui.selected.and_then(|index| ui.library.items.get(index)) {
        ui.inspector.sync(item);
    }
}

fn tag_index(sender: &NSMenuItem) -> usize {
    return usize::try_from(sender.tag()).unwrap_or(0);
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct Actions;

    unsafe impl NSObjectProtocol for Actions {}

    unsafe impl NSApplicationDelegate for Actions {
        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn last_window(&self, _application: &NSApplication) -> bool {
            false
        }

        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn reopen(&self, _application: &NSApplication, visible: bool) -> bool {
            if !visible {
                update(|ui, _| ui.window.makeKeyAndOrderFront(None));
            }
            true
        }
    }

    impl Actions {
        #[unsafe(method(noop:))]
        fn noop(&self, _sender: Option<&AnyObject>) {}

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
            commit(|ui| ui.filter.list = ui.toolbar.layout.selectedIndex() == 1);
        }

        #[unsafe(method(showGrid:))]
        fn show_grid(&self, _sender: Option<&AnyObject>) {
            commit(|ui| ui.filter.list = false);
        }

        #[unsafe(method(showList:))]
        fn show_list(&self, _sender: Option<&AnyObject>) {
            commit(|ui| ui.filter.list = true);
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

        #[unsafe(method(settingSlide:))]
        fn setting_slide(&self, sender: &NSSlider) {
            let (tag, value) = (sender.tag(), sender.doubleValue());
            later(move |ui, mtm| ui.settings(mtm).slid(tag, value));
        }

        #[unsafe(method(settingTyped:))]
        fn setting_typed(&self, sender: &NumberField) {
            let tag = sender.tag();
            if let Some(value) = sender.value().filter(|_| !busy()) {
                later(move |ui, mtm| ui.settings(mtm).typed(tag, value));
            }
        }

        #[unsafe(method(restoreShortcuts:))]
        fn restore_shortcuts(&self, _sender: Option<&AnyObject>) {
            update(|ui, _| {
                ui.hotkeys.restore();
                if let Some(settings) = &ui.settings {
                    settings.sync_shortcuts(&ui.hotkeys);
                }
            });
        }

        #[unsafe(method(mode:))]
        fn mode(&self, sender: &NSSegmentedControl) {
            let mode = usize::try_from(sender.selectedSegment()).unwrap_or(0);
            later(move |ui, mtm| {
                ui.look.mode = mode;
                ui.restyle(mtm);
            });
        }

        #[unsafe(method(toggle:))]
        fn toggle(&self, sender: &NSSwitch) {
            let (tag, on) = (sender.tag(), sender.state() == NSControlStateValueOn);
            update(|ui, _| {
                if let Some(settings) = &ui.settings {
                    settings.toggled(tag, on);
                }
            });
        }

        #[unsafe(method(customColor:))]
        fn custom_color(&self, _sender: Option<&AnyObject>) {
            later(|ui, mtm| {
                let Some((accent, canvas)) = ui.settings.as_ref().and_then(settings::Settings::custom_colors) else {
                    return;
                };
                ui.look.set_custom(palette::Palette::custom(accent, canvas));
                ui.restyle(mtm);
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
            update(|ui, _| {
                if let Some(item) = selected_item(ui) {
                    item.settings = fixture::Settings::default();
                }
                sync_inspector(ui);
            });
        }

        #[unsafe(method(typed:))]
        fn typed(&self, sender: &NumberField) {
            if busy() {
                return;
            }
            match sender.value() {
                Some(value) => adjust(sender.tag(), value),
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
            commit(|ui| {
                if let Some(index) = ui.selected {
                    ui.library.set_lock(index);
                }
            });
        }

        #[unsafe(method(lockFrame:))]
        fn lock_frame(&self, _sender: Option<&AnyObject>) {
            commit(|ui| {
                if let (Some(index), Some(still)) = (ui.selected, ui.inspector.still()) {
                    ui.library.set_lock_frame(index, still);
                }
            });
        }

        #[unsafe(method(delete:))]
        fn delete(&self, _sender: Option<&AnyObject>) {
            confirm_delete();
        }

        #[unsafe(method(moveFolder:))]
        fn move_folder(&self, sender: &NSPopUpButton) {
            let folder = usize::try_from(sender.indexOfSelectedItem()).ok().and_then(|index| index.checked_sub(1));
            commit(|ui| {
                if let Some(item) = selected_item(ui) {
                    item.folder = folder;
                }
            });
        }

        #[unsafe(method(moveTo:))]
        fn move_to(&self, sender: &NSMenuItem) {
            let tag = sender.tag();
            if tag == -2 {
                new_folder(UI.with_borrow(|ui| ui.as_ref().and_then(|ui| ui.selected)));
                return;
            }
            let folder = usize::try_from(tag).ok();
            commit(|ui| {
                if let Some(item) = selected_item(ui) {
                    item.folder = folder;
                }
            });
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
            if let Some(chosen) = Command::ALL.get(tag_index(sender)) {
                command(*chosen);
            }
        }

        #[unsafe(method(play:))]
        fn play(&self, _sender: Option<&AnyObject>) {
            toggle_play();
        }

        #[unsafe(method(mute:))]
        fn mute(&self, _sender: Option<&AnyObject>) {
            update(|ui, _| {
                if let Some(item) = selected_item(ui) {
                    let settings = &mut item.settings;
                    settings.muted = !settings.muted;
                    if !settings.muted && settings.volume <= 0.0 {
                        settings.volume = 0.6;
                    }
                }
                sync_inspector(ui);
            });
        }

        #[unsafe(method(fill:))]
        fn fill(&self, sender: &NSSegmentedControl) {
            let mode = sender.selectedSegment();
            update(|ui, _| {
                if let Some(item) = selected_item(ui) {
                    item.settings.fill = mode;
                }
                sync_inspector(ui);
            });
        }

        #[unsafe(method(slide:))]
        fn slide(&self, sender: &NSSlider) {
            let (tag, value) = (sender.tag(), sender.doubleValue());
            adjust(tag, if tag == inspector::SPEED { (value * 20.0).round() / 20.0 } else { value });
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

fn adjust(tag: isize, value: f64) {
    later(move |ui, _| {
        if tag == inspector::SCRUB {
            ui.inspector.seek(value.max(0.0));
            return;
        }
        if let Some(item) = selected_item(ui) {
            let value = inspector::stored(item, tag, value);
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
        }
        sync_inspector(ui);
    });
}

pub fn focus() {
    update(|ui, _| {
        ui.window.makeFirstResponder(Some(&ui.wall));
    });
}

pub fn apply_selection() {
    commit(|ui| {
        if let Some(index) = ui.selected {
            ui.library.assign(ui.inspector.target.unwrap_or(ui.filter.scope), index);
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
    commit(move |ui| {
        ui.filter.scope = scope;
        ui.library.assign(scope, item);
    });
}

pub fn command(command: Command) {
    commit(move |ui| ui.run(command));
}

pub fn rebind(command: Command, combo: Option<KeyCombo>) -> Result<(), String> {
    return UI.with_borrow_mut(|ui| {
        let Some(ui) = ui.as_mut() else {
            return Ok(());
        };
        ui.hotkeys.bind(command, combo)?;
        if let Some(settings) = &ui.settings {
            settings.sync_shortcuts(&ui.hotkeys);
        }
        return Ok(());
    });
}

pub fn recording(on: bool) {
    later(move |ui, _| {
        if on {
            ui.hotkeys.suspend();
            return;
        }
        ui.hotkeys.resume();
        if let Some(settings) = &ui.settings {
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
    commit(|ui| ui.library.assign(ui.filter.scope, index));
}

pub fn reveal(index: usize) {
    update(|ui, _| {
        ui.select(Some(index));
        ui.wall.reveal(index);
    });
}

pub fn pick_theme(index: usize) {
    later(move |ui, mtm| {
        ui.look.select(index);
        ui.restyle(mtm);
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
        menu.addItem(&wallpaper);
        menu.addItem(&item("Set as Lock Screen", sel!(setLock:), 0));
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
    let Some(window) = UI.with_borrow(|ui| ui.as_ref().map(|ui| ui.window.clone())) else {
        return;
    };
    let block = RcBlock::new(done);
    alert.beginSheetModalForWindow_completionHandler(&window, Some(&block));
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
        if response == NSAlertFirstButtonReturn {
            later(|ui, mtm| {
                if let Some(index) = ui.selected {
                    ui.select(None);
                    ui.library.remove(index);
                }
                ui.refresh(mtm);
            });
        }
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
            ui.library.folders.push(name);
            let folder = ui.library.folders.len() - 1;
            match item.and_then(|index| ui.library.items.get_mut(index)) {
                Some(entry) => entry.folder = Some(folder),
                None => ui.filter.folder = Some(folder),
            }
            ui.refresh(mtm);
        });
    });
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
        let paths: Vec<PathBuf> = (0..urls.count())
            .filter_map(|position| urls.objectAtIndex(position).path())
            .map(|path| PathBuf::from(path.to_string()))
            .collect();
        later(move |ui, mtm| {
            let before = ui.library.items.len();
            let added: usize = paths.iter().map(|path| ui.library.import(path)).sum();
            if added > 0 {
                ui.filter.folder = None;
                ui.filter.query.clear();
                ui.toolbar.search.setStringValue(&NSString::new());
            }
            ui.refresh(mtm);
            if added > 0 {
                ui.select(Some(before));
                ui.wall.reveal(before);
            }
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
    let label = NSTextField::labelWithString(&NSString::from_str("Quality"), mtm);
    let options = widgets::stack(mtm, &[&label, &quality], false, 8.0);
    let accessory = widgets::stack(mtm, &[&field, &options], true, 10.0);
    accessory.setAlignment(objc2_app_kit::NSLayoutAttribute::Leading);
    accessory.setFrameSize(accessory.fittingSize());
    alert.setAccessoryView(Some(&accessory));
    alert.addButtonWithTitle(&NSString::from_str("Download"));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    alert.window().setInitialFirstResponder(Some(&field));
    sheet(&alert, |_| {});
}

fn env(name: &str) -> Option<String> {
    return std::env::var(name).ok().filter(|value| !value.is_empty());
}

fn initial_filter(library: &Library) -> Filter {
    let scope = match env("LW_UI_SCOPE").as_deref() {
        Some("all") => Scope::All,
        Some(value) => match value.parse::<usize>().ok().and_then(|number| number.checked_sub(1)) {
            Some(space) if space < library.desktops => Scope::Space(space),
            _ => Scope::Space(0),
        },
        None => Scope::Space(0),
    };
    let folder = env("LW_UI_FOLDER").and_then(|name| library.folders.iter().position(|folder| *folder == name));
    return Filter {
        scope,
        folder,
        sort: Sort::NameAscending,
        kind: None,
        query: env("LW_UI_SEARCH").unwrap_or_default(),
        list: env("LW_UI_LAYOUT").as_deref() == Some("list"),
    };
}

fn initial_look() -> Result<Look, String> {
    let custom = env("LW_UI_PALETTE").map(|path| Palette::load(std::path::Path::new(&path))).transpose()?;
    let mode = match env("LW_UI_APPEARANCE").as_deref() {
        Some("light") => 1,
        Some("dark") => 2,
        _ => 0,
    };
    let mut look = Look::new(mode, custom);
    if let Some(theme) =
        env("LW_UI_THEME").and_then(|name| look.themes.iter().position(|(title, _)| title.eq_ignore_ascii_case(&name)))
    {
        look.select(theme);
    }
    return Ok(look);
}

pub fn run() -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("lwui must run on the main thread")?;
    let application = NSApplication::sharedApplication(mtm);
    let _ = application.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let look = initial_look()?;
    match look.mode {
        1 => application.setAppearance(NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).as_deref()),
        2 => application.setAppearance(widgets::dark().as_deref()),
        _ => {}
    }

    let root = env("LW_UI_FIXTURES").map_or_else(fixture::default_root, PathBuf::from);
    let thumbs = std::env::temp_dir().join("lwui-thumbs");
    std::fs::create_dir_all(&thumbs).map_err(|error| format!("{}: {error}", thumbs.display()))?;
    let library = fixture::load(&root, &thumbs);
    if library.items.is_empty() {
        return Err(format!("no media under {}", root.display()));
    }
    let filter = initial_filter(&library);

    let actions = Actions::new(mtm);
    application.setDelegate(Some(ProtocolObject::from_ref(&*actions)));
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

    let selected =
        env("LW_UI_SELECT").and_then(|value| value.parse::<usize>().ok()).filter(|index| *index < library.items.len());
    inspector_item.setCollapsed(selected.is_none());

    let mut ui = Ui {
        library,
        filter,
        look,
        selected,
        actions,
        wall,
        inspector,
        inspector_item,
        toolbar,
        settings: None,
        hotkeys: Hotkeys::install(),
        window: window.clone(),
        split,
    };
    ui.refresh(mtm);
    UI.with_borrow_mut(|slot| *slot = Some(ui));

    window.makeKeyAndOrderFront(None);
    focus();
    if let Some(index) = selected {
        later(move |ui, _| ui.wall.reveal(index));
    }
    #[allow(deprecated)]
    application.activateIgnoringOtherApps(true);
    if let Some(tab) = env("LW_UI_SETTINGS").and_then(|tab| tab.parse::<isize>().ok()) {
        update(|ui, mtm| ui.settings(mtm).show(Some(tab)));
    }
    let shown =
        UI.with_borrow(|ui| ui.as_ref().and_then(|ui| ui.settings.as_ref().map(|settings| settings.window.clone())));
    if let Some(path) = env("LW_UI_WINDOW_FILE") {
        std::fs::write(&path, shown.as_ref().unwrap_or(&window).windowNumber().to_string())
            .map_err(|error| format!("{path}: {error}"))?;
    }
    application.run();
    return Ok(());
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
    command: Command,
    key: &str,
    modifiers: NSEventModifierFlags,
    target: &AnyObject,
) -> Retained<NSMenuItem> {
    let item = menu_item(mtm, command.title(), sel!(run:), key, modifiers, Some(target));
    item.setTag(
        Command::ALL
            .iter()
            .position(|other| *other == command)
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
            menu_item(mtm, "Quit LiveWall", sel!(terminate:), "q", command, None),
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
            command_item(mtm, Command::Next, "]", command, target),
            command_item(mtm, Command::Previous, "[", command, target),
            command_item(mtm, Command::Random, "r", command, target),
            NSMenuItem::separatorItem(mtm),
            command_item(mtm, Command::PlayPause, "p", option, target),
            command_item(mtm, Command::VolumeUp, "\u{f700}", command, target),
            command_item(mtm, Command::VolumeDown, "\u{f701}", command, target),
            command_item(mtm, Command::Mute, "m", shift, target),
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
