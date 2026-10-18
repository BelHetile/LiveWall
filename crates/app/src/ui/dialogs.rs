use std::path::Path;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSButton, NSControlStateValueOff, NSControlStateValueOn,
    NSLayoutAttribute, NSModalResponse, NSModalResponseOK, NSOpenPanel, NSPopUpButton, NSStackView, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView, NSWindow,
};
use objc2_core_foundation::CGRect;
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSString};
use objc2_uniform_type_identifiers::{UTTypeGIF, UTTypeImage, UTTypeMovie};
use proto::Request;
use uuid::Uuid;

use crate::app;
use crate::i18n::{self, Arg, tr, trf, trp};
use crate::ui::{UI, context, later, ns, submit};

const NAME_WIDTH: f64 = 260.0;
const LINK_WIDTH: f64 = 340.0;
const QUALITIES: [&str; 5] = [i18n::key("Best Available"), "4K", "1440p", "1080p", "720p"];
const DEFAULT_QUALITY: isize = 3;

fn host() -> Option<Retained<NSWindow>> {
    return UI.with_borrow(|ui| ui.as_ref().map(|ui| ui.window.clone())).filter(|window| window.isVisible());
}

pub fn sheet(alert: &NSAlert, done: impl Fn(NSModalResponse) + 'static) {
    alert.window().setPreventsApplicationTerminationWhenModal(false);
    let Some(window) = host() else {
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
    alert.setMessageText(&ns(&tr(message)));
    alert.addButtonWithTitle(&ns(&tr("OK")));
    sheet(&alert, |_| {});
}

fn input(mtm: MainThreadMarker, placeholder: &str, width: f64) -> Retained<NSTextField> {
    let field = NSTextField::textFieldWithString(&ns(""), mtm);
    field.setPlaceholderString(Some(&ns(placeholder)));
    field.widthAnchor().constraintEqualToConstant(width).setActive(true);
    return field;
}

fn name_field(mtm: MainThreadMarker) -> Retained<NSTextField> {
    return input(mtm, &tr("Folder name"), NAME_WIDTH);
}

fn stack(mtm: MainThreadMarker, views: &[&NSView], vertical: bool) -> Retained<NSStackView> {
    let stack = NSStackView::stackViewWithViews(&NSArray::from_slice(views), mtm);
    stack.setOrientation(if vertical {
        NSUserInterfaceLayoutOrientation::Vertical
    } else {
        NSUserInterfaceLayoutOrientation::Horizontal
    });
    stack.setAlignment(if vertical { NSLayoutAttribute::Leading } else { NSLayoutAttribute::CenterY });
    return stack;
}

struct FlagsIvars {
    hidden: Retained<NSButton>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = FlagsIvars]
    struct FolderFlags;

    unsafe impl NSObjectProtocol for FolderFlags {}

    impl FolderFlags {
        #[unsafe(method(privateToggled:))]
        fn private_toggled(&self, sender: &NSButton) {
            let on = sender.state() == NSControlStateValueOn;
            let hidden = &self.ivars().hidden;
            hidden.setEnabled(on);
            if !on {
                hidden.setState(NSControlStateValueOff);
            }
        }
    }
);

impl FolderFlags {
    fn new(mtm: MainThreadMarker, hidden: Retained<NSButton>) -> Retained<Self> {
        let flags = Self::alloc(mtm).set_ivars(FlagsIvars { hidden });
        return unsafe { objc2::msg_send![super(flags), init] };
    }
}

