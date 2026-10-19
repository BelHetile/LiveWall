use objc2::rc::Retained;
use objc2_app_kit::{NSColor, NSColorSpace, NSImage};

use crate::model::Icon;
use crate::style::{Rgba, Tag};
use crate::ui::ns;
use crate::ui::view::{Library, icon_file};

pub fn symbol(name: &str) -> Option<Retained<NSImage>> {
    return NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(name), None);
}

pub fn color(value: &str) -> Option<Retained<NSColor>> {
    if let Some(tag) = Tag::parse(value) {
        return Some(match tag {
            Tag::Red => NSColor::systemRedColor(),
            Tag::Orange => NSColor::systemOrangeColor(),
            Tag::Yellow => NSColor::systemYellowColor(),
            Tag::Green => NSColor::systemGreenColor(),
            Tag::Blue => NSColor::systemBlueColor(),
            Tag::Purple => NSColor::systemPurpleColor(),
            Tag::Pink => NSColor::systemPinkColor(),
            Tag::Gray => NSColor::systemGrayColor(),
        });
    }
    let [red, green, blue, alpha] = Rgba::parse(value)?.components();
    return Some(NSColor::colorWithSRGBRed_green_blue_alpha(red, green, blue, alpha));
}

pub fn from_rgba(rgba: Rgba) -> Retained<NSColor> {
    let [red, green, blue, alpha] = rgba.components();
    return NSColor::colorWithSRGBRed_green_blue_alpha(red, green, blue, alpha);
}

pub fn to_rgba(color: &NSColor) -> Option<Rgba> {
    let color = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
    return Some(Rgba::unit(color.redComponent(), color.greenComponent(), color.blueComponent(), 1.0));
}

pub fn tint(library: &Library, folder: usize) -> Option<Retained<NSColor>> {
    return library.colors.get(folder)?.as_deref().and_then(color);
}

pub fn default_glyph(library: &Library, folder: usize) -> &'static str {
    let locked = library.locks.get(folder).is_some_and(|lock| lock.locked);
    return match (locked, library.unlocked) {
        (true, true) => "lock.open.fill",
        (true, false) => "lock.fill",
        (false, _) => "folder",
    };
}

pub fn image(library: &Library, folder: usize) -> Option<Retained<NSImage>> {
    let custom = library.icons.get(folder).and_then(|icon| icon.as_deref()).and_then(Icon::parse);
    return match custom {
        Some(Icon::Symbol(name)) => symbol(name),
        Some(Icon::Image(file)) => icon_file(&library.icons_dir.join(file)),
        None => None,
    }
    .or_else(|| symbol(default_glyph(library, folder)));
}
