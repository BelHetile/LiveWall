use std::path::Path;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSLayoutAttribute, NSModalResponse, NSModalResponseOK,
    NSOpenPanel, NSPopUpButton, NSTextField,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSArray, NSString};
use proto::Request;

use crate::app;
use crate::ui::widgets::{self, ns};
use crate::ui::{UI, later, selected_id, submit};

const NAME_WIDTH: f64 = 260.0;
const LINK_WIDTH: f64 = 340.0;
const FIELD_HEIGHT: f64 = 24.0;
const QUALITIES: [&str; 5] = ["Best Available", "4K", "1440p", "1080p", "720p"];
const DEFAULT_QUALITY: isize = 3;

fn sheet(alert: &NSAlert, done: impl Fn(NSModalResponse) + 'static) {
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
    alert.setInformativeText(&ns("The original file stays where it is."));
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

pub fn new_folder(mtm: MainThreadMarker, item: Option<usize>) {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&ns("New Folder"));
    alert.setInformativeText(&ns("Folders only organise the library; files are not moved."));
    let field = NSTextField::initWithFrame(
        NSTextField::alloc(mtm),
        CGRect::new(CGPoint::ZERO, CGSize::new(NAME_WIDTH, FIELD_HEIGHT)),
    );
    field.setPlaceholderString(Some(&ns("Folder name")));
    alert.setAccessoryView(Some(&field));
    alert.addButtonWithTitle(&ns("Create"));
    alert.addButtonWithTitle(&ns("Cancel"));
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
            app::with(|app| app.submit(Request::LibraryAdd { paths }));
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
    let field = NSTextField::initWithFrame(
        NSTextField::alloc(mtm),
        CGRect::new(CGPoint::ZERO, CGSize::new(LINK_WIDTH, FIELD_HEIGHT)),
    );
    field.setPlaceholderString(Some(&ns("https://youtu.be/…")));
    field.widthAnchor().constraintEqualToConstant(LINK_WIDTH).setActive(true);
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
