use std::path::Path;

use objc2_core_foundation::{CFDictionary, CFRetained, CFString, CFType, CGAffineTransform};
use objc2_core_graphics::CGImage;
use objc2_image_io::{CGImageSource, kCGImagePropertyOrientation};

use crate::cf::{cf_file_url, cf_i64};
use crate::media::av::{image_size, thumbnail_options};

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
    let (width, height) = image_size(&source);
    if width == 0 || height == 0 {
        return unsafe { source.image_at_index(0, None) };
    }
    let options = thumbnail_options(usize::try_from(width.max(height)).unwrap_or(usize::MAX));
    let options: &CFDictionary = unsafe { (*options).cast_unchecked() };
    return unsafe { source.thumbnail_at_index(0, Some(options)) };
}

#[cfg(test)]
mod tests {
    use objc2_core_foundation::CFNumber;
    use objc2_image_io::CGImageDestination;

    use super::*;

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
    fn only_the_transposing_orientations_swap_axes() {
        assert!((5..=8).all(swaps_axes));
        assert!(!(1..=4).any(swaps_axes));
        assert!(!swaps_axes(0));
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

    const WIDE: [u8; 70] = [
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00,
        0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x7b, 0x40, 0xe8, 0xdd, 0x00, 0x00, 0x00,
        0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0x00, 0x04, 0xff, 0x01, 0x07, 0x00, 0x01, 0xff,
        0xe2, 0x23, 0x9e, 0x59, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    fn oriented(dir: &Path, value: i64) -> std::path::PathBuf {
        let wide = dir.join("wide.png");
        std::fs::write(&wide, WIDE).expect("png");
        let output = dir.join(format!("wide-{value}.png"));
        let source = unsafe { CGImageSource::with_url(&cf_file_url(&wide).expect("url"), None) }.expect("source");
        let kind = CFString::from_static_str("public.png");
        let target = cf_file_url(&output).expect("url");
        let destination = unsafe { CGImageDestination::with_url(&target, &kind, 1, None) }.expect("writer");
        let number = CFNumber::new_i64(value);
        let keys: [&CFString; 1] = [unsafe { kCGImagePropertyOrientation }];
        let values: [&CFType; 1] = [number.as_ref()];
        let properties = CFDictionary::<CFString, CFType>::from_slices(&keys, &values);
        let properties: &CFDictionary = unsafe { (*properties).cast_unchecked() };
        unsafe { destination.add_image_from_source(&source, 0, Some(properties)) };
        assert!(unsafe { destination.finalize() });
        return output;
    }

    fn upright_size(path: &Path) -> (usize, usize) {
        let image = upright_image(path).expect("decodes");
        return (CGImage::width(Some(&image)), CGImage::height(Some(&image)));
    }

    #[test]
    fn images_load_upright_and_report_their_displayed_size() {
        let dir = tempfile::tempdir().expect("tempdir");
        let plain = oriented(dir.path(), 1);
        let turned = oriented(dir.path(), 6);
        assert_eq!(upright_size(&plain), (2, 1));
        assert_eq!(upright_size(&turned), (1, 2));
        let source = unsafe { CGImageSource::with_url(&cf_file_url(&turned).expect("url"), None) }.expect("source");
        assert_eq!(orientation(&source), 6);
        assert_eq!(image_size(&source), (1, 2));
        assert!(upright_image(&dir.path().join("missing.png")).is_none());
    }
}
