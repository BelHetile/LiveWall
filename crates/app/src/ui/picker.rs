use std::cell::RefCell;
use std::path::{Path, PathBuf};

use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBezelStyle, NSButton, NSButtonType, NSColor, NSColorPanel, NSColorPanelMode, NSColorWell, NSColorWellStyle,
    NSLayoutAttribute, NSLayoutConstraintOrientation, NSMenu, NSMenuItem, NSModalResponseOK, NSOpenPanel, NSPopover,
    NSPopoverBehavior, NSPopoverDelegate, NSStackView, NSStackViewDistribution, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView, NSViewController,
};
use objc2_core_foundation::CGRect;
use objc2_foundation::{
    NSArray, NSCopying, NSEdgeInsets, NSNotification, NSObject, NSObjectProtocol, NSRectEdge, NSString,
};
use proto::{Request, ResponseBody};
use uuid::Uuid;

use crate::app;
use crate::i18n::tr;
use crate::model::{Icon, SYMBOL_PREFIX};
use crate::style::{Rgba, Tag};
use crate::ui::view::icon_file;
use crate::ui::{UI, dialogs, folders, notice, ns, submit};

const COLUMNS: usize = 8;
const PADDING: f64 = 16.0;
const PANEL_WIDTH: f64 = 340.0;
const IMAGE_TYPES: [&str; 8] = ["png", "jpg", "jpeg", "heic", "heif", "tiff", "gif", "webp"];
const SYMBOLS: [&str; 48] = [
    "folder.fill",
    "star.fill",
    "heart.fill",
    "bookmark.fill",
    "tag.fill",
    "flag.fill",
    "bolt.fill",
    "sparkles",
    "photo.fill",
    "film.fill",
    "camera.fill",
    "paintpalette.fill",
    "music.note",
    "gamecontroller.fill",
    "tv.fill",
    "desktopcomputer",
    "sun.max.fill",
    "moon.fill",
    "cloud.fill",
    "snowflake",
    "flame.fill",
    "drop.fill",
    "leaf.fill",
    "tree.fill",
    "mountain.2.fill",
    "water.waves",
    "globe.americas.fill",
    "airplane",
    "car.fill",
    "sailboat.fill",
    "house.fill",
    "building.2.fill",
    "pawprint.fill",
    "fish.fill",
    "bird.fill",
    "ladybug.fill",
    "person.fill",
    "person.2.fill",
    "face.smiling.fill",
    "theatermasks.fill",
    "crown.fill",
    "gift.fill",
    "cup.and.saucer.fill",
    "book.fill",
    "graduationcap.fill",
    "briefcase.fill",
    "atom",
    "lock.fill",
];

struct Ivars {
    folder: Uuid,
    icons_dir: PathBuf,
    icon: RefCell<Option<String>>,
    color: RefCell<Option<String>>,
    choices: RefCell<Vec<(String, Retained<NSButton>)>>,
    tags: Vec<(Tag, Retained<NSButton>)>,
    images: Retained<NSStackView>,
    body: Retained<NSStackView>,
    well: Retained<NSColorWell>,
    hex: Retained<NSTextField>,
    popover: Retained<NSPopover>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct FolderPicker;

    unsafe impl NSObjectProtocol for FolderPicker {}

    impl FolderPicker {
        #[unsafe(method(pickIcon:))]
        fn pick_icon(&self, sender: &NSButton) {
            self.choose(sender.tag());
        }

        #[unsafe(method(pickTag:))]
        fn pick_tag(&self, sender: &NSButton) {
            self.choose_tag(sender.tag());
        }

        #[unsafe(method(colorChanged:))]
        fn color_changed(&self, sender: &NSColorWell) {
            self.commit_well(&sender.color());
        }

        #[unsafe(method(hexEntered:))]
        fn hex_entered(&self, sender: &NSTextField) {
            self.commit_hex(&sender.stringValue().to_string());
        }

        #[unsafe(method(addImage:))]
        fn add_image(&self, _sender: Option<&AnyObject>) {
            self.browse();
        }

