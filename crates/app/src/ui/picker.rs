use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSButton, NSDragOperation, NSDraggingDestination, NSDraggingInfo, NSImageView, NSLayoutAttribute,
    NSMenu, NSMenuItem, NSModalResponseOK, NSOpenPanel, NSPasteboardTypeFileURL, NSPopover, NSPopoverBehavior,
    NSPopoverDelegate, NSView, NSViewController,
};
use objc2_core_foundation::CGRect;
use objc2_foundation::{NSArray, NSCopying, NSNotification, NSObject, NSObjectProtocol, NSRectEdge, NSString, NSURL};
use proto::Request;
use uuid::Uuid;

use crate::app;
use crate::i18n::tr;
use crate::model::Icon;
use crate::style::Key;
use crate::ui::metrics::{self, Text};
use crate::ui::swatches::{Panel, Pick};
use crate::ui::theme::{self, Theme};
use crate::ui::widgets::{self, Slot, Swatch, ns};
use crate::ui::{folders, report};

const COLUMNS: usize = 9;
const CELL: f64 = 30.0;
const GAP: f64 = metrics::SPACE_4;
const PREVIEW: f64 = metrics::ICON_HERO;
const SWATCH: f64 = 18.0;
const PADDING: f64 = metrics::SPACE_16;
const IMAGE_TYPES: [&str; 8] = ["png", "jpg", "jpeg", "heic", "heif", "tiff", "gif", "webp"];

pub struct PickerIvars {
    folder: Uuid,
    fallback: &'static str,
    icons_dir: PathBuf,
    icon: RefCell<Option<String>>,
    color: RefCell<Option<String>>,
    preview: Retained<NSImageView>,
    board: Retained<Panel>,
    icons: Vec<(String, Choice)>,
    popover: Retained<NSPopover>,
}

struct Choice {
    view: Retained<NSView>,
    button: Retained<NSButton>,
    mark: Retained<Swatch>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    pub struct DropView;

    unsafe impl NSObjectProtocol for DropView {}

    unsafe impl NSDraggingDestination for DropView {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            if dropped_image(info).is_some() { NSDragOperation::Copy } else { NSDragOperation::None }
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            dropped(info)
        }
    }
);

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = PickerIvars]
    pub struct StylePicker;

    unsafe impl NSObjectProtocol for StylePicker {}

    impl StylePicker {
        #[unsafe(method(pickIcon:))]
        fn pick_icon(&self, sender: &NSButton) {
            self.choose_icon(sender.tag());
        }

        #[unsafe(method(removeImage:))]
        fn remove_image(&self, sender: &NSMenuItem) {
            self.remove(sender.tag());
        }

        #[unsafe(method(addImage:))]
        fn add_image(&self, _sender: Option<&AnyObject>) {
            self.browse();
        }

        #[unsafe(method(reset:))]
        fn reset(&self, _sender: Option<&AnyObject>) {
            self.clear();
        }

        #[unsafe(method(done:))]
        fn done(&self, _sender: Option<&AnyObject>) {
            self.retain().ivars().popover.close();
        }
    }

    unsafe impl NSPopoverDelegate for StylePicker {
        #[unsafe(method(popoverDidClose:))]
        fn popover_did_close(&self, _notification: &NSNotification) {
            self.closed();
        }
    }
);

thread_local! {
    static PICKER: RefCell<Option<Retained<StylePicker>>> = const { RefCell::new(None) };
}

fn dropped(info: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
    let Some(path) = dropped_image(info) else {
        return false;
    };
    if let Some(picker) = PICKER.with_borrow(Clone::clone) {
        picker.import(&path);
    }
    return true;
}

impl StylePicker {
    fn pick(&self, pick: Option<Pick>) {
        let color = pick.map(|pick| match pick {
            Pick::Tag(tag) => tag.id().to_string(),
            Pick::Hex(color) => color.to_hex(),
        });
        *self.ivars().color.borrow_mut() = color;
        self.apply();
    }

    fn choose_icon(&self, tag: isize) {
        let ivars = self.ivars();
        let Some((icon, _)) = usize::try_from(tag).ok().and_then(|index| ivars.icons.get(index)) else {
            return;
        };
        *ivars.icon.borrow_mut() = Some(icon.clone());
        self.apply();
    }

    fn remove(&self, tag: isize) {
        let ivars = self.ivars();
        let Some((icon, choice)) = usize::try_from(tag).ok().and_then(|index| ivars.icons.get(index)) else {
            return;
        };
        let Some(response) = app::with(|app| app.remove_icon(icon)) else {
            return;
        };
        if matches!(response.body, proto::ResponseBody::Err(_)) {
            report(&response);
            return;
        }
        choice.view.setHidden(true);
        if ivars.icon.borrow().as_deref() == Some(icon.as_str()) {
            *ivars.icon.borrow_mut() = None;
        }
        self.paint();
    }

