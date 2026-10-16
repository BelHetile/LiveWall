use objc2::rc::Retained;
use objc2_core_foundation::{CFRetained, CGRect};
use objc2_core_graphics::CGImage;
use objc2_quartz_core::{CALayer, CATransaction};

use crate::cf::object;
use crate::platform::spaces::SurfaceKey;

pub fn without_actions<T>(body: impl FnOnce() -> T) -> T {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    let result = body();
    CATransaction::commit();
    return result;
}

pub fn set_image(layer: &CALayer, image: &CFRetained<CGImage>) {
    unsafe { layer.setContents(Some(object(&**image))) };
}

#[derive(Default)]
pub struct LayerSet {
    entries: Vec<(SurfaceKey, Retained<CALayer>)>,
}

impl LayerSet {
    pub fn add(&mut self, key: &SurfaceKey, layer: Retained<CALayer>, root: &CALayer) {
        root.addSublayer(&layer);
        self.entries.push((key.clone(), layer));
    }

    pub fn detach(&mut self, key: &SurfaceKey) {
        let Some(index) = self.entries.iter().position(|(existing, _)| existing == key) else {
            return;
        };
        let (_, layer) = self.entries.remove(index);
        without_actions(|| layer.removeFromSuperlayer());
    }

    pub fn clear(&mut self) {
        without_actions(|| {
            for (_, layer) in self.entries.drain(..) {
                layer.removeFromSuperlayer();
            }
        });
    }

    pub fn set_frame(&self, key: &SurfaceKey, frame: CGRect) {
        without_actions(|| {
            for (_, layer) in self.entries.iter().filter(|(existing, _)| existing == key) {
                layer.setFrame(frame);
            }
        });
    }

    pub fn show(&self, image: &CFRetained<CGImage>) {
        without_actions(|| {
            for (_, layer) in &self.entries {
                set_image(layer, image);
            }
        });
    }

    pub fn layer(&self, key: &SurfaceKey) -> Option<Retained<CALayer>> {
        return self.entries.iter().find(|(existing, _)| existing == key).map(|(_, layer)| layer.clone());
    }

    pub fn iter(&self) -> impl Iterator<Item = &CALayer> {
        return self.entries.iter().map(|(_, layer)| &**layer);
    }

    pub fn keys(&self) -> impl Iterator<Item = &SurfaceKey> {
        return self.entries.iter().map(|(key, _)| key);
    }

    pub fn len(&self) -> usize {
        return self.entries.len();
    }

    pub fn is_empty(&self) -> bool {
        return self.entries.is_empty();
    }
}
