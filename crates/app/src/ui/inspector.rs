use std::fmt::Write;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSButton, NSColor, NSControlSize, NSImage, NSLayoutAttribute, NSPopUpButton, NSScrollView, NSSegmentSwitchTracking,
    NSSegmentedControl, NSSlider, NSStackView, NSView,
};
use objc2_av_foundation::{
    AVLayerVideoGravityResizeAspectFill, AVPlayerItem, AVPlayerLayer, AVPlayerLooper, AVQueuePlayer, AVURLAsset,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSArray, NSString, NSURL};
use objc2_quartz_core::{CAAutoresizingMask, CALayer, kCAGravityResizeAspectFill};

use crate::model::ItemKind;
use crate::ui::fixture::Library;
use crate::ui::palette::Theme;
use crate::ui::wall::{duration, meta_line};
use crate::ui::widgets::{self, Weight};

const WIDTH: CGFloat = 320.0;
const INSET: CGFloat = 16.0;

pub struct Inspector {
    pub scroll: Retained<NSScrollView>,
    content: Retained<NSStackView>,
    player: Option<(Retained<AVQueuePlayer>, Retained<AVPlayerLooper>)>,
}

impl Inspector {
    pub fn new(mtm: MainThreadMarker, theme: &Theme) -> Self {
        let scroll = NSScrollView::new(mtm);
        scroll.setWantsLayer(true);
        if let Some(layer) = scroll.layer() {
            layer.setBackgroundColor(Some(&widgets::cg(mtm, &theme.chrome)));
        }
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setAutomaticallyAdjustsContentInsets(true);
        let content = widgets::stack(mtm, &[], true, 20.0);
        content.setAlignment(NSLayoutAttribute::Leading);
        content.setEdgeInsets(widgets::insets(8.0, INSET, 24.0, INSET));
        let document = widgets::Flipped::new(mtm);
        document.setTranslatesAutoresizingMaskIntoConstraints(false);
        document.addSubview(&content);
        widgets::pin(&content, &document, 0.0);
        scroll.setDocumentView(Some(&document));
        let clip = scroll.contentView();
        document.leadingAnchor().constraintEqualToAnchor(&clip.leadingAnchor()).setActive(true);
        document.trailingAnchor().constraintEqualToAnchor(&clip.trailingAnchor()).setActive(true);
        document.topAnchor().constraintEqualToAnchor(&clip.topAnchor()).setActive(true);
        return Self { scroll, content, player: None };
    }

    pub fn width() -> CGFloat {
        return WIDTH;
    }

    pub fn show(
        &mut self,
        mtm: MainThreadMarker,
        library: &Library,
        theme: &Theme,
        selected: Option<usize>,
        target: &AnyObject,
    ) {
        let views = self.content.arrangedSubviews();
        for position in 0..views.count() {
            let view = views.objectAtIndex(position);
            self.content.removeArrangedSubview(&view);
            view.removeFromSuperview();
        }
        if let Some((player, _)) = self.player.take() {
            unsafe { player.pause() };
        }
        let Some(index) = selected else {
            self.empty(mtm);
            return;
        };
        let item = &library.items[index];

        let preview = self.preview(mtm, library, index);
        let controls = Self::media_controls(mtm, item.duration, item.kind == ItemKind::Video, theme, target);

        let title = widgets::label(mtm, &item.title, 17.0, &Weight::Semibold, &NSColor::labelColor());
        let mut info = meta_line(item);
        if let Some(codec) = &item.codec {
            let _ = write!(info, " · {}", codec.to_uppercase());
        }
        if item.fps > 0.0 {
            let _ = write!(info, " · {:.0} fps", item.fps);
        }
        #[allow(clippy::cast_precision_loss)]
        let _ = write!(info, " · {:.1} MB", item.bytes as f64 / 1_048_576.0);
        let meta = widgets::label(mtm, &info, 12.0, &Weight::Regular, &NSColor::secondaryLabelColor());
        let markers = widgets::marker_text(library, &library.markers(index));
        let chip_views: Vec<Retained<NSView>> =
            markers.iter().map(|(text, icon)| shown_chip(mtm, text, icon, theme)).collect();
        let chip_refs: Vec<&NSView> = chip_views.iter().map(|view| &**view).collect();
        let chips = widgets::stack(mtm, &chip_refs, false, 6.0);
        let mut header_views: Vec<&NSView> = vec![&title, &meta];
        if !chip_refs.is_empty() {
            header_views.push(&chips);
        }
        let header = widgets::stack(mtm, &header_views, true, 6.0);
        header.setAlignment(NSLayoutAttribute::Leading);
        header.setCustomSpacing_afterView(10.0, &meta);

        let playback = Self::playback(mtm, item.kind == ItemKind::Video, theme, target);
        let placement = placement(mtm, theme, target);
        let organize = organize(mtm, library, item.folder, target);
        let actions = actions(mtm, library, theme, target);

        let sections: [&NSView; 7] = [&preview, &controls, &header, &playback, &placement, &organize, &actions];
        for view in sections {
            self.content.addArrangedSubview(view);
            view.leadingAnchor().constraintEqualToAnchor_constant(&self.content.leadingAnchor(), INSET).setActive(true);
            view.trailingAnchor()
                .constraintEqualToAnchor_constant(&self.content.trailingAnchor(), -INSET)
                .setActive(true);
        }
        self.content.setCustomSpacing_afterView(10.0, &preview);
        preview.heightAnchor().constraintEqualToAnchor_multiplier(&preview.widthAnchor(), 10.0 / 16.0).setActive(true);
    }

