use std::cell::RefCell;
use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSButton, NSColor, NSControlTextEditingDelegate, NSDragOperation, NSDraggingDestination, NSDraggingInfo,
    NSImageView, NSLayoutAttribute, NSMenu, NSMenuItem, NSModalResponseOK, NSOpenPanel, NSPasteboardTypeFileURL,
    NSPopover, NSPopoverBehavior, NSPopoverDelegate, NSTextField, NSTextFieldDelegate, NSView, NSViewController,
};
use objc2_core_foundation::CGRect;
use objc2_foundation::{NSArray, NSCopying, NSNotification, NSObject, NSObjectProtocol, NSRectEdge, NSString, NSURL};
use proto::Request;
use uuid::Uuid;

use crate::app;
use crate::i18n::tr;
use crate::model::{self, Icon};
use crate::style::{self, Key, Tag};
use crate::ui::metrics::{self, Text};
use crate::ui::theme::{self, Theme, folder_color};
use crate::ui::widgets::{self, ns};
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
    glyph: Retained<NSColor>,
    soft: Retained<NSColor>,
    empty: Retained<NSColor>,
    preview: Retained<NSImageView>,
    field: Retained<NSTextField>,
    well: Retained<NSView>,
    colors: Vec<(Option<String>, Retained<NSButton>)>,
    icons: Vec<(String, Retained<NSButton>)>,
    popover: Retained<NSPopover>,
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
        #[unsafe(method(pickColor:))]
        fn pick_color(&self, sender: &NSButton) {
            self.choose_color(sender.tag());
        }

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

    unsafe impl NSControlTextEditingDelegate for StylePicker {
        #[unsafe(method(controlTextDidChange:))]
        fn text_did_change(&self, _notification: &NSNotification) {
            self.typed();
        }
    }

    unsafe impl NSTextFieldDelegate for StylePicker {}

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
    fn choose_color(&self, tag: isize) {
        let ivars = self.ivars();
        let Some((color, _)) = usize::try_from(tag).ok().and_then(|index| ivars.colors.get(index)) else {
            return;
        };
        ivars.field.setStringValue(&ns(color.as_deref().unwrap_or("")));
        ivars.color.borrow_mut().clone_from(color);
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
        let Some((icon, button)) = usize::try_from(tag).ok().and_then(|index| ivars.icons.get(index)) else {
            return;
        };
        let Some(response) = app::with(|app| app.remove_icon(icon)) else {
            return;
        };
        if matches!(response.body, proto::ResponseBody::Err(_)) {
            report(&response);
            return;
        }
        button.setHidden(true);
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
        ivars.field.setStringValue(&ns(""));
        self.apply();
    }

    fn typed(&self) {
        let ivars = self.ivars();
        let text = ivars.field.stringValue().to_string();
        if text.trim().is_empty() {
            *ivars.color.borrow_mut() = None;
            self.apply();
            return;
        }
        match model::parse_color(&text) {
            Some(hex) => {
                *ivars.color.borrow_mut() = Some(hex);
                self.apply();
            }
            None => self.paint(),
        }
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
        ivars.preview.setContentTintColor(Some(&ivars.glyph));
        let typed = model::parse_color(&ivars.field.stringValue().to_string());
        let fill = typed.as_deref().and_then(folder_color).unwrap_or_else(|| ivars.empty.clone());
        if let Some(layer) = ivars.well.layer() {
            layer.setBackgroundColor(Some(&widgets::cg(&ivars.well, &fill)));
        }
        for (value, button) in &ivars.colors {
            mark(button, *value == color, &ivars.soft);
        }
        let current = icon.as_deref().unwrap_or(&folders::symbol_value(ivars.fallback)).to_string();
        for (value, button) in &ivars.icons {
            mark(button, *value == current, &ivars.soft);
        }
    }
}

fn mark(button: &NSButton, on: bool, soft: &NSColor) {
    let Some(layer) = button.layer() else {
        return;
    };
    let fill = if on { soft.retain() } else { NSColor::clearColor() };
    layer.setBackgroundColor(Some(&widgets::cg(button, &fill)));
}

