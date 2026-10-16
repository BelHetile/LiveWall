use std::path::{Path, PathBuf};

use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString, CFType, CGAffineTransform};
use objc2_core_graphics::CGImage;
use objc2_image_io::{CGImageDestination, CGImageSource, kCGImagePropertyOrientation};

use crate::cf::{cf_file_url, cf_i64};
use crate::media::av::thumbnail_options;
use crate::model::ItemKind;

const TURNS: [u32; 4] = [1, 6, 3, 8];
const MIRRORED_TURNS: [u32; 4] = [2, 7, 4, 5];

pub fn turn_exif(orientation: u32, turns: u8) -> u32 {
    let step = usize::from(turns % 4);
    for table in [TURNS, MIRRORED_TURNS] {
        if let Some(index) = table.iter().position(|value| *value == orientation) {
            return table[(index + step) % 4];
        }
    }
    return TURNS[step];
}

pub fn swaps_axes(orientation: u32) -> bool {
    return (5..=8).contains(&orientation);
}

pub fn fit(transform: CGAffineTransform, width: f64, height: f64) -> CGAffineTransform {
    let corners = [(0.0, 0.0), (width, 0.0), (0.0, height), (width, height)];
    let xs = corners.map(|(x, y)| transform.a * x + transform.c * y);
    let ys = corners.map(|(x, y)| transform.b * x + transform.d * y);
    let min = |values: [f64; 4]| values.into_iter().fold(f64::INFINITY, f64::min);
    return CGAffineTransform { tx: -min(xs), ty: -min(ys), ..transform };
}

pub fn turn(transform: CGAffineTransform, width: f64, height: f64, turns: u8) -> CGAffineTransform {
    let mut out = transform;
    for _ in 0..turns % 4 {
        out = CGAffineTransform { a: -out.b, b: out.a, c: -out.d, d: out.c, tx: -out.ty, ty: out.tx };
    }
    return fit(out, width, height);
}

pub fn orientation(source: &CGImageSource) -> u32 {
    let Some(properties) = (unsafe { source.properties_at_index(0, None) }) else {
        return 1;
    };
    let properties: &CFDictionary<CFString, CFType> = unsafe { properties.cast_unchecked() };
    return cf_i64(properties.get(unsafe { kCGImagePropertyOrientation }).as_deref())
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| (1..=8).contains(value))
        .unwrap_or(1);
}

pub fn upright_image(path: &Path) -> Option<CFRetained<CGImage>> {
    let url = cf_file_url(path)?;
    let source = unsafe { CGImageSource::with_url(&url, None) }?;
    let (width, height) = crate::media::av::image_size(&source);
    let longest = width.max(height).max(1);
    let options = thumbnail_options(usize::try_from(longest).unwrap_or(usize::MAX));
    let options: &CFDictionary = unsafe { (*options).cast_unchecked() };
    return unsafe { source.thumbnail_at_index(0, Some(options)) };
}

pub fn write_png(input: &Path, output: &Path, max_px: usize) -> Result<(), String> {
    let source_url = cf_file_url(input).ok_or("invalid image path")?;
    let source = unsafe { CGImageSource::with_url(&source_url, None) }.ok_or("cannot open the image")?;
    let options = thumbnail_options(max_px);
    let options: &CFDictionary = unsafe { (*options).cast_unchecked() };
    let image = unsafe { source.thumbnail_at_index(0, Some(options)) }.ok_or("cannot read the image")?;
    let target_url = cf_file_url(output).ok_or("invalid output path")?;
    let kind = CFString::from_static_str("public.png");
    let destination = unsafe { CGImageDestination::with_url(&target_url, &kind, 1, None) }.ok_or("cannot write PNG")?;
    unsafe { destination.add_image(&image, None) };
    if !unsafe { destination.finalize() } {
        let _ = std::fs::remove_file(output);
        return Err("cannot save the icon".to_string());
    }
    return Ok(());
}