    fn empty(&self, mtm: MainThreadMarker) {
        let icon = widgets::symbol_view(mtm, "photo.on.rectangle.angled", 32.0, &NSColor::tertiaryLabelColor());
        let title = widgets::label(mtm, "No Selection", 15.0, &Weight::Semibold, &NSColor::secondaryLabelColor());
        let hint = widgets::label(
            mtm,
            "Select a wallpaper to see its details.",
            12.0,
            &Weight::Regular,
            &NSColor::tertiaryLabelColor(),
        );
        let stack = widgets::stack(mtm, &[&icon, &title, &hint], true, 8.0);
        stack.setAlignment(NSLayoutAttribute::CenterX);
        self.content.addArrangedSubview(&stack);
        stack.leadingAnchor().constraintEqualToAnchor(&self.content.leadingAnchor()).setActive(true);
        stack.trailingAnchor().constraintEqualToAnchor(&self.content.trailingAnchor()).setActive(true);
        stack.topAnchor().constraintEqualToAnchor_constant(&self.content.topAnchor(), 180.0).setActive(true);
    }

    fn preview(&mut self, mtm: MainThreadMarker, library: &Library, index: usize) -> Retained<NSView> {
        let item = &library.items[index];
        let view = NSView::new(mtm);
        let layer: Retained<CALayer> = if item.kind == ItemKind::Video {
            let url = NSURL::fileURLWithPath(&NSString::from_str(&item.path.to_string_lossy()));
            let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) };
            let player = unsafe { AVQueuePlayer::new(mtm) };
            let template = unsafe { AVPlayerItem::playerItemWithAsset(&asset, mtm) };
            let looper = unsafe { AVPlayerLooper::playerLooperWithPlayer_templateItem(&player, &template) };
            unsafe {
                player.setMuted(true);
                player.play();
            }
            let layer = unsafe { AVPlayerLayer::playerLayerWithPlayer(Some(&player)) };
            if let Some(gravity) = unsafe { AVLayerVideoGravityResizeAspectFill } {
                unsafe { layer.setVideoGravity(gravity) };
            }
            self.player = Some((player, looper));
            Retained::into_super(layer)
        } else {
            let layer = CALayer::new();
            if let Some(thumb) =
                NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(&item.path.to_string_lossy()))
            {
                unsafe { layer.setContents(Some(&thumb)) };
            }
            layer.setContentsGravity(unsafe { kCAGravityResizeAspectFill });
            layer
        };
        layer.setAutoresizingMask(CAAutoresizingMask::LayerWidthSizable | CAAutoresizingMask::LayerHeightSizable);
        view.setWantsLayer(true);
        if let Some(host) = view.layer() {
            host.setCornerRadius(12.0);
            host.setMasksToBounds(true);
            host.setBorderWidth(0.5);
            host.setBorderColor(Some(&widgets::cg(mtm, &NSColor::separatorColor())));
            host.addSublayer(&layer);
        }
        return view;
    }

    fn media_controls(
        mtm: MainThreadMarker,
        length: f64,
        video: bool,
        theme: &Theme,
        target: &AnyObject,
    ) -> Retained<NSView> {
        let play = icon_button(mtm, "pause.fill", target);
        let scrub = unsafe {
            NSSlider::sliderWithValue_minValue_maxValue_target_action(
                0.35,
                0.0,
                1.0,
                Some(target),
                Some(sel!(noop:)),
                mtm,
            )
        };
        scrub.setTrackFillColor(Some(&theme.accent));
        scrub.setControlSize(NSControlSize::Small);
        let time = widgets::label(
            mtm,
            &format!("{} / {}", duration(length * 0.35), duration(length)),
            11.0,
            &Weight::Regular,
            &NSColor::secondaryLabelColor(),
        );
        time.setFont(Some(&objc2_app_kit::NSFont::monospacedDigitSystemFontOfSize_weight(11.0, unsafe {
            objc2_app_kit::NSFontWeightRegular
        })));
        let row = widgets::stack(mtm, &[&play, &scrub, &time], false, 10.0);
        if !video {
            row.setHidden(true);
        }
        return widgets::view(&row);
    }

    fn playback(mtm: MainThreadMarker, video: bool, theme: &Theme, target: &AnyObject) -> Retained<NSView> {
        let mute = icon_button(mtm, "speaker.wave.2.fill", target);
        let volume = slider(mtm, 0.6, 0.0, 1.0, theme, target);
        let volume_box = widgets::stack(mtm, &[&mute, &volume], false, 6.0);
        let speed = slider(mtm, 1.0, 0.25, 2.0, theme, target);
        let speed_value = widgets::label(mtm, "1.0×", 12.0, &Weight::Regular, &NSColor::secondaryLabelColor());
        let speed_box = widgets::stack(mtm, &[&speed, &speed_value], false, 8.0);
        let volume_row = widgets::row(mtm, "Volume", &volume_box);
        let speed_row = widgets::row(mtm, "Speed", &speed_box);
        let section = widgets::group(mtm, Some("Playback"), &[&volume_row, &speed_row]);
        if !video {
            section.setHidden(true);
        }
        return widgets::view(&section);
    }
}

