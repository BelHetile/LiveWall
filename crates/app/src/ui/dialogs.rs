use std::path::Path;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSAlertThirdButtonReturn, NSButton, NSButtonType,
    NSControlStateValueOn, NSLayoutAttribute, NSMenu, NSModalResponse, NSModalResponseOK, NSOpenPanel, NSPopUpButton,
    NSSavePanel, NSTextField, NSView,
};
use objc2_core_foundation::CGRect;
use objc2_foundation::{NSArray, NSString};
use proto::Request;
use uuid::Uuid;

use crate::app;
use crate::i18n::{Arg, text, tr, trf};
use crate::ui::widgets::{self, ns, raw};
use crate::ui::{UI, later, submit};

const NAME_WIDTH: f64 = 260.0;
pub const PRIVATE_HIDE_TAG: isize = 1;
const LINK_WIDTH: f64 = 340.0;
const QUALITIES: [&str; 5] = ["Best Available", "4K", "1440p", "1080p", "720p"];
const DEFAULT_QUALITY: isize = 3;

fn sheet(alert: &NSAlert, done: impl Fn(NSModalResponse) + 'static) {
    alert.window().setPreventsApplicationTerminationWhenModal(false);
    let Some(window) = UI.with_borrow(|ui| ui.as_ref().map(|ui| ui.window.clone())).filter(|window| window.isVisible())
    else {
        done(alert.runModal());
        return;
    };
    let block = RcBlock::new(done);
    alert.beginSheetModalForWindow_completionHandler(&window, Some(&block));
}

pub fn notice(message: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.setMessageText(&ns(message));
    alert.addButtonWithTitle(&ns("OK"));
    sheet(&alert, |_| {});
}

pub fn consent_sheet(url: String, message: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&ns("Install download tools?"));
    alert.setInformativeText(&raw(&trf(
        "%@. LiveWall can download yt-dlp, ffmpeg and deno into its own folder.",
        &[text(message)],
    )));
    alert.addButtonWithTitle(&ns("Install and Download"));
    alert.addButtonWithTitle(&ns("Cancel"));
    sheet(&alert, move |response| {
        if response == NSAlertFirstButtonReturn {
            let request = Request::Download { url: url.clone(), consent: true };
            submit(move |app| {
                app.submit(request);
                return None;
            });
        }
    });
}

