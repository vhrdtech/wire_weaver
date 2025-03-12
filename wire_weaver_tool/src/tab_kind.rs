use crate::debug_view::DebugView;
use crate::tab::{Tab, TabUi};
use crate::tiles_demo::TilesDemo;
use egui_dock::{NodeIndex, SurfaceIndex};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, EnumIter};

#[derive(Serialize, Deserialize, AsRefStr, EnumIter)]
pub enum TabKind {
    DebugView,
    Tiles,
}

impl TabKind {
    pub fn new(&self, surface: SurfaceIndex, node: NodeIndex) -> Tab {
        let id = node.0;
        let kind: Box<dyn TabUi> = match self {
            TabKind::DebugView => Box::new(DebugView::new_with_id(id)),
            TabKind::Tiles => Box::new(TilesDemo::new_with_id(id)),
        };
        Tab {
            kind,
            surface,
            node,
        }
    }
}