fn placement(mtm: MainThreadMarker, theme: &Theme, target: &AnyObject) -> Retained<NSView> {
    let labels: Vec<Retained<NSString>> =
        ["Fill", "Fit", "Stretch", "Center", "Custom"].iter().map(|text| NSString::from_str(text)).collect();
    let modes = unsafe {
        NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
            &NSArray::from_retained_slice(&labels),
            NSSegmentSwitchTracking::SelectOne,
            Some(target),
            Some(sel!(noop:)),
            mtm,
        )
    };
    modes.setSelectedSegment(0);
    modes.setSelectedSegmentBezelColor(Some(&theme.fill));
    modes.setControlSize(NSControlSize::Regular);
    let modes_row = widgets::stack(mtm, &[&modes], false, 0.0);
    modes_row.setEdgeInsets(widgets::insets(10.0, 12.0, 6.0, 12.0));
    modes.leadingAnchor().constraintEqualToAnchor_constant(&modes_row.leadingAnchor(), 12.0).setActive(true);
    modes.trailingAnchor().constraintEqualToAnchor_constant(&modes_row.trailingAnchor(), -12.0).setActive(true);
    let horizontal = widgets::row(mtm, "Horizontal", &slider(mtm, 0.0, -1.0, 1.0, theme, target));
    let vertical = widgets::row(mtm, "Vertical", &slider(mtm, 0.2, -1.0, 1.0, theme, target));
    let zoom = widgets::row(mtm, "Zoom", &slider(mtm, 1.0, 1.0, 4.0, theme, target));
    return widgets::view(&widgets::group(mtm, Some("Placement"), &[&modes_row, &horizontal, &vertical, &zoom]));
}

fn organize(mtm: MainThreadMarker, library: &Library, folder: Option<usize>, target: &AnyObject) -> Retained<NSView> {
    let popup =
        NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), objc2_core_foundation::CGRect::ZERO, false);
    let mut titles: Vec<Retained<NSString>> = vec![NSString::from_str("None")];
    titles.extend(library.folders.iter().map(|name| NSString::from_str(name)));
    popup.addItemsWithTitles(&NSArray::from_retained_slice(&titles));
    let selected = folder.map_or(0, |index| index + 1);
    popup.selectItemAtIndex(isize::try_from(selected).unwrap_or(0));
    unsafe {
        popup.setTarget(Some(target));
        popup.setAction(Some(sel!(noop:)));
    }
    popup.setControlSize(NSControlSize::Small);
    popup.widthAnchor().constraintLessThanOrEqualToConstant(160.0).setActive(true);
    let row = widgets::row(mtm, "Folder", &popup);
    return widgets::view(&widgets::group(mtm, Some("Organize"), &[&row]));
}