pub fn confirm_delete(mtm: MainThreadMarker, index: Option<usize>) {
    let Some((id, title)) = UI.with_borrow(|ui| {
        let ui = ui.as_ref()?;
        let index = index.or(ui.selected)?;
        return ui.library.items.get(index).map(|item| (item.id, item.title.clone()));
    }) else {
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.setMessageText(&raw(&trf("Remove “%@” from the library?", &[text(title)])));
    alert.setInformativeText(&ns("LiveWall deletes its copy. Your original file isn’t affected."));
    let remove = alert.addButtonWithTitle(&ns("Remove"));
    remove.setHasDestructiveAction(true);
    alert.addButtonWithTitle(&ns("Cancel"));
    sheet(&alert, move |response| {
        if response == NSAlertFirstButtonReturn {
            let request = Request::LibraryRemove { ids: vec![id.to_string()] };
            submit(move |app| Some(app.dispatch(&request)));
        }
    });
}

pub fn rename_folder(mtm: MainThreadMarker, id: Uuid) {
    let Some(name) =
        app::with(|app| app.library().folders.iter().find(|folder| folder.id == id).map(|folder| folder.name.clone()))
            .flatten()
    else {
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&ns("Rename Folder"));
    let field = widgets::input(mtm, "Folder name", NAME_WIDTH);
    field.setStringValue(&raw(&name));
    alert.setAccessoryView(Some(&field));
    alert.addButtonWithTitle(&ns("Rename"));
    alert.addButtonWithTitle(&ns("Cancel"));
    alert.window().setInitialFirstResponder(Some(&field));
    sheet(&alert, move |response| {
        let name = field.stringValue().to_string().trim().to_string();
        if response != NSAlertFirstButtonReturn || name.is_empty() {
            return;
        }
        let request = Request::FolderRename { id: id.to_string(), name };
        submit(move |app| Some(app.dispatch(&request)));
    });
}

pub fn delete_folder(mtm: MainThreadMarker, id: Uuid) {
    let Some((name, count, locked)) = app::with(|app| {
        let folder = app.library().folders.iter().find(|folder| folder.id == id)?;
        return Some((folder.name.clone(), app.library().folder_items(id).len(), folder.locked));
    })
    .flatten() else {
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.setMessageText(&raw(&trf("Delete the folder “%@”?", &[text(name)])));
    let moved = match count {
        0 => tr("The folder is empty."),
        1 => tr("The wallpaper inside moves to Unfiled."),
        count => trf("The %ld wallpapers inside move to Unfiled.", &[Arg::Int(i64::try_from(count).unwrap_or(0))]),
    };
    let info = if locked && count > 0 { format!("{moved} {}", tr("They become public.")) } else { moved };
    alert.setInformativeText(&raw(&info));
    let delete = alert.addButtonWithTitle(&ns("Delete"));
    delete.setHasDestructiveAction(true);
    alert.addButtonWithTitle(&ns("Cancel"));
    sheet(&alert, move |response| {
        if response == NSAlertFirstButtonReturn {
            let request = Request::FolderDelete { id: id.to_string() };
            submit(move |app| Some(app.dispatch(&request)));
        }
    });
}

pub fn new_folder(mtm: MainThreadMarker, item: Option<usize>) {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&ns("New Folder"));
    alert.setInformativeText(&ns("Locked folders keep their wallpapers hidden until you unlock them with Touch ID."));
    let field = widgets::input(mtm, "Folder name", NAME_WIDTH);
    let target = UI.with_borrow(|ui| ui.as_ref().map(|ui| ui.actions.clone()));
    let private = unsafe {
        NSButton::checkboxWithTitle_target_action(
            &ns("Private"),
            target.as_deref().map(|target| target as &AnyObject),
            Some(sel!(newFolderPrivate:)),
            mtm,
        )
    };
    let hidden = unsafe { NSButton::checkboxWithTitle_target_action(&ns("Hide while locked"), None, None, mtm) };
    hidden.setEnabled(false);
    hidden.setTag(PRIVATE_HIDE_TAG);
    if item.is_some() && !app::with(|app| app.is_unlocked()).unwrap_or(false) {
        private.setEnabled(false);
        private.setToolTip(Some(&ns("Unlock private folders to move a wallpaper into a private folder")));
    }
    let flags = widgets::stack(mtm, &[&private, &hidden], false, 16.0);
    let accessory = widgets::stack(mtm, &[&field, &flags], true, 10.0);
    accessory.setAlignment(NSLayoutAttribute::Leading);
    accessory.setFrameSize(accessory.fittingSize());
    alert.setAccessoryView(Some(&accessory));
    alert.addButtonWithTitle(&ns("Create"));
    alert.addButtonWithTitle(&ns("Cancel"));
    alert.window().setInitialFirstResponder(Some(&field));
    sheet(&alert, move |response| {
        let name = field.stringValue().to_string().trim().to_string();
        if response != NSAlertFirstButtonReturn || name.is_empty() {
            return;
        }
        let flags = (private.state() == NSControlStateValueOn).then(|| hidden.state() == NSControlStateValueOn);
        later(move |ui, mtm| {
            let id = item.and_then(|index| ui.library.items.get(index)).map(|entry| entry.id);
            let Some(folder) = app::with(|app| app.create_folder(name, id, flags)) else {
                return;
            };
            ui.reload(mtm);
            if id.is_none() {
                ui.filter.folder = ui
                    .library
                    .folder_ids
                    .iter()
                    .position(|other| *other == folder)
                    .filter(|index| !ui.library.sealed(*index));
                ui.refresh(mtm);
            }
        });
    });
}

pub const STYLE_EXTENSION: &str = "livewall-style";

const SYMBOLS: [&str; 32] = [
    "folder.fill",
    "star.fill",
    "heart.fill",
    "flame.fill",
    "leaf.fill",
    "moon.fill",
    "sun.max.fill",
    "cloud.fill",
    "sparkles",
    "bolt.fill",
    "drop.fill",
    "snowflake",
    "mountain.2.fill",
    "tree.fill",
    "pawprint.fill",
    "fish.fill",
    "gamecontroller.fill",
    "film.fill",
    "music.note",
    "photo.fill",
    "paintpalette.fill",
    "camera.fill",
    "globe.europe.africa.fill",
    "car.fill",
    "airplane",
    "house.fill",
    "person.fill",
    "eye.fill",
    "lock.fill",
    "crown.fill",
    "theatermasks.fill",
    "atom",
];
const ICONS_PER_ROW: usize = 8;
const ICON_BUTTON: f64 = 34.0;
pub const PICK_TAG: isize = 1000;
pub const PICK_RESPONSE: isize = 5000;
pub const REOPEN_RESPONSE: isize = 4000;

thread_local! {
    static PICKING: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn picked(tag: isize) -> Option<String> {
    let index = usize::try_from(tag.checked_sub(PICK_TAG)?).ok()?;
    return PICKING.with_borrow(|icons| icons.get(index).cloned());
}

fn icon_grid(
    mtm: MainThreadMarker,
    icons: &[String],
    first: usize,
    current: Option<&str>,
    icons_dir: &Path,
    removable: bool,
) -> Retained<NSView> {
    let target = UI.with_borrow(|ui| ui.as_ref().map(|ui| ui.actions.clone()));
    let buttons: Vec<Retained<NSButton>> = icons
        .iter()
        .enumerate()
        .filter_map(|(offset, icon)| {
            let image = crate::ui::view::icon_image(icon, icons_dir, 20.0)?;
            let button = unsafe {
                NSButton::buttonWithImage_target_action(
                    &image,
                    target.as_deref().map(|target| target as &AnyObject),
                    Some(sel!(pickIcon:)),
                    mtm,
                )
            };
            button.setTag(PICK_TAG + isize::try_from(first + offset).unwrap_or(0));
            button.setButtonType(NSButtonType::PushOnPushOff);
            button.setState(isize::from(current == Some(icon.as_str())));
            button.widthAnchor().constraintEqualToConstant(ICON_BUTTON).setActive(true);
            button.heightAnchor().constraintEqualToConstant(ICON_BUTTON).setActive(true);
            if removable {
                let menu = NSMenu::new(mtm);
                let remove = widgets::menu_item(
                    mtm,
                    "Remove Icon",
                    sel!(removeIcon:),
                    target.as_deref().map(|target| target as &AnyObject),
                    button.tag(),
                    None,
                );
                menu.addItem(&remove);
                unsafe { button.setMenu(Some(&menu)) };
            }
            return Some(button);
        })
        .collect();
    let rows: Vec<Retained<objc2_app_kit::NSStackView>> = buttons
        .chunks(ICONS_PER_ROW)
        .map(|chunk| {
            let refs: Vec<&NSView> = chunk.iter().map(|button| &****button).collect();
            return widgets::stack(mtm, &refs, false, 6.0);
        })
        .collect();
    let refs: Vec<&NSView> = rows.iter().map(|row| &***row).collect();
    let grid = widgets::stack(mtm, &refs, true, 6.0);
    grid.setAlignment(NSLayoutAttribute::Leading);
    return widgets::view(&grid);
}

pub fn icon_picker(mtm: MainThreadMarker, folder: Uuid) {
    let Some((current, mine, icons_dir)) = app::with(|app| {
        let current =
            app.library().folders.iter().find(|entry| entry.id == folder).and_then(|entry| entry.icon.clone());
        return (current, app.custom_icons(), app.paths().icons_dir());
    }) else {
        return;
    };
    let symbols: Vec<String> = SYMBOLS.iter().map(|name| format!("{}{name}", crate::model::SYMBOL_PREFIX)).collect();
    let candidates: Vec<String> = symbols.iter().chain(&mine).cloned().collect();
    PICKING.with_borrow_mut(|icons| icons.clone_from(&candidates));

    let alert = NSAlert::new(mtm);
    alert.setMessageText(&ns("Folder Icon"));
    let symbols_label = widgets::label(
        mtm,
        "Symbols",
        11.0,
        &widgets::Weight::Semibold,
        &objc2_app_kit::NSColor::secondaryLabelColor(),
    );
    let symbol_grid = icon_grid(mtm, &symbols, 0, current.as_deref(), &icons_dir, false);
    let mine_label = widgets::label(
        mtm,
        "My Icons",
        11.0,
        &widgets::Weight::Semibold,
        &objc2_app_kit::NSColor::secondaryLabelColor(),
    );
    let mine_view: Retained<NSView> = if mine.is_empty() {
        widgets::view(&widgets::label(
            mtm,
            "Add your own PNG, JPEG or HEIC images with Choose Image….",
            11.0,
            &widgets::Weight::Regular,
            &objc2_app_kit::NSColor::tertiaryLabelColor(),
        ))
    } else {
        icon_grid(mtm, &mine, symbols.len(), current.as_deref(), &icons_dir, true)
    };
    let hint = (!mine.is_empty()).then(|| {
        let count = i64::try_from(mine.len()).unwrap_or(0);
        return widgets::raw_label(
            mtm,
            &crate::i18n::trp("%ld icon(s) · right-click to remove", count, &[]),
            11.0,
            &widgets::Weight::Regular,
            &objc2_app_kit::NSColor::tertiaryLabelColor(),
        );
    });
    let mut views: Vec<&NSView> = vec![&symbols_label, &symbol_grid, &mine_label, &mine_view];
    if let Some(hint) = &hint {
        views.push(hint);
    }
    let accessory = widgets::stack(mtm, &views, true, 8.0);
    accessory.setAlignment(NSLayoutAttribute::Leading);
    accessory.setCustomSpacing_afterView(14.0, &symbol_grid);
    accessory.setFrameSize(accessory.fittingSize());
    alert.setAccessoryView(Some(&accessory));
    alert.addButtonWithTitle(&ns("Choose Image…"));
    alert.addButtonWithTitle(&ns("Cancel"));
    if current.is_some() {
        alert.addButtonWithTitle(&ns("Reset"));
    }
    let response = alert.runModal();
    let choice = match response {
        code if code >= PICK_RESPONSE => PICKING.with_borrow(|icons| {
            usize::try_from(code - PICK_RESPONSE).ok().and_then(|index| icons.get(index).cloned())
        }),
        REOPEN_RESPONSE => {
            later(move |_, mtm| icon_picker(mtm, folder));
            return;
        }
        code if code == NSAlertFirstButtonReturn => match choose_icon_image(mtm) {
            Some(Ok(icon)) => Some(icon),
            Some(Err(message)) => {
                notice(&message);
                return;
            }
            None => return,
        },
        code if code == NSAlertThirdButtonReturn => None,
        _ => return,
    };
    let request = Request::FolderIcon { id: folder.to_string(), icon: choice };
    submit(move |app| Some(app.dispatch(&request)));
}

fn choose_icon_image(mtm: MainThreadMarker) -> Option<Result<String, String>> {
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(false);
    panel.setAllowsMultipleSelection(false);
    panel.setPrompt(Some(&ns("Choose")));
    if panel.runModal() != NSModalResponseOK {
        return None;
    }
    let path = panel.URL().and_then(|url| url.path())?.to_string();
    return app::with(|app| app.import_icon(Path::new(&path)));
}

pub fn save_style(mtm: MainThreadMarker) {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&ns("Save Style"));
    let field = widgets::input(mtm, "Style name", NAME_WIDTH);
    field.setStringValue(&ns("My Style"));
    alert.setAccessoryView(Some(&field));
    alert.addButtonWithTitle(&ns("Save"));
    alert.addButtonWithTitle(&ns("Cancel"));
    alert.window().setInitialFirstResponder(Some(&field));
    let response = alert.runModal();
    let name = field.stringValue().to_string();
    if response != NSAlertFirstButtonReturn {
        return;
    }
    later(move |ui, _| {
        let palette = ui.look.palette();
        match crate::model::Style::new(&name, &palette.accent.to_hex(), &palette.canvas.to_hex()) {
            Ok(style) => {
                app::with(|app| app.save_style(style, &crate::ui::palette::reserved_names()));
            }
            Err(message) => later(move |_, _| notice(&message)),
        }
    });
}

pub fn import_style(mtm: MainThreadMarker) {
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(false);
    panel.setAllowsMultipleSelection(true);
    panel.setPrompt(Some(&ns("Import")));
    panel.setMessage(Some(&ns("Choose .livewall-style or .json files")));
    if panel.runModal() != NSModalResponseOK {
        return;
    }
    let urls = panel.URLs();
    let files: Vec<String> = (0..urls.count())
        .filter_map(|position| urls.objectAtIndex(position).path())
        .map(|path| path.to_string())
        .collect();
    later(move |_, _| {
        for file in files {
            let response = match std::fs::read_to_string(&file) {
                Ok(text) => app::with(|app| app.import_styles(&text, &crate::ui::palette::reserved_names())),
                Err(error) => Some(proto::Response::err(proto::ErrorCode::Invalid, error.to_string())),
            };
            if let Some(response) = response {
                crate::ui::report(&response);
            }
        }
    });
}

pub fn export_style(mtm: MainThreadMarker) {
    let Some((name, palette)) = UI.with_borrow(|ui| {
        let ui = ui.as_ref()?;
        return ui.look.themes.get(ui.look.theme).map(|(name, palette)| (name.clone(), *palette));
    }) else {
        return;
    };
    let shown = tr(&name);
    let Ok(style) = crate::model::Style::new(&shown, &palette.accent.to_hex(), &palette.canvas.to_hex()) else {
        return;
    };
    let panel = NSSavePanel::savePanel(mtm);
    panel.setNameFieldStringValue(&raw(&format!("{shown}.{STYLE_EXTENSION}")));
    panel.setPrompt(Some(&ns("Export")));
    if panel.runModal() != NSModalResponseOK {
        return;
    }
    let Some(path) = panel.URL().and_then(|url| url.path()) else {
        return;
    };
    if let Err(error) = std::fs::write(path.to_string(), style.export()) {
        notice(&trf("Could not export the style: %@", &[text(error.to_string())]));
    }
}

pub fn delete_style() {
    later(|ui, _| {
        if !ui.look.is_saved(ui.look.theme) {
            return;
        }
        let Some((name, _)) = ui.look.themes.get(ui.look.theme).cloned() else {
            return;
        };
        let fallback = crate::ui::palette::reserved_names()[0];
        app::with(|app| app.delete_style(&name, fallback));
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

pub fn add_files(mtm: MainThreadMarker, directories: bool) {
    let Some(window) = UI.with_borrow(|ui| ui.as_ref().map(|ui| ui.window.clone())) else {
        return;
    };
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(!directories);
    panel.setCanChooseDirectories(directories);
    panel.setAllowsMultipleSelection(true);
    panel.setPrompt(Some(&ns("Add")));
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
            let folder = ui.filter.folder.and_then(|index| ui.library.folder_ids.get(index)).map(ToString::to_string);
            app::with(|app| app.submit(Request::LibraryAdd { paths, folder }));
        });
    });
    panel.beginSheetModalForWindow_completionHandler(&window, &block);
}