    fn browse(&self) {
        let keep = self.retain();
        let panel = NSOpenPanel::openPanel(self.mtm());
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        panel.setAllowsMultipleSelection(false);
        panel.setPrompt(Some(&ns(&tr("Use as Icon"))));
        let types: Vec<Retained<NSString>> = IMAGE_TYPES.iter().map(|kind| ns(kind)).collect();
        #[allow(deprecated)]
        panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&types)));
        if panel.runModal() != NSModalResponseOK {
            return;
        }
        if let Some(path) = panel.URL().and_then(|url| url.path()) {
            keep.import(Path::new(&path.to_string()));
        }
    }

    fn clear(&self) {
        let ivars = self.ivars();
        *ivars.icon.borrow_mut() = None;
        *ivars.color.borrow_mut() = None;
        self.apply();
    }

    fn closed(&self) {
        let closed = std::ptr::from_ref(self) as usize;
        dispatch2::DispatchQueue::main().exec_async(move || {
            PICKER.with_borrow_mut(|slot| {
                if slot.as_ref().is_some_and(|picker| std::ptr::from_ref::<StylePicker>(picker) as usize == closed) {
                    *slot = None;
                }
            });
        });
    }

    fn import(&self, path: &Path) {
        let _keep = self.retain();
        match app::with(|app| app.import_icon(path)) {
            Some(Ok(icon)) => {
                *self.ivars().icon.borrow_mut() = Some(icon);
                self.apply();
                if !self.ivars().popover.isShown() {
                    return;
                }
                let folder = self.ivars().folder;
                self.ivars().popover.close();
                crate::ui::later(move |ui, mtm| {
                    if let Some(index) = ui.library.folder_ids.iter().position(|id| *id == folder) {
                        show(mtm, ui, index);
                    }
                });
            }
            Some(Err(message)) => crate::ui::dialogs::notice(&message),
            None => {}
        }
    }

    fn apply(&self) {
        let ivars = self.ivars();
        let request = Request::FolderStyle {
            id: ivars.folder.to_string(),
            icon: Some(ivars.icon.borrow().clone().unwrap_or_default()),
            color: Some(ivars.color.borrow().clone().unwrap_or_default()),
        };
        crate::ui::submit(move |app| Some(app.dispatch(&request)));
        self.paint();
    }

    fn paint(&self) {
        let ivars = self.ivars();
        let icon = ivars.icon.borrow().clone();
        let color = ivars.color.borrow().clone();
        let image = folders::preview(icon.as_deref(), color.as_deref(), &ivars.icons_dir, ivars.fallback, PREVIEW);
        ivars.preview.setImage(image.as_deref());
        ivars.board.select(color.as_deref().and_then(Pick::parse));
        let current = icon.as_deref().unwrap_or(&folders::symbol_value(ivars.fallback)).to_string();
        for (value, choice) in &ivars.icons {
            let on = *value == current;
            choice.mark.setHidden(!on);
            choice.button.setAccessibilitySelected(on);
        }
    }
}

fn cell(
    mtm: MainThreadMarker,
    image: &objc2_app_kit::NSImage,
    tag: usize,
    action: objc2::runtime::Sel,
    tip: &str,
    theme: &Theme,
) -> Choice {
    let button = unsafe { NSButton::buttonWithImage_target_action(image, None, Some(action), mtm) };
    widgets::hoverable(&button);
    button.setTag(isize::try_from(tag).unwrap_or(0));
    widgets::describe(&button, tip, tip);
    let mark = Swatch::new(mtm, metrics::RADIUS_SMALL, Some(&theme.color(Key::PickerSelected)), None);
    mark.setHidden(true);
    let view = NSView::new(mtm);
    view.addSubview(&mark);
    view.addSubview(&button);
    widgets::pin(&mark, &view, 0.0);
    widgets::pin(&button, &view, 0.0);
    view.widthAnchor().constraintEqualToConstant(CELL).setActive(true);
    view.heightAnchor().constraintEqualToConstant(CELL).setActive(true);
    return Choice { view, button, mark };
}

fn grid(mtm: MainThreadMarker, cells: &[Retained<NSView>]) -> Retained<NSView> {
    let rows: Vec<Retained<objc2_app_kit::NSStackView>> = cells
        .chunks(COLUMNS)
        .map(|chunk| {
            let views: Vec<&NSView> = chunk.iter().map(|cell| &**cell).collect();
            return widgets::stack(mtm, &views, false, GAP);
        })
        .collect();
    let views: Vec<&NSView> = rows.iter().map(|row| &***row as &NSView).collect();
    let grid = widgets::stack(mtm, &views, true, GAP);
    grid.setAlignment(NSLayoutAttribute::Leading);
    return widgets::view(&grid);
}

