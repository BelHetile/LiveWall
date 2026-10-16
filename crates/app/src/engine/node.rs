use objc2_quartz_core::CALayer;

use crate::engine::gif::GifNode;
use crate::engine::image::ImageNode;
use crate::engine::layer::{LayerSet, without_actions};
use crate::engine::video::VideoNode;
use crate::geometry::Size;
use crate::model::ItemKind;
use crate::platform::spaces::SurfaceKey;

pub enum Node {
    Video(VideoNode),
    Gif(GifNode),
    Image(ImageNode),
}

impl Node {
    pub fn kind(&self) -> ItemKind {
        return match self {
            Self::Video(_) => ItemKind::Video,
            Self::Gif(_) => ItemKind::Gif,
            Self::Image(_) => ItemKind::Image,
        };
    }

    pub fn attach(&mut self, key: &SurfaceKey, root: &CALayer) {
        without_actions(|| match self {
            Self::Video(node) => node.attach(key, root),
            Self::Gif(node) => node.attach(key, root),
            Self::Image(node) => node.attach(key, root),
        });
    }

    pub fn layers(&self) -> &LayerSet {
        return match self {
            Self::Video(node) => &node.layers,
            Self::Gif(node) => &node.layers,
            Self::Image(node) => &node.layers,
        };
    }

    pub fn layers_mut(&mut self) -> &mut LayerSet {
        return match self {
            Self::Video(node) => &mut node.layers,
            Self::Gif(node) => &mut node.layers,
            Self::Image(node) => &mut node.layers,
        };
    }

    pub fn size(&self) -> Size {
        return match self {
            Self::Video(node) => node.size,
            Self::Gif(node) => node.size,
            Self::Image(node) => node.size,
        };
    }

    pub fn video(&self) -> Option<&VideoNode> {
        return match self {
            Self::Video(node) => Some(node),
            _ => None,
        };
    }

    pub fn video_mut(&mut self) -> Option<&mut VideoNode> {
        return match self {
            Self::Video(node) => Some(node),
            _ => None,
        };
    }

    pub fn gif_mut(&mut self) -> Option<&mut GifNode> {
        return match self {
            Self::Gif(node) => Some(node),
            _ => None,
        };
    }
}