pub fn download_sheet(mtm: MainThreadMarker) {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&ns("Add from Video Link"));
    alert.setInformativeText(&ns(
        "Paste a link from YouTube, Vimeo or any site yt-dlp supports. The video is downloaded into your library.",
    ));
    let field = widgets::input(mtm, "https://youtu.be/…", LINK_WIDTH);
    let quality = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), CGRect::ZERO, false);
    let titles: Vec<Retained<NSString>> = QUALITIES.iter().map(|title| ns(title)).collect();
    quality.addItemsWithTitles(&NSArray::from_retained_slice(&titles));
    quality.selectItemAtIndex(DEFAULT_QUALITY);
    quality.setEnabled(false);
    let label = NSTextField::labelWithString(&ns("Quality"), mtm);
    let options = widgets::stack(mtm, &[&label, &quality], false, 8.0);
    let accessory = widgets::stack(mtm, &[&field, &options], true, 10.0);
    accessory.setAlignment(NSLayoutAttribute::Leading);
    accessory.setFrameSize(accessory.fittingSize());
    alert.setAccessoryView(Some(&accessory));
    alert.addButtonWithTitle(&ns("Download"));
    alert.addButtonWithTitle(&ns("Cancel"));
    alert.window().setInitialFirstResponder(Some(&field));
    sheet(&alert, move |response| {
        let url = field.stringValue().to_string().trim().to_string();
        if response != NSAlertFirstButtonReturn || url.is_empty() {
            return;
        }
        submit(move |app| {
            app.submit(Request::Download { url, consent: false });
            return None;
        });
    });
}
