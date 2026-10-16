use objc2_core_foundation::CFRetained;
use objc2_core_graphics::CGImage;
use objc2_image_io::CGImageSource;
use objc2_quartz_core::{CALayer, kCAGravityResize};

use crate::cf::cf_file_url;
use crate::engine::layer::{LayerSet, set_image};
use crate::geometry::Size;
use crate::platform::spaces::SurfaceKey;

pub struct ImageNode {
    pub size: Size,
    pub layers: LayerSet,
    image: CFRetained<CGImage>,
}

impl ImageNode {
    pub fn new(path: &str) -> Option<Self> {
        let url = cf_file_url(path)?;
        let source = unsafe { CGImageSource::with_url(&url, None) }?;
        let image = unsafe { source.image_at_index(0, None) }?;
        let width = u32::try_from(CGImage::width(Some(&image))).map_or(0.0, f64::from);
        let height = u32::try_from(CGImage::height(Some(&image))).map_or(0.0, f64::from);
        return Some(Self { size: Size::new(width, height), layers: LayerSet::default(), image });
    }

    pub fn attach(&mut self, key: &SurfaceKey, root: &CALayer) {
        let layer = CALayer::layer();
        layer.setContentsGravity(unsafe { kCAGravityResize });
        set_image(&layer, &self.image);
        self.layers.add(key, layer, root);
    }
}