        #[unsafe(method(removeImage:))]
        fn remove_image(&self, sender: &NSMenuItem) {
            self.remove(sender.tag());
        }

        #[unsafe(method(reset:))]
        fn reset(&self, _sender: Option<&AnyObject>) {
            self.clear();
        }
    }

    unsafe impl NSPopoverDelegate for FolderPicker {
        #[unsafe(method(popoverDidClose:))]
        fn popover_did_close(&self, _notification: &NSNotification) {
            self.closed();
        }
    }
);

define_class!(
    #[unsafe(super(NSColorWell))]
    #[thread_kind = MainThreadOnly]
    struct OpaqueWell;

    unsafe impl NSObjectProtocol for OpaqueWell {}

    impl OpaqueWell {
        #[unsafe(method(activate:))]
        fn activate(&self, exclusive: bool) {
            let _: () = unsafe { msg_send![super(self), activate: exclusive] };
            let panel = NSColorPanel::sharedColorPanel(self.mtm());
            panel.setMode(NSColorPanelMode::Wheel);
            panel.setShowsAlpha(false);
        }
    }
);

thread_local! {
    static PICKER: RefCell<Option<Retained<FolderPicker>>> = const { RefCell::new(None) };
}

fn stack(mtm: MainThreadMarker, views: &[Retained<NSView>], vertical: bool) -> Retained<NSStackView> {
    let stack = NSStackView::stackViewWithViews(&NSArray::from_retained_slice(views), mtm);
    stack.setOrientation(if vertical {
        NSUserInterfaceLayoutOrientation::Vertical
    } else {
        NSUserInterfaceLayoutOrientation::Horizontal
    });
    stack.setAlignment(if vertical { NSLayoutAttribute::Leading } else { NSLayoutAttribute::CenterY });
    return stack;
}

fn equal(mtm: MainThreadMarker, cells: &[Retained<NSView>]) -> Vec<Retained<NSStackView>> {
    return cells
        .chunks(COLUMNS)
        .map(|chunk| {
            let mut views = chunk.to_vec();
            while views.len() < COLUMNS {
                views.push(NSView::new(mtm));
            }
            let row = stack(mtm, &views, false);
            row.setDistribution(NSStackViewDistribution::FillEqually);
            return row;
        })
        .collect();
}

fn fill_rows(grid: &NSStackView, rows: &[Retained<NSStackView>]) {
    for row in rows {
        grid.addArrangedSubview(row);
        row.widthAnchor().constraintEqualToAnchor(&grid.widthAnchor()).setActive(true);
    }
}

fn choice(mtm: MainThreadMarker, image: &objc2_app_kit::NSImage, tip: &str, tag: usize) -> Retained<NSButton> {
    let button = unsafe { NSButton::buttonWithImage_target_action(image, None, Some(sel!(pickIcon:)), mtm) };
    button.setButtonType(NSButtonType::PushOnPushOff);
    button.setBezelStyle(NSBezelStyle::Toolbar);
    button.setBordered(false);
    button.setToolTip(Some(&ns(tip)));
    button.setTag(isize::try_from(tag).unwrap_or(0));
    button.setContentTintColor(Some(&NSColor::labelColor()));
    return button;
}

fn up(button: Retained<NSButton>) -> Retained<NSView> {
    return Retained::into_super(Retained::into_super(button));
}

fn rgba_of(value: &str) -> Option<Rgba> {
    return Tag::parse(value).map(Tag::reference).or_else(|| Rgba::parse(value));
}

impl FolderPicker {
    fn apply(&self, sync: bool) {
        let ivars = self.ivars();
        let request = Request::FolderStyle {
            id: ivars.folder.to_string(),
            icon: Some(ivars.icon.borrow().clone().unwrap_or_default()),
            color: Some(ivars.color.borrow().clone().unwrap_or_default()),
        };
        submit(move |app| Some(app.dispatch(&request)));
        self.paint(sync);
    }