fn dropped_image(info: &ProtocolObject<dyn NSDraggingInfo>) -> Option<PathBuf> {
    let kind = unsafe { NSPasteboardTypeFileURL };
    let items = info.draggingPasteboard().pasteboardItems()?;
    return items
        .iter()
        .filter_map(|item| item.stringForType(kind))
        .filter_map(|text| NSURL::URLWithString(&text))
        .filter_map(|url| url.path())
        .map(|path| PathBuf::from(path.to_string()))
        .find(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| IMAGE_TYPES.iter().any(|kind| extension.eq_ignore_ascii_case(kind)))
        });
}

fn icon_buttons(
    mtm: MainThreadMarker,
    icons_dir: &Path,
    custom: Vec<String>,
    theme: &Theme,
) -> (Vec<(String, Choice)>, usize) {
    let mut icons: Vec<(String, Choice)> = Vec::new();
    for symbol in folders::SYMBOLS {
        let Some(image) = folders::painted(symbol, None, SWATCH) else {
            continue;
        };
        let tag = icons.len();
        let title = symbol.trim_end_matches(".fill").replace('.', " ");
        let choice = cell(mtm, &image, tag, sel!(pickIcon:), &title, theme);
        choice.button.setContentTintColor(Some(&theme.color(Key::TextSecondary)));
        theme::register(&choice.button, Key::TextSecondary, Slot::Tint);
        icons.push((folders::symbol_value(symbol), choice));
    }
    let symbol_count = icons.len();
    for value in custom {
        let Some(Icon::Image(file)) = Icon::parse(&value) else {
            continue;
        };
        let Some(image) = folders::picture(&icons_dir.join(file), metrics::ICON_LARGE) else {
            continue;
        };
        let tag = icons.len();
        let choice = cell(mtm, &image, tag, sel!(pickIcon:), &tr("Your image · right-click to remove"), theme);
        icons.push((value, choice));
    }
    return (icons, symbol_count);
}

fn wire(picker: &StylePicker, symbol_count: usize) {
    let mtm = picker.mtm();
    let target: &AnyObject = picker;
    for button in picker.ivars().icons.iter().map(|(_, choice)| &choice.button) {
        unsafe { button.setTarget(Some(target)) };
    }
    for (index, (_, choice)) in picker.ivars().icons.iter().enumerate().skip(symbol_count) {
        let button = &choice.button;
        let menu = NSMenu::new(mtm);
        let remove = widgets::menu_item(
            mtm,
            &tr("Remove Image"),
            sel!(removeImage:),
            Some(target),
            isize::try_from(index).unwrap_or(0),
            None,
        );
        menu.addItem(&remove);
        unsafe { button.setMenu(Some(&menu)) };
    }
    picker.ivars().popover.setDelegate(Some(ProtocolObject::from_ref(picker)));
}

fn images_view(mtm: MainThreadMarker, cells: &[Retained<NSView>], theme: &Theme) -> Retained<NSView> {
    if !cells.is_empty() {
        return grid(mtm, cells);
    }
    let hint = widgets::label(
        mtm,
        &tr("Drop a PNG, JPEG or HEIC here, or choose one."),
        Text::Subheadline,
        &theme.color(Key::TextTertiary),
    );
    return widgets::view(&hint);
}

fn popover(mtm: MainThreadMarker, body: &NSView) -> Retained<NSPopover> {
    let root: Retained<DropView> = unsafe { msg_send![DropView::alloc(mtm), initWithFrame: CGRect::ZERO] };
    root.addSubview(body);
    widgets::pin(body, &root, 0.0);
    let types = [unsafe { NSPasteboardTypeFileURL }.copy()];
    root.registerForDraggedTypes(&NSArray::from_retained_slice(&types));
    let controller = NSViewController::new(mtm);
    controller.setView(&root);
    let popover = NSPopover::new(mtm);
    popover.setBehavior(NSPopoverBehavior::Transient);
    popover.setContentViewController(Some(&controller));
    popover.setContentSize(root.fittingSize());
    return popover;
}

