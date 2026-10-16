use crate::engine::Engine;
use crate::engine::node::Node;
use crate::engine::window::desktop_icon_level;
use crate::model::{ItemId, ItemKind};

#[derive(serde::Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct WindowReport {
    pub space: String,
    pub number: u32,
    pub level: i64,
    pub frame: [f64; 4],
    pub is_on_active_space: bool,
    pub active: bool,
    pub can_join_all: bool,
    pub item: Option<String>,
    pub rate: f64,
    pub ready: bool,
}

#[derive(serde::Serialize)]
pub struct NodeReport {
    pub item: String,
    pub kind: ItemKind,
    pub layers: usize,
    pub keys: Vec<String>,
}

impl Engine {
    pub fn rate_ready(&self, item: ItemId) -> (f64, bool) {
        return match self.nodes.get(&item).and_then(Node::video) {
            Some(video) => (f64::from(video.rate()), video.is_ready()),
            None => (0.0, false),
        };
    }

    pub fn windows(&self) -> Vec<WindowReport> {
        let level = desktop_icon_level();
        let mut reports = Vec::new();
        for (key, surface) in &self.surfaces {
            let (rate, ready) = match surface.item {
                Some(item) => self.rate_ready(item),
                None => (0.0, false),
            };
            reports.push(WindowReport {
                space: key.space.clone(),
                number: surface.window.window_number(),
                level,
                frame: [
                    surface.frame.origin.x,
                    surface.frame.origin.y,
                    surface.frame.size.width,
                    surface.frame.size.height,
                ],
                is_on_active_space: surface.window.is_on_active_space(),
                active: surface.active,
                can_join_all: surface.spec.can_join_all,
                item: surface.item.map(|item| item.to_string()),
                rate,
                ready,
            });
        }
        reports.sort_by(|a, b| a.space.cmp(&b.space));
        return reports;
    }

    pub fn node_summary(&self) -> Vec<NodeReport> {
        let mut reports = Vec::new();
        for (item, node) in &self.nodes {
            reports.push(NodeReport {
                item: item.to_string(),
                kind: node.kind(),
                layers: node.layers().len(),
                keys: node.layers().keys().map(|key| key.space.clone()).collect(),
            });
        }
        reports.sort_by(|a, b| a.item.cmp(&b.item));
        return reports;
    }
}