fn cell(
    mtm: MainThreadMarker,
    image: &objc2_app_kit::NSImage,
    tag: usize,
    action: objc2::runtime::Sel,
    tip: &str,
) -> Retained<NSButton> {
    let button = unsafe { NSButton::buttonWithImage_target_action(image, None, Some(action), mtm) };
    button.setBordered(false);
    button.setTag(isize::try_from(tag).unwrap_or(0));
    button.setToolTip(Some(&ns(tip)));
    button.setWantsLayer(true);
    if let Some(layer) = button.layer() {
        layer.setCornerRadius(metrics::RADIUS_SMALL);
    }
    button.widthAnchor().constraintEqualToConstant(CELL).setActive(true);
    button.heightAnchor().constraintEqualToConstant(CELL).setActive(true);
    return button;
}

fn grid(mtm: MainThreadMarker, buttons: &[Retained<NSButton>]) -> Retained<NSView> {
    let rows: Vec<Retained<objc2_app_kit::NSStackView>> = buttons
        .chunks(COLUMNS)
        .map(|chunk| {
            let views: Vec<&NSView> = chunk.iter().map(|button| &***button as &NSView).collect();
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

fn color_buttons(mtm: MainThreadMarker) -> Vec<(Option<String>, Retained<NSButton>)> {
    let mut colors: Vec<(Option<String>, Retained<NSButton>)> = Vec::new();
    if let Some(image) = folders::painted("circle.dashed", None, SWATCH) {
        colors.push((None, cell(mtm, &image, 0, sel!(pickColor:), &tr("Default"))));
    }
    for tag in Tag::ALL {
        let Some(image) = folders::swatch(&theme::system(tag.system()), SWATCH) else {
            continue;
        };
        let index = colors.len();
        colors.push((Some(tag.id().to_string()), cell(mtm, &image, index, sel!(pickColor:), &tr(tag.title()))));
    }
    return colors;
}

fn hex_row(
    mtm: MainThreadMarker,
    color: Option<&str>,
    theme: &Theme,
) -> (Retained<NSTextField>, Retained<NSView>, Retained<objc2_app_kit::NSStackView>) {
    let field = widgets::input(mtm, "#RRGGBB", 92.0);
    field.setStringValue(&ns(color.unwrap_or("")));
    field.setFont(Some(&metrics::digits(Text::Subheadline)));
    let well = NSView::new(mtm);
    well.setWantsLayer(true);
    let size = SWATCH + metrics::SPACE_4;
    if let Some(layer) = well.layer() {
        layer.setCornerRadius(metrics::capsule(size));
        layer.setBorderWidth(metrics::BORDER_HAIRLINE);
        layer.setBorderColor(Some(&widgets::cg(&well, &theme.color(Key::PickerDivider))));
    }
    well.widthAnchor().constraintEqualToConstant(size).setActive(true);
    well.heightAnchor().constraintEqualToConstant(size).setActive(true);
    let custom_label = widgets::label(mtm, &tr(style::CUSTOM), Text::Callout, &theme.color(Key::TextSecondary));
    let hex_row = widgets::stack(mtm, &[&custom_label, &field, &well], false, metrics::SPACE_8);
    return (field, well, hex_row);
}

fn icon_buttons(
    mtm: MainThreadMarker,
    icons_dir: &Path,
    custom: Vec<String>,
    theme: &Theme,
) -> (Vec<(String, Retained<NSButton>)>, usize) {
    let mut icons: Vec<(String, Retained<NSButton>)> = Vec::new();
    let quiet = theme.color(Key::TextSecondary);
    for symbol in folders::SYMBOLS {
        let Some(image) = folders::painted(symbol, None, SWATCH) else {
            continue;
        };
        let tag = icons.len();
        let button = cell(mtm, &image, tag, sel!(pickIcon:), symbol);
        button.setContentTintColor(Some(&quiet));
        icons.push((folders::symbol_value(symbol), button));
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
        let button = cell(mtm, &image, tag, sel!(pickIcon:), &tr("Your image · right-click to remove"));
        icons.push((value, button));
    }
    return (icons, symbol_count);
}

fn wire(picker: &StylePicker, symbol_count: usize) {
    let mtm = picker.mtm();
    let target: &AnyObject = picker;
    let buttons = picker
        .ivars()
        .colors
        .iter()
        .map(|(_, button)| button)
        .chain(picker.ivars().icons.iter().map(|(_, button)| button));
    for button in buttons {
        unsafe { button.setTarget(Some(target)) };
    }
    for (index, (_, button)) in picker.ivars().icons.iter().enumerate().skip(symbol_count) {
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
    unsafe { picker.ivars().field.setDelegate(Some(ProtocolObject::from_ref(picker))) };
    picker.ivars().popover.setDelegate(Some(ProtocolObject::from_ref(picker)));
}

fn images_view(mtm: MainThreadMarker, buttons: &[Retained<NSButton>], theme: &Theme) -> Retained<NSView> {
    if !buttons.is_empty() {
        return grid(mtm, buttons);
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
    preview.widthAnchor().constraintEqualToConstant(PREVIEW).setActive(true);
    preview.heightAnchor().constraintEqualToConstant(PREVIEW).setActive(true);
    let name = widgets::label(mtm, &library.folders[folder], Text::Title3, &theme.color(Key::Text));
    let header = widgets::stack(mtm, &[&preview, &name], false, metrics::SPACE_8);

    let colors = color_buttons(mtm);
    let color_buttons: Vec<Retained<NSButton>> = colors.iter().map(|(_, button)| button.clone()).collect();
    let swatches = grid(mtm, &color_buttons);

    let (field, well, hex_row) = hex_row(mtm, color.as_deref(), theme);

    let (icons, symbol_count) = icon_buttons(mtm, &library.icons_dir, custom, theme);
    let symbol_buttons: Vec<Retained<NSButton>> =
        icons[..symbol_count].iter().map(|(_, button)| button.clone()).collect();
    let symbols = grid(mtm, &symbol_buttons);
    let image_buttons: Vec<Retained<NSButton>> =
        icons[symbol_count..].iter().map(|(_, button)| button.clone()).collect();
    let images = images_view(mtm, &image_buttons, theme);

    let reset_button = widgets::compact(mtm, &tr("Reset"), Some(sel!(reset:)), None);
    let add_button = widgets::compact(mtm, &tr("Choose Image…"), Some(sel!(addImage:)), None);
    let done_button = widgets::compact(mtm, &tr("Done"), Some(sel!(done:)), None);
    done_button.setKeyEquivalent(&ns("\r"));
    let gap = widgets::spacer(mtm);
    let footer = widgets::stack(mtm, &[&reset_button, &add_button, &gap, &done_button], false, metrics::SPACE_8);

    let color_heading = widgets::header(mtm, &tr("Color"), theme);
    let icon_heading = widgets::header(mtm, &tr("Icon"), theme);
    let image_heading = widgets::header(mtm, &tr("Your Images"), theme);
    let divider = widgets::divider(mtm, &theme.color(Key::PickerDivider));
    let views: [&NSView; 10] = [
        &header,
        &color_heading,
        &swatches,
        &hex_row,
        &icon_heading,
        &symbols,
        &image_heading,
        &images,
        &divider,
        &footer,
    ];
    let body = widgets::stack(mtm, &views, true, metrics::SPACE_8);
    body.setAlignment(NSLayoutAttribute::Leading);
    body.setEdgeInsets(widgets::insets(PADDING, PADDING, PADDING, PADDING));
    body.setCustomSpacing_afterView(metrics::SPACE_16, &header);
    body.setCustomSpacing_afterView(metrics::SPACE_12, &hex_row);
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
        glyph: theme.color(Key::SidebarIcon),
        soft: theme.color(Key::PickerSelected),
        empty: theme.color(Key::FieldFill),
        preview,
        field,
        well,
        colors,
        icons,
        popover: popover.clone(),
    });
    let picker: Retained<StylePicker> = unsafe { msg_send![super(picker), init] };
    wire(&picker, symbol_count);
    for button in [&reset_button, &add_button, &done_button] {
        unsafe { button.setTarget(Some(&picker)) };
    }
    picker.paint();
    PICKER.with_borrow_mut(|slot| *slot = Some(picker));
    popover.showRelativeToRect_ofView_preferredEdge(rect, &view, NSRectEdge::MaxX);
}