pub fn show(mtm: MainThreadMarker, ui: &crate::ui::Ui, folder: usize) {
    let library = &ui.library;
    let Some(id) = library.folder_ids.get(folder).copied() else {
        return;
    };
    if library.sealed(folder) {
        return;
    }
    let Some((view, rect)) = ui.sidebar.anchor(folder) else {
        return;
    };
    let theme: &Theme = &ui.look.theme;
    let icon = library.icons.get(folder).cloned().flatten().filter(|value| Icon::parse(value).is_some());
    let color = library.colors.get(folder).cloned().flatten();
    let fallback = folders::default_glyph(library, folder);
    let custom = app::with(|app| app.custom_icons()).unwrap_or_default();

    let preview = NSImageView::new(mtm);
    preview.setContentTintColor(Some(&theme.color(Key::SidebarIcon)));
    theme::register(&preview, Key::SidebarIcon, Slot::Tint);
    preview.widthAnchor().constraintEqualToConstant(PREVIEW).setActive(true);
    preview.heightAnchor().constraintEqualToConstant(PREVIEW).setActive(true);
    let name = widgets::label(mtm, &library.folders[folder], Text::Title3, &theme.color(Key::Text));
    let header = widgets::stack(mtm, &[&preview, &name], false, metrics::SPACE_8);

    let board = Panel::new(mtm, theme, true);
    board.configure(color.as_deref().and_then(Pick::parse), false, Some(&tr("Default")), Rc::new(|_| {}));
    let swatches = board.view();

    let (icons, symbol_count) = icon_buttons(mtm, &library.icons_dir, custom, theme);
    let symbol_cells: Vec<Retained<NSView>> =
        icons[..symbol_count].iter().map(|(_, choice)| choice.view.clone()).collect();
    let symbols = grid(mtm, &symbol_cells);
    let image_cells: Vec<Retained<NSView>> =
        icons[symbol_count..].iter().map(|(_, choice)| choice.view.clone()).collect();
    let images = images_view(mtm, &image_cells, theme);

    let reset_button = widgets::compact(mtm, &tr("Reset"), Some(sel!(reset:)), None);
    let add_button = widgets::compact(mtm, &tr("Choose Image…"), Some(sel!(addImage:)), None);
    let done_button = widgets::compact(mtm, &tr("Done"), Some(sel!(done:)), None);
    done_button.setKeyEquivalent(&ns("\r"));
    let gap = widgets::spacer(mtm);
    let footer = widgets::stack(mtm, &[&reset_button, &add_button, &gap, &done_button], false, metrics::SPACE_8);

    let color_heading = widgets::header(mtm, &tr("Color"), theme);
    let icon_heading = widgets::header(mtm, &tr("Icon"), theme);
    let image_heading = widgets::header(mtm, &tr("Your Images"), theme);
    let divider = widgets::divider(mtm, Key::PickerDivider, theme);
    let views: [&NSView; 9] =
        [&header, &color_heading, &swatches, &icon_heading, &symbols, &image_heading, &images, &divider, &footer];
    let body = widgets::stack(mtm, &views, true, metrics::SPACE_8);
    body.setAlignment(NSLayoutAttribute::Leading);
    body.setEdgeInsets(widgets::insets(PADDING, PADDING, PADDING, PADDING));
    body.setCustomSpacing_afterView(metrics::SPACE_16, &header);
    body.setCustomSpacing_afterView(metrics::SPACE_12, &swatches);
    body.setCustomSpacing_afterView(metrics::SPACE_12, &symbols);
    body.setCustomSpacing_afterView(metrics::SPACE_12, &images);
    divider.leadingAnchor().constraintEqualToAnchor_constant(&body.leadingAnchor(), PADDING).setActive(true);
    divider.trailingAnchor().constraintEqualToAnchor_constant(&body.trailingAnchor(), -PADDING).setActive(true);
    footer.leadingAnchor().constraintEqualToAnchor_constant(&body.leadingAnchor(), PADDING).setActive(true);
    footer.trailingAnchor().constraintEqualToAnchor_constant(&body.trailingAnchor(), -PADDING).setActive(true);

    let popover = popover(mtm, &body);

    let picker = StylePicker::alloc(mtm).set_ivars(PickerIvars {
        folder: id,
        fallback,
        icons_dir: library.icons_dir.clone(),
        icon: RefCell::new(icon),
        color: RefCell::new(color),
        preview,
        board: board.clone(),
        icons,
        popover: popover.clone(),
    });
    let picker: Retained<StylePicker> = unsafe { msg_send![super(picker), init] };
    wire(&picker, symbol_count);
    let weak = Weak::new(&*picker);
    board.set_handler(Rc::new(move |pick| {
        if let Some(picker) = weak.load() {
            picker.pick(pick);
        }
    }));
    for button in [&reset_button, &add_button, &done_button] {
        unsafe { button.setTarget(Some(&picker)) };
    }
    picker.paint();
    PICKER.with_borrow_mut(|slot| *slot = Some(picker));
    popover.showRelativeToRect_ofView_preferredEdge(rect, &view, NSRectEdge::MaxX);
}
