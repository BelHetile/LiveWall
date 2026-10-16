use std::path::Path;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSButton, NSControlStateValueOn, NSLayoutAttribute,
    NSModalResponse, NSModalResponseOK, NSOpenPanel, NSPopUpButton, NSTextField,
};
use objc2_core_foundation::CGRect;
use objc2_foundation::{NSArray, NSString};
use proto::Request;
use uuid::Uuid;

use crate::app;
use crate::ui::widgets::{self, ns};
use crate::ui::{UI, later, selected_id, submit};

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
    alert.setInformativeText(&ns(&format!(
        "{message}. LiveWall can download yt-dlp, ffmpeg and deno into its own folder."
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

pub fn confirm_delete(mtm: MainThreadMarker) {
    let Some(title) = UI.with_borrow(|ui| {
        let ui = ui.as_ref()?;
        return ui.selected.and_then(|index| ui.library.items.get(index)).map(|item| item.title.clone());
    }) else {
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.setMessageText(&ns(&format!("Remove “{title}” from the library?")));
    alert.setInformativeText(&ns("LiveWall deletes its copy. Your original file isn’t affected."));
    let remove = alert.addButtonWithTitle(&ns("Remove"));
    remove.setHasDestructiveAction(true);
    alert.addButtonWithTitle(&ns("Cancel"));
    sheet(&alert, |response| {
        if response == NSAlertFirstButtonReturn
            && let Some(id) = selected_id()
        {
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
    field.setStringValue(&ns(&name));
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
    alert.setMessageText(&ns(&format!("Delete the folder “{name}”?")));
    let moved = match count {
        0 => String::from("The folder is empty."),
        1 => String::from("The wallpaper inside moves to Unfiled."),
        count => format!("The {count} wallpapers inside move to Unfiled."),
    };
    let info = if locked && count > 0 { format!("{moved} They become public.") } else { moved };
    alert.setInformativeText(&ns(&info));
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
