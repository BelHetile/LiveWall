use std::path::Path;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_app_kit::{NSColor, NSCompositingOperation, NSImage, NSRectFillUsingOperation};
use objc2_foundation::{NSCopying, NSPoint, NSRect, NSSize};

use crate::model::{Icon, SYMBOL_PREFIX};
use crate::ui::theme::folder_color;
use crate::ui::view::{Library, icon_file};
use crate::ui::widgets;

pub const SYMBOLS: [&str; 48] = [
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

pub enum Glyph<'a> {
    Symbol(&'a str),
    Image(&'a str),
}

pub fn default_glyph(library: &Library, folder: usize) -> &'static str {
    let locked = library.locks.get(folder).is_some_and(|lock| lock.locked);
    return match (locked, library.unlocked) {
        (true, true) => "lock.open.fill",
        (true, false) => "lock.fill",
        (false, _) => "folder.fill",
    };
}

pub fn glyph(library: &Library, folder: usize) -> Glyph<'_> {
    let custom = library.icons.get(folder).and_then(|icon| icon.as_deref()).and_then(Icon::parse);
    return match custom {
        Some(Icon::Symbol(name)) => Glyph::Symbol(name),
        Some(Icon::Image(file)) => Glyph::Image(file),
        None => Glyph::Symbol(default_glyph(library, folder)),
    };
}

pub fn tint(library: &Library, folder: usize) -> Option<Retained<NSColor>> {
    return library.colors.get(folder)?.as_deref().and_then(folder_color);
}

pub fn image(library: &Library, folder: usize, size: f64) -> Option<Retained<NSImage>> {
    let color = tint(library, folder);
    return match glyph(library, folder) {
        Glyph::Image(file) => picture(&library.icons_dir.join(file), size)
            .or_else(|| painted(default_glyph(library, folder), color.as_deref(), size)),
        Glyph::Symbol(name) => painted(name, color.as_deref(), size),
    };
}

pub fn preview(
    icon: Option<&str>,
    color: Option<&str>,
    icons_dir: &Path,
    fallback: &str,
    size: f64,
) -> Option<Retained<NSImage>> {
    let tint = color.and_then(folder_color);
    return match icon.and_then(Icon::parse) {
        Some(Icon::Image(file)) => picture(&icons_dir.join(file), size),
        Some(Icon::Symbol(name)) => painted(name, tint.as_deref(), size),
        None => painted(fallback, tint.as_deref(), size),
    };
}

pub fn symbol_value(name: &str) -> String {
    return format!("{SYMBOL_PREFIX}{name}");
}

pub fn picture(file: &Path, size: f64) -> Option<Retained<NSImage>> {
    let image: Retained<NSImage> = icon_file(file)?.copy();
    image.setSize(NSSize::new(size, size));
    return Some(image);
}

pub fn painted(name: &str, color: Option<&NSColor>, size: f64) -> Option<Retained<NSImage>> {
    let symbol = widgets::symbol(name, size * 0.72)?;
    let natural = symbol.size();
    if natural.width <= 0.0 || natural.height <= 0.0 {
        return None;
    }
    let color: Option<Retained<NSColor>> = color.map(Retained::from);
    let template = color.is_none();
    let draw = RcBlock::new(move |bounds: NSRect| -> Bool {
        let scale = (bounds.size.width / natural.width).min(bounds.size.height / natural.height).min(1.0);
        let fitted = NSSize::new(natural.width * scale, natural.height * scale);
        let origin = NSPoint::new((bounds.size.width - fitted.width) / 2.0, (bounds.size.height - fitted.height) / 2.0);
        let rect = NSRect::new(origin, fitted);
        symbol.drawInRect(rect);
        if let Some(color) = &color {
            color.set();
            NSRectFillUsingOperation(rect, NSCompositingOperation::SourceAtop);
        }
        return Bool::YES;
    });
    let image = NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(size, size), false, &draw);
    image.setTemplate(template);
    return Some(image);
}

pub fn swatch(color: &NSColor, size: f64) -> Option<Retained<NSImage>> {
    return painted("circle.fill", Some(color), size);
}