    fn paint(&self, sync: bool) {
        let ivars = self.ivars();
        let icon = ivars.icon.borrow().clone();
        for (value, button) in ivars.choices.borrow().iter() {
            let on = icon.as_deref() == Some(value.as_str());
            button.setState(isize::from(on));
            button.setBordered(on);
        }
        let color = ivars.color.borrow().clone();
        let selected = color.as_deref().and_then(Tag::parse);
        for (tag, button) in &ivars.tags {
            let name = if Some(*tag) == selected { "checkmark.circle.fill" } else { "circle.fill" };
            button.setImage(folders::symbol(name).as_deref());
        }
        let rgba = color.as_deref().and_then(rgba_of);
        ivars.hex.setStringValue(&ns(&rgba.map(Rgba::to_hex).unwrap_or_default()));
        if sync {
            let shown = rgba.map_or_else(NSColor::controlAccentColor, folders::from_rgba);
            ivars.well.setColor(&shown);
        }
    }

    fn choose(&self, tag: isize) {
        let ivars = self.ivars();
        let value = usize::try_from(tag).ok().and_then(|index| ivars.choices.borrow().get(index).map(|c| c.0.clone()));
        if let Some(value) = value {
            *ivars.icon.borrow_mut() = Some(value);
            self.apply(false);
        }
    }

    fn choose_tag(&self, tag: isize) {
        let ivars = self.ivars();
        let tag = usize::try_from(tag).ok().and_then(|index| ivars.tags.get(index)).map(|entry| entry.0);
        if let Some(tag) = tag {
            *ivars.color.borrow_mut() = Some(tag.id().to_string());
            self.apply(true);
        }
    }

    fn commit_well(&self, color: &NSColor) {
        if let Some(rgba) = folders::to_rgba(color) {
            *self.ivars().color.borrow_mut() = Some(rgba.to_hex());
            self.apply(false);
        }
    }

    fn commit_hex(&self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            *self.ivars().color.borrow_mut() = None;
            self.apply(true);
            return;
        }
        match Rgba::parse(text) {
            Some(rgba) => {
                *self.ivars().color.borrow_mut() = Some(rgba.opaque().to_hex());
                self.apply(true);
            }
            None => self.paint(false),
        }
    }

    fn clear(&self) {
        let ivars = self.ivars();
        *ivars.icon.borrow_mut() = None;
        *ivars.color.borrow_mut() = None;
        self.apply(true);
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

    fn import(&self, path: &Path) {
        match app::with(|app| app.import_icon(path)) {
            Some(Ok(icon)) => {
                *self.ivars().icon.borrow_mut() = Some(icon);
                self.rebuild();
                self.apply(false);
            }
            Some(Err(message)) => dialogs::notice(&message),
            None => {}
        }
    }

    fn remove(&self, tag: isize) {
        let ivars = self.ivars();
        let icon = usize::try_from(tag).ok().and_then(|index| ivars.choices.borrow().get(index).map(|c| c.0.clone()));
        let Some(icon) = icon else {
            return;
        };
        let Some(response) = app::with(|app| app.remove_icon(&icon)) else {
            return;
        };
        if let ResponseBody::Err(body) = &response.body {
            notice(body.message.clone());
            return;
        }
        if ivars.icon.borrow().as_deref() == Some(icon.as_str()) {
            *ivars.icon.borrow_mut() = None;
        }
        self.rebuild();
        self.apply(false);
    }

    fn rebuild(&self) {
        let ivars = self.ivars();
        let mtm = self.mtm();
        let target: &AnyObject = self;
        let customs = app::with(|app| app.custom_icons()).unwrap_or_default();
        let side = folders::symbol("folder.fill").map(|image| image.size());
        let mut choices = ivars.choices.borrow_mut();
        choices.retain(|(value, _)| value.starts_with(SYMBOL_PREFIX));
        for view in &ivars.images.arrangedSubviews() {
            view.removeFromSuperview();
        }
        let mut cells: Vec<Retained<NSView>> = Vec::new();
        for value in customs {
            let Some(Icon::Image(file)) = Icon::parse(&value) else {
                continue;
            };
            let Some(source) = icon_file(&ivars.icons_dir.join(file)) else {
                continue;
            };
            let image = source.copy();
            if let Some(side) = side {
                image.setSize(side);
            }
            let index = choices.len();
            let button = choice(mtm, &image, &tr("Your image · right-click to remove"), index);
            button.setImageScaling(objc2_app_kit::NSImageScaling::ScaleProportionallyDown);
            unsafe { button.setTarget(Some(target)) };
            let menu = NSMenu::new(mtm);
            let remove = NSMenuItem::new(mtm);
            remove.setTitle(&ns(&tr("Remove Image")));
            remove.setTag(isize::try_from(index).unwrap_or(0));
            unsafe {
                remove.setTarget(Some(target));
                remove.setAction(Some(sel!(removeImage:)));
                button.setMenu(Some(&menu));
            }
            menu.addItem(&remove);
            choices.push((value, button.clone()));
            cells.push(up(button));
        }
        drop(choices);
        fill_rows(&ivars.images, &equal(mtm, &cells));
        ivars.images.setHidden(cells.is_empty());
        ivars.body.layoutSubtreeIfNeeded();
        ivars.popover.setContentSize(ivars.body.fittingSize());
    }

    fn closed(&self) {
        let ivars = self.ivars();
        ivars.well.deactivate();
        let mtm = self.mtm();
        if NSColorPanel::sharedColorPanelExists(mtm) {
            NSColorPanel::sharedColorPanel(mtm).close();
        }
        let closed = std::ptr::from_ref(self) as usize;
        DispatchQueue::main().exec_async(move || {
            PICKER.with_borrow_mut(|slot| {
                if slot.as_ref().is_some_and(|picker| std::ptr::from_ref::<FolderPicker>(picker) as usize == closed) {
                    *slot = None;
                }
            });
        });
    }
}