pub fn new_folder(mtm: MainThreadMarker) {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&ns(&tr("New Folder")));
    alert.setInformativeText(&ns(&tr(
        "Locked folders keep their wallpapers hidden until you unlock them with Touch ID.",
    )));
    let field = name_field(mtm);
    let hidden = unsafe { NSButton::checkboxWithTitle_target_action(&ns(&tr("Hide while locked")), None, None, mtm) };
    hidden.setEnabled(false);
    let flags = FolderFlags::new(mtm, hidden.clone());
    let private = unsafe {
        NSButton::checkboxWithTitle_target_action(
            &ns(&tr("Private")),
            Some(&flags as &AnyObject),
            Some(sel!(privateToggled:)),
            mtm,
        )
    };
    let options = stack(mtm, &[&private, &hidden], false);
    let accessory = stack(mtm, &[&field, &options], true);
    accessory.setFrameSize(accessory.fittingSize());
    alert.setAccessoryView(Some(&accessory));
    alert.addButtonWithTitle(&ns(&tr("Create")));
    alert.addButtonWithTitle(&ns(&tr("Cancel")));
    alert.window().setInitialFirstResponder(Some(&field));
    sheet(&alert, move |response| {
        let _keep = &flags;
        let name = field.stringValue().to_string().trim().to_string();
        if response != NSAlertFirstButtonReturn || name.is_empty() {
            return;
        }
        let private = (private.state() == NSControlStateValueOn).then(|| hidden.state() == NSControlStateValueOn);
        later(move |ui, mtm| {
            let Some(folder) = app::with(|app| app.create_folder(name, None, private)) else {
                return;
            };
            ui.reload(mtm);
            ui.filter.folder = ui
                .library
                .folder_ids
                .iter()
                .position(|other| *other == folder)
                .filter(|index| !ui.library.sealed(*index));
            ui.refresh(mtm);
        });
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
    alert.setMessageText(&ns(&tr("Rename Folder")));
    let field = name_field(mtm);
    field.setStringValue(&ns(&name));
    field.setFrameSize(field.fittingSize());
    alert.setAccessoryView(Some(&field));
    alert.addButtonWithTitle(&ns(&tr("Rename")));
    alert.addButtonWithTitle(&ns(&tr("Cancel")));
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
    alert.setMessageText(&ns(&trf("Delete the folder “%@”?", &[Arg::Text(name)])));
    let moved = match count {
        0 => tr("The folder is empty."),
        1 => tr("The wallpaper inside moves to Unfiled."),
        count => trp("%ld wallpapers inside move to Unfiled.", i64::try_from(count).unwrap_or(0)),
    };
    let info = if locked && count > 0 { format!("{moved} {}", tr("They become public.")) } else { moved };
    alert.setInformativeText(&ns(&info));
    let delete = alert.addButtonWithTitle(&ns(&tr("Delete")));
    delete.setHasDestructiveAction(true);
    alert.addButtonWithTitle(&ns(&tr("Cancel")));
    sheet(&alert, move |response| {
        if response == NSAlertFirstButtonReturn {
            let request = Request::FolderDelete { id: id.to_string() };
            submit(move |app| Some(app.dispatch(&request)));
        }
    });
}

pub fn add_files(mtm: MainThreadMarker) {
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(true);
    panel.setAllowsMultipleSelection(true);
    panel.setPrompt(Some(&ns(&tr("Add"))));
    let types = unsafe { [UTTypeImage, UTTypeMovie, UTTypeGIF] };
    panel.setAllowedContentTypes(&NSArray::from_slice(&types));
    let chosen = panel.clone();
    let finish = move |response: NSModalResponse| {
        if response != NSModalResponseOK {
            return;
        }
        let paths: Vec<String> = chosen
            .URLs()
            .iter()
            .filter_map(|url| url.path())
            .flat_map(|path| context::media_in(Path::new(&path.to_string())))
            .collect();
        later(move |ui, _| {
            let folder = ui.filter.folder.filter(|index| !ui.library.sealed(*index));
            context::import(ui, paths, folder);
        });
    };
    match host() {
        Some(window) => {
            let block = RcBlock::new(finish);
            panel.beginSheetModalForWindow_completionHandler(&window, &block);
        }
        None => finish(panel.runModal()),
    }
}

pub fn download_sheet(mtm: MainThreadMarker) {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&ns(&tr("Add from Video Link")));
    alert.setInformativeText(&ns(&tr(
        "Paste a link from YouTube, Vimeo or any site yt-dlp supports. The video is downloaded into your library.",
    )));
    let field = input(mtm, "https://youtu.be/…", LINK_WIDTH);
    let quality = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), CGRect::ZERO, false);
    let titles: Vec<Retained<NSString>> = QUALITIES.iter().map(|title| ns(&tr(title))).collect();
    quality.addItemsWithTitles(&NSArray::from_retained_slice(&titles));
    quality.selectItemAtIndex(DEFAULT_QUALITY);
    quality.setEnabled(false);
    let label = NSTextField::labelWithString(&ns(&tr("Quality")), mtm);
    let options = stack(mtm, &[&label, &quality], false);
    let accessory = stack(mtm, &[&field, &options], true);
    accessory.setFrameSize(accessory.fittingSize());
    alert.setAccessoryView(Some(&accessory));
    alert.addButtonWithTitle(&ns(&tr("Download")));
    alert.addButtonWithTitle(&ns(&tr("Cancel")));
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

pub fn consent_sheet(url: String, message: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&ns(&tr("Install download tools?")));
    alert.setInformativeText(&ns(&trf(
        "%@. LiveWall can download yt-dlp, ffmpeg and deno into its own folder.",
        &[Arg::Text(message.to_string())],
    )));
    alert.addButtonWithTitle(&ns(&tr("Install and Download")));
    alert.addButtonWithTitle(&ns(&tr("Cancel")));
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
