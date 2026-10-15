use objc2::rc::Retained;
use objc2_core_foundation::{CFRetained, CFURL};
use objc2_core_graphics::CGImage;
use objc2_image_io::CGImageSource;
use objc2_quartz_core::{CALayer, kCAGravityResize};

use crate::platform::spaces::SurfaceKey;

pub struct ImageNode {
    pub size: (f64, f64),
    image: CFRetained<CGImage>,
    layers: Vec<(SurfaceKey, Retained<CALayer>)>,
}

impl ImageNode {
    pub fn new(path: &str) -> Option<Self> {
        let url = CFURL::from_file_path(path)?;
        let source = unsafe { CGImageSource::with_url(&url, None) }?;
        let image = unsafe { source.image_at_index(0, None) }?;
        let width = u32::try_from(CGImage::width(Some(&image))).map_or(0.0, f64::from);
        let height = u32::try_from(CGImage::height(Some(&image))).map_or(0.0, f64::from);
        return Some(Self { size: (width, height), image, layers: Vec::new() });
    }

    pub fn attach(&mut self, key: &SurfaceKey, root: &CALayer) {
        let layer = CALayer::layer();
        set_image(&layer, &self.image);
        layer.setContentsGravity(unsafe { kCAGravityResize });
        root.addSublayer(&layer);
        self.layers.push((key.clone(), layer));
    }

    pub fn detach(&mut self, key: &SurfaceKey) {
        if let Some(index) = self.layers.iter().position(|(existing, _)| existing == key) {
            let (_, layer) = self.layers.remove(index);
            layer.removeFromSuperlayer();
        }
    }

    pub fn set_frame(&self, key: &SurfaceKey, frame: objc2_core_foundation::CGRect) {
        for (existing, layer) in &self.layers {
            if existing == key {
                layer.setFrame(frame);
            }
        }
    }

    pub fn keys(&self) -> Vec<SurfaceKey> {
        return self.layers.iter().map(|(key, _)| key.clone()).collect();
    }

    pub fn layer_count(&self) -> usize {
        return self.layers.len();
    }
}

pub fn set_image(layer: &CALayer, image: &CFRetained<CGImage>) {
    let pointer = CFRetained::as_ptr(image).as_ptr().cast::<objc2::runtime::AnyObject>();
    let object = unsafe { &*pointer };
    unsafe { layer.setContents(Some(object)) };
}
