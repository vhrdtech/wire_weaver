use crate::context::Context;
use crate::tab::TabUi;
use crate::tab_kind::TabKind;
use eframe::Storage;
use egui::{Ui, WidgetText};
use serde::{Deserialize, Serialize};

pub struct TilesDemo {
    id: usize,
    state: State,
}

#[derive(Serialize, Deserialize)]
struct State {
    tree: egui_tiles::Tree<Pane>,
}

impl Default for State {
    fn default() -> Self {
        State {
            tree: create_tree(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Pane {
    nr: usize,
}

struct TreeBehavior {}

impl egui_tiles::Behavior<Pane> for TreeBehavior {
    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        format!("Pane {}", pane.nr).into()
    }

    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: egui_tiles::TileId,
        pane: &mut Pane,
    ) -> egui_tiles::UiResponse {
        // Give each pane a unique color:
        let color = egui::epaint::Hsva::new(0.103 * pane.nr as f32, 0.5, 0.5, 1.0);
        ui.painter().rect_filled(ui.max_rect(), 0.0, color);

        ui.label(format!("The contents of pane {}.", pane.nr));

        // You can make your pane draggable like so:
        if ui
            .add(egui::Button::new("Drag me!").sense(egui::Sense::drag()))
            .drag_started()
        {
            egui_tiles::UiResponse::DragStarted
        } else {
            egui_tiles::UiResponse::None
        }
    }
}

impl TilesDemo {
    pub fn new_with_id(id: usize) -> Self {
        TilesDemo {
            id,
            state: State::default(),
        }
    }
}

impl TabUi for TilesDemo {
    fn kind(&self) -> TabKind {
        TabKind::Tiles
    }

    fn title(&self) -> WidgetText {
        "TilesDemo".into()
    }

    fn ui(&mut self, ui: &mut Ui, _cx: &mut Context) {
        let mut behavior = TreeBehavior {};
        self.state.tree.ui(&mut behavior, ui);
    }

    fn save_state(&self, storage: &mut dyn Storage) {
        let key = format!("{}_{}", self.kind().as_ref(), self.id);
        eframe::set_value(storage, key.as_str(), &self.state);
    }

    fn load_state(&mut self, storage: &dyn Storage, _cx: &mut Context) {
        let key = format!("{}_{}", self.kind().as_ref(), self.id);
        self.state = eframe::get_value(storage, key.as_str()).unwrap_or_default();
    }
}

fn create_tree() -> egui_tiles::Tree<Pane> {
    let mut next_view_nr = 0;
    let mut gen_pane = || {
        let pane = Pane { nr: next_view_nr };
        next_view_nr += 1;
        pane
    };

    let mut tiles = egui_tiles::Tiles::default();

    let mut tabs = vec![];
    tabs.push({
        let children = (0..7).map(|_| tiles.insert_pane(gen_pane())).collect();
        tiles.insert_horizontal_tile(children)
    });
    tabs.push({
        let cells = (0..11).map(|_| tiles.insert_pane(gen_pane())).collect();
        tiles.insert_grid_tile(cells)
    });
    tabs.push(tiles.insert_pane(gen_pane()));

    let root = tiles.insert_tab_tile(tabs);

    egui_tiles::Tree::new("my_tree", root, tiles)
}