fn actions(mtm: MainThreadMarker, library: &Library, theme: &Theme, target: &AnyObject) -> Retained<NSView> {
    let wallpaper = text_button(mtm, "Set as Wallpaper", Some("photo.on.rectangle"), target);
    wallpaper.setBezelColor(Some(&theme.fill));
    wallpaper.setContentTintColor(Some(&theme.accent_text));
    wallpaper.setKeyEquivalent(&NSString::from_str("\r"));
    let lock = text_button(mtm, "Set as Lock Screen", Some("lock"), target);
    let scope = match library.displays.first() {
        Some(display) => format!("Applies to {} · Space 1", display.name),
        None => "Applies to all displays".to_string(),
    };
    let hint = widgets::label(mtm, &scope, 11.0, &Weight::Regular, &NSColor::tertiaryLabelColor());
    let delete = text_button(mtm, "Delete from Library", Some("trash"), target);
    delete.setHasDestructiveAction(true);
    delete.setControlSize(NSControlSize::Regular);
    delete.setContentTintColor(Some(&NSColor::systemRedColor()));
    let stack = widgets::stack(mtm, &[&wallpaper, &lock, &hint, &delete], true, 8.0);
    stack.setAlignment(NSLayoutAttribute::CenterX);
    stack.setCustomSpacing_afterView(20.0, &hint);
    for view in [&wallpaper, &lock] {
        view.leadingAnchor().constraintEqualToAnchor(&stack.leadingAnchor()).setActive(true);
        view.trailingAnchor().constraintEqualToAnchor(&stack.trailingAnchor()).setActive(true);
    }
    return widgets::view(&stack);
}

fn shown_chip(mtm: MainThreadMarker, text: &str, icon: &str, theme: &Theme) -> Retained<NSView> {
    let glyph = widgets::symbol_view(mtm, icon, 10.0, &theme.accent);
    let label =
        widgets::label(mtm, if text == "Lock" { "Lock Screen" } else { text }, 11.0, &Weight::Semibold, &theme.accent);
    let content = widgets::stack(mtm, &[&glyph, &label], false, 4.0);
    content.setEdgeInsets(widgets::insets(3.0, 8.0, 3.0, 8.0));
    content.setWantsLayer(true);
    if let Some(layer) = content.layer() {
        layer.setCornerRadius(10.0);
        layer.setBackgroundColor(Some(&widgets::cg(mtm, &theme.accent.colorWithAlphaComponent(0.14))));
    }
    return widgets::view(&content);
}

fn slider(
    mtm: MainThreadMarker,
    value: f64,
    min: f64,
    max: f64,
    theme: &Theme,
    target: &AnyObject,
) -> Retained<NSSlider> {
    let slider = unsafe {
        NSSlider::sliderWithValue_minValue_maxValue_target_action(value, min, max, Some(target), Some(sel!(noop:)), mtm)
    };
    slider.setTrackFillColor(Some(&theme.accent));
    slider.setControlSize(NSControlSize::Small);
    slider.widthAnchor().constraintEqualToConstant(150.0).setActive(true);
    return slider;
}

fn icon_button(mtm: MainThreadMarker, name: &str, target: &AnyObject) -> Retained<NSButton> {
    let image = widgets::symbol(name, 13.0).unwrap_or_default();
    let button = unsafe { NSButton::buttonWithImage_target_action(&image, Some(target), Some(sel!(noop:)), mtm) };
    button.setBordered(false);
    button.setContentTintColor(Some(&NSColor::secondaryLabelColor()));
    return button;
}

fn text_button(mtm: MainThreadMarker, title: &str, icon: Option<&str>, target: &AnyObject) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(&NSString::from_str(title), Some(target), Some(sel!(noop:)), mtm)
    };
    button.setControlSize(NSControlSize::Large);
    if let Some(image) = icon.and_then(|name| widgets::symbol(name, 13.0)) {
        button.setImage(Some(&image));
        button.setImagePosition(objc2_app_kit::NSCellImagePosition::ImageLeading);
    }
    return button;
}