pub fn rotate_image(input: &Path, output: &Path, turns: u8) -> Result<(), String> {
    let source_url = cf_file_url(input).ok_or("invalid image path")?;
    let source = unsafe { CGImageSource::with_url(&source_url, None) }.ok_or("cannot open the image")?;
    let kind = unsafe { source.r#type() }.ok_or("unknown image type")?;
    let target_url = cf_file_url(output).ok_or("invalid output path")?;
    let destination =
        unsafe { CGImageDestination::with_url(&target_url, &kind, 1, None) }.ok_or("cannot write this image type")?;
    let value = CFNumber::new_i64(i64::from(turn_exif(orientation(&source), turns)));
    let keys: [&CFString; 1] = [unsafe { kCGImagePropertyOrientation }];
    let values: [&CFType; 1] = [value.as_ref()];
    let properties = CFDictionary::<CFString, CFType>::from_slices(&keys, &values);
    let properties: &CFDictionary = unsafe { (*properties).cast_unchecked() };
    unsafe { destination.add_image_from_source(&source, 0, Some(properties)) };
    if !unsafe { destination.finalize() } {
        let _ = std::fs::remove_file(output);
        return Err("cannot save the rotated image".to_string());
    }
    return Ok(());
}

pub fn rotate_file(input: &Path, kind: ItemKind, turns: u8) -> Result<PathBuf, String> {
    let dir = input.parent().ok_or("the wallpaper has no folder")?;
    let stem = input.file_stem().and_then(|stem| stem.to_str()).ok_or("the wallpaper has no file name")?;
    let extension = input.extension().and_then(|extension| extension.to_str()).unwrap_or_default();
    let (temp, target) = match kind {
        ItemKind::Video => (dir.join(format!("{stem}.rotating.mov")), dir.join(format!("{stem}.mov"))),
        ItemKind::Image => (dir.join(format!("{stem}.rotating.{extension}")), input.to_path_buf()),
        ItemKind::Gif => return Err("GIFs can’t be rotated".to_string()),
    };
    let written = match kind {
        ItemKind::Video => crate::media::apple::reorient_video(input, &temp, turns).map_err(|error| error.to_string()),
        _ => rotate_image(input, &temp, turns),
    };
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    std::fs::rename(&temp, &target).map_err(|error| format!("cannot replace the wallpaper: {error}"))?;
    if target != input {
        let _ = std::fs::remove_file(input);
    }
    return Ok(target);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quarter_turns(transform: CGAffineTransform) -> Option<u8> {
        const ROTATIONS: [[f64; 4]; 4] =
            [[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, -1.0, 0.0], [-1.0, 0.0, 0.0, -1.0], [0.0, -1.0, 1.0, 0.0]];
        let actual = [transform.a, transform.b, transform.c, transform.d];
        return ROTATIONS
            .iter()
            .position(|rotation| rotation.iter().zip(actual).all(|(want, have)| (want - have).abs() < 1e-6))
            .and_then(|index| u8::try_from(index).ok());
    }

    fn transform(a: f64, b: f64, c: f64, d: f64, tx: f64, ty: f64) -> CGAffineTransform {
        return CGAffineTransform { a, b, c, d, tx, ty };
    }

    fn same(left: CGAffineTransform, right: CGAffineTransform) -> bool {
        return [
            left.a - right.a,
            left.b - right.b,
            left.c - right.c,
            left.d - right.d,
            left.tx - right.tx,
            left.ty - right.ty,
        ]
        .iter()
        .all(|delta| delta.abs() < 1e-9);
    }

    #[test]
    fn exif_turns_cycle_through_four_orientations() {
        assert_eq!(turn_exif(1, 1), 6);
        assert_eq!(turn_exif(6, 1), 3);
        assert_eq!(turn_exif(3, 1), 8);
        assert_eq!(turn_exif(8, 1), 1);
        assert_eq!(turn_exif(1, 2), 3);
        assert_eq!(turn_exif(6, 3), 1);
        assert_eq!(turn_exif(1, 4), 1);
        assert_eq!(turn_exif(2, 1), 7);
        assert_eq!(turn_exif(5, 1), 2);
        assert_eq!(turn_exif(0, 1), 6, "unknown orientations count as upright");
        assert!(swaps_axes(6) && swaps_axes(8) && swaps_axes(5) && swaps_axes(7));
        assert!(!swaps_axes(1) && !swaps_axes(3) && !swaps_axes(2));
    }

    #[test]
    fn fit_recomputes_the_translation_for_a_smaller_frame() {
        let source = transform(0.0, 1.0, -1.0, 0.0, 1964.0, 0.0);
        assert!(same(fit(source, 2704.0, 1756.0), transform(0.0, 1.0, -1.0, 0.0, 1756.0, 0.0)));
        assert!(same(
            fit(transform(-1.0, 0.0, 0.0, -1.0, 9.0, 9.0), 4.0, 2.0),
            transform(-1.0, 0.0, 0.0, -1.0, 4.0, 2.0)
        ));
        assert!(same(fit(transform(1.0, 0.0, 0.0, 1.0, 5.0, 5.0), 4.0, 2.0), transform(1.0, 0.0, 0.0, 1.0, 0.0, 0.0)));
    }

    #[test]
    fn turning_composes_quarter_turns() {
        let upright = transform(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);
        let once = turn(upright, 1920.0, 1080.0, 1);
        assert_eq!(quarter_turns(once), Some(1));
        assert!(same(once, transform(0.0, 1.0, -1.0, 0.0, 1080.0, 0.0)));
        assert_eq!(quarter_turns(turn(upright, 1920.0, 1080.0, 2)), Some(2));
        assert_eq!(quarter_turns(turn(upright, 1920.0, 1080.0, 3)), Some(3));
        assert!(same(turn(once, 1920.0, 1080.0, 3), upright));
        assert!(same(turn(upright, 1920.0, 1080.0, 4), upright));
        let furina = transform(0.0, 1.0, -1.0, 0.0, 1964.0, 0.0);
        assert!(same(turn(furina, 3024.0, 1964.0, 3), upright), "a quarter turn back undoes a stray rotation flag");
    }

    #[test]
    fn quarter_turns_rejects_skewed_transforms() {
        assert_eq!(quarter_turns(transform(0.5, 0.0, 0.0, 1.0, 0.0, 0.0)), None);
        assert_eq!(quarter_turns(transform(-1.0, 0.0, 0.0, 1.0, 0.0, 0.0)), None);
    }

    fn png(dir: &Path) -> std::path::PathBuf {
        const BYTES: [u8; 70] = [
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00,
            0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x7b, 0x40, 0xe8, 0xdd, 0x00, 0x00, 0x00,
            0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0x00, 0x04, 0xff, 0x01, 0x07, 0x00, 0x01, 0xff,
            0xe2, 0x23, 0x9e, 0x59, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        let path = dir.join("wide.png");
        std::fs::write(&path, BYTES).expect("png");
        return path;
    }

    fn upright_size(path: &Path) -> (usize, usize) {
        let image = upright_image(path).expect("decodes");
        return (CGImage::width(Some(&image)), CGImage::height(Some(&image)));
    }

    #[test]
    fn rotating_an_image_rewrites_its_orientation_and_upright_loading_follows_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = png(dir.path());
        assert_eq!(upright_size(&input), (2, 1));
        let output = dir.path().join("turned.png");
        rotate_image(&input, &output, 1).expect("rotate");
        let url = cf_file_url(&output).expect("url");
        let source = unsafe { CGImageSource::with_url(&url, None) }.expect("source");
        assert_eq!(orientation(&source), 6);
        assert_eq!(upright_size(&output), (1, 2), "a quarter turn stands the image up");
        let back = dir.path().join("back.png");
        rotate_image(&output, &back, 3).expect("rotate back");
        assert_eq!(upright_size(&back), (2, 1));
    }

    fn wide_png(dir: &Path) -> PathBuf {
        const BYTES: [u8; 116] = [
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00,
            0x00, 0x40, 0x00, 0x00, 0x00, 0x20, 0x08, 0x02, 0x00, 0x00, 0x00, 0x2d, 0xff, 0xe9, 0xd3, 0x00, 0x00, 0x00,
            0x3b, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0xed, 0xcf, 0x31, 0x0d, 0x00, 0x00, 0x0c, 0xc3, 0xb0, 0xc1, 0x19,
            0x7f, 0x3c, 0x05, 0x33, 0x0c, 0x3b, 0xfa, 0x59, 0xca, 0x1d, 0xc9, 0x93, 0xdd, 0x6a, 0xe5, 0x7d, 0x06, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0xe0, 0xdb, 0x01, 0xb8, 0xc4, 0x60, 0x88, 0x09, 0x3f, 0x90, 0x5b, 0x00, 0x00, 0x00, 0x00,
            0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        let path = dir.join("wide-64.png");
        std::fs::write(&path, BYTES).expect("png");
        return path;
    }

    fn clip(dir: &Path) -> PathBuf {
        let frame = wide_png(dir);
        let gif = dir.join("clip.gif");
        let source_url = cf_file_url(&frame).expect("url");
        let source = unsafe { CGImageSource::with_url(&source_url, None) }.expect("source");
        let target_url = cf_file_url(&gif).expect("url");
        let kind = CFString::from_static_str("com.compuserve.gif");
        let destination = unsafe { CGImageDestination::with_url(&target_url, &kind, 2, None) }.expect("gif writer");
        unsafe {
            destination.add_image_from_source(&source, 0, None);
            destination.add_image_from_source(&source, 0, None);
        }
        assert!(unsafe { destination.finalize() });
        let video = dir.join("clip.mp4");
        let plan = crate::media::plan::GifPlan { input: gif, output: video.clone(), max_pixels: 4096 };
        let progress: crate::media::apple::Progress = std::sync::Arc::new(|_| {});
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        crate::media::apple::run(&crate::media::apple::Task::Gif(plan), &progress, &cancel).expect("encode clip");
        return video;
    }

    fn displayed(path: &Path) -> (u32, u32) {
        let probed = crate::media::probe::inspect(path, ItemKind::Video);
        return (probed.width, probed.height);
    }

    #[test]
    fn rotating_a_video_turns_its_picture_without_reencoding() {
        let dir = tempfile::tempdir().expect("tempdir");
        let video = clip(dir.path());
        assert_eq!(displayed(&video), (64, 32));
        let before = std::fs::metadata(&video).expect("clip").len();
        let turned = rotate_file(&video, ItemKind::Video, 1).expect("rotate");
        assert_eq!(turned, dir.path().join("clip.mov"));
        assert!(!video.exists(), "the original is replaced");
        assert_eq!(displayed(&turned), (32, 64));
        let after = std::fs::metadata(&turned).expect("turned").len();
        assert!(after < before * 2 + 4096, "samples are copied, not re-encoded ({before} → {after})");
        let back = rotate_file(&turned, ItemKind::Video, 3).expect("rotate back");
        assert_eq!(back, turned);
        assert_eq!(displayed(&back), (64, 32));
        assert!(!dir.path().join("clip.rotating.mov").exists());
    }

    #[test]
    fn icons_are_written_as_small_upright_pngs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = wide_png(dir.path());
        let icon = dir.path().join("icon.png");
        write_png(&input, &icon, 16).expect("icon");
        assert_eq!(upright_size(&icon), (16, 8));
        assert!(write_png(&dir.path().join("missing.png"), &icon, 16).is_err());
    }

    #[test]
    fn rotate_file_keeps_an_image_in_place_and_refuses_gifs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = png(dir.path());
        let target = rotate_file(&input, ItemKind::Image, 1).expect("rotate");
        assert_eq!(target, input);
        assert_eq!(upright_size(&input), (1, 2));
        assert!(!dir.path().join("wide.rotating.png").exists());
        assert!(rotate_file(&input, ItemKind::Gif, 1).is_err());
        assert!(input.exists(), "a refused rotation leaves the file alone");
    }
}