#[allow(clippy::too_many_lines)]
pub fn show(mtm: MainThreadMarker, index: usize, anchor: &NSView, rect: CGRect) {
    let Some((folder, icon, color, icons_dir)) = UI.with_borrow(|ui| {
        let library = &ui.as_ref()?.library;
        if library.sealed(index) {
            return None;
        }
        let icon = library.icons.get(index).cloned().flatten().filter(|value| Icon::parse(value).is_some());
        return Some((
            *library.folder_ids.get(index)?,
            icon,
            library.colors.get(index).cloned().flatten(),
            library.icons_dir.clone(),
        ));
    }) else {
        return;
    };
    PICKER.with_borrow_mut(|slot| {
        if let Some(previous) = slot.take() {
            previous.ivars().popover.close();
        }
    });

    let mut choices: Vec<(String, Retained<NSButton>)> = Vec::new();
    let mut cells: Vec<Retained<NSView>> = Vec::new();
    for name in SYMBOLS {
        let Some(image) = folders::symbol(name) else {
            continue;
        };
        let title = name.trim_end_matches(".fill").replace('.', " ");
        let button = choice(mtm, &image, &title, choices.len());
        choices.push((format!("{SYMBOL_PREFIX}{name}"), button.clone()));
        cells.push(up(button));
    }
    let symbols = stack(mtm, &[], true);
    fill_rows(&symbols, &equal(mtm, &cells));

    let mut tags: Vec<(Tag, Retained<NSButton>)> = Vec::new();
    let mut swatches: Vec<Retained<NSView>> = Vec::new();
    for tag in Tag::ALL {
        let image = folders::symbol("circle.fill").unwrap_or_default();
        let button = unsafe { NSButton::buttonWithImage_target_action(&image, None, Some(sel!(pickTag:)), mtm) };
        button.setBordered(false);
        button.setTag(isize::try_from(tags.len()).unwrap_or(0));
        button.setToolTip(Some(&ns(&tr(tag.title()))));
        button.setContentTintColor(folders::color(tag.id()).as_deref());
        tags.push((tag, button.clone()));
        swatches.push(up(button));
    }
    let tag_row = stack(mtm, &swatches, false);
    tag_row.setDistribution(NSStackViewDistribution::FillEqually);

    let well: Retained<OpaqueWell> = unsafe { msg_send![OpaqueWell::alloc(mtm), initWithFrame: CGRect::ZERO] };
    well.setColorWellStyle(NSColorWellStyle::Default);
    well.setContentHuggingPriority_forOrientation(750.0, NSLayoutConstraintOrientation::Horizontal);
    let hex = NSTextField::textFieldWithString(&ns(""), mtm);
    hex.setPlaceholderString(Some(&ns("#RRGGBB")));
    let reset = unsafe { NSButton::buttonWithTitle_target_action(&ns(&tr("Reset")), None, Some(sel!(reset:)), mtm) };
    let colors = stack(
        mtm,
        &[
            Retained::into_super(Retained::into_super(Retained::into_super(well.clone()))),
            Retained::into_super(Retained::into_super(hex.clone())),
            up(reset.clone()),
        ],
        false,
    );
    colors.setDistribution(NSStackViewDistribution::Fill);

    let images = stack(mtm, &[], true);
    let add = unsafe {
        NSButton::buttonWithTitle_target_action(&ns(&tr("Choose Image…")), None, Some(sel!(addImage:)), mtm)
    };

    let arranged: [Retained<NSView>; 5] = [
        Retained::into_super(tag_row.clone()),
        Retained::into_super(colors.clone()),
        Retained::into_super(symbols.clone()),
        Retained::into_super(images.clone()),
        up(add.clone()),
    ];
    let body = stack(mtm, &arranged, true);
    body.widthAnchor().constraintGreaterThanOrEqualToConstant(PANEL_WIDTH).setActive(true);
    body.setEdgeInsets(NSEdgeInsets { top: PADDING, left: PADDING, bottom: PADDING, right: PADDING });
    let sections: [&NSView; 4] = [&tag_row, &colors, &symbols, &images];
    for section in sections {
        section.leadingAnchor().constraintEqualToAnchor_constant(&body.leadingAnchor(), PADDING).setActive(true);
        section.trailingAnchor().constraintEqualToAnchor_constant(&body.trailingAnchor(), -PADDING).setActive(true);
    }

    let controller = NSViewController::new(mtm);
    controller.setView(&body);
    let popover = NSPopover::new(mtm);
    popover.setBehavior(NSPopoverBehavior::Semitransient);
    popover.setContentViewController(Some(&controller));

    let picker = FolderPicker::alloc(mtm).set_ivars(Ivars {
        folder,
        icons_dir,
        icon: RefCell::new(icon),
        color: RefCell::new(color),
        choices: RefCell::new(choices),
        tags,
        images,
        body: body.clone(),
        well: Retained::into_super(well.clone()),
        hex: hex.clone(),
        popover: popover.clone(),
    });
    let picker: Retained<FolderPicker> = unsafe { msg_send![super(picker), init] };
    let target: &AnyObject = &picker;
    unsafe {
        for (_, button) in picker.ivars().choices.borrow().iter() {
            button.setTarget(Some(target));
        }
        for (_, button) in &picker.ivars().tags {
            button.setTarget(Some(target));
        }
        well.setTarget(Some(target));
        well.setAction(Some(sel!(colorChanged:)));
        hex.setTarget(Some(target));
        hex.setAction(Some(sel!(hexEntered:)));
        reset.setTarget(Some(target));
        add.setTarget(Some(target));
    }
    popover.setDelegate(Some(ProtocolObject::from_ref(&*picker)));
    picker.rebuild();
    picker.paint(true);

    PICKER.with_borrow_mut(|slot| *slot = Some(picker));
    popover.showRelativeToRect_ofView_preferredEdge(rect, anchor, NSRectEdge::MaxX);
}
