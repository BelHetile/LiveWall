use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSButton, NSControlStateValueOff, NSControlStateValueOn,
    NSLayoutAttribute, NSModalResponse, NSStackView, NSTextField, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol};
use proto::Request;
use uuid::Uuid;

use crate::app;
use crate::i18n::{Arg, tr, trf, trp};
use crate::ui::{UI, later, ns, submit};

const NAME_WIDTH: f64 = 260.0;

pub fn sheet(alert: &NSAlert, done: impl Fn(NSModalResponse) + 'static) {
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
    alert.setMessageText(&ns(&tr(message)));
    alert.addButtonWithTitle(&ns(&tr("OK")));
    sheet(&alert, |_| {});
}

fn name_field(mtm: MainThreadMarker) -> Retained<NSTextField> {
    let field = NSTextField::textFieldWithString(&ns(""), mtm);
    field.setPlaceholderString(Some(&ns(&tr("Folder name"))));
    field.widthAnchor().constraintEqualToConstant(NAME_WIDTH).setActive(true);
    return field;
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
