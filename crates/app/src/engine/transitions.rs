use objc2::rc::Retained;
use objc2_foundation::NSTimer;
use objc2_quartz_core::CALayer;

use crate::app::Event;
use crate::engine::layer::without_actions;
use crate::engine::{Engine, transition};
use crate::model::ItemId;
use crate::platform::spaces::SurfaceKey;
use crate::platform::{cancel_timer, schedule_timer};

pub struct ActiveTransition {
    pub old: ItemId,
    layer: Retained<CALayer>,
    timer: Retained<NSTimer>,
}

impl Engine {
    pub fn on_transition_done(&mut self, key: &SurfaceKey) {
        self.finish_transition(key);
    }

    pub(super) fn begin_transitions(&mut self, spec: Option<&transition::ResolvedTransition>) {
        let pending = std::mem::take(&mut self.pending_starts);
        if pending.is_empty() {
            return;
        }
        let Some(spec) = spec.filter(|spec| !spec.instant()) else {
            for (key, old) in &pending {
                self.detach(*old, key);
            }
            self.prune_orphans();
            return;
        };
        for (key, old) in pending {
            let Some(new) = self.surfaces.get(&key).and_then(|surface| surface.item) else { continue };
            let Some(layer) = self.nodes.get(&new).and_then(|node| node.layers().layer(&key)) else {
                self.detach(old, &key);
                continue;
            };
            transition::apply(&layer, spec);
            let timer = schedule_timer(spec.timer_seconds(), false, {
                let key = key.clone();
                move || Event::TransitionDone { key: key.clone() }
            });
            self.transitions.insert(key, ActiveTransition { old, layer, timer });
        }
        self.prune_orphans();
    }

    pub(super) fn finish_transition(&mut self, key: &SurfaceKey) {
        let Some(active) = self.transitions.remove(key) else { return };
        cancel_timer(Some(active.timer));
        self.detach(active.old, key);
        without_actions(|| unsafe { active.layer.setMask(None) });
        self.prune_orphans();
    }

    pub(super) fn finish_transitions_matching(&mut self, matches: impl Fn(&ActiveTransition) -> bool) {
        let fading: Vec<SurfaceKey> =
            self.transitions.iter().filter(|(_, active)| matches(active)).map(|(key, _)| key.clone()).collect();
        for key in fading {
            self.finish_transition(&key);
        }
    }
}
