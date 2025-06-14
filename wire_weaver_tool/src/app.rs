use egui::TopBottomPanel;
use egui_dock::{DockArea, DockState, NodeIndex, Style, SurfaceIndex};
use serde::{Deserialize, Serialize};

use crate::context::Context;
use crate::tab::{MxTabViewer, Tab};
use crate::tab_kind::TabKind;

pub struct WireWeaverToolApp {
    cx: Context,
    state: State,
}

#[derive(Serialize, Deserialize)]
struct State {
    dock_state: DockState<Tab>,
    tab_counter: usize,
}

impl State {
    fn default_with_cx(_cx: &Context) -> State {
        let tabs = vec![TabKind::DebugView.new(SurfaceIndex::main(), NodeIndex(1))];
        let tab_counter = tabs.len() + 1;
        let dock_state = DockState::new(tabs);

        State {
            dock_state,
            tab_counter,
        }
    }
}

impl WireWeaverToolApp {
    pub fn new(cc: &eframe::CreationContext<'_>, mut cx: Context) -> Self {
        // Load previous app state (if any).
        let mut state = if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY).unwrap_or(State::default_with_cx(&cx))
        } else {
            State::default_with_cx(&cx)
        };

        if let Some(storage) = cc.storage {
            // Restore context for all the tabs
            for (_, tab) in state.dock_state.iter_all_tabs_mut() {
                tab.load_state(storage, &mut cx);
            }
        }

        WireWeaverToolApp { cx, state }
    }
}

impl eframe::App for WireWeaverToolApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut added_nodes = Vec::new();

        TopBottomPanel::top("top_panel").show(ctx, |ui| {
            if ui.button("+").clicked() {
                added_nodes.push(
                    TabKind::DebugView.new(SurfaceIndex::main(), NodeIndex(self.state.tab_counter)),
                );
                self.state.tab_counter += 1;
            }
        });

        DockArea::new(&mut self.state.dock_state)
            .style(egui_dock::Style::from_egui(ctx.style().as_ref()))
            .show_close_buttons(true)
            .show_add_buttons(true)
            .show_add_popup(true)
            .draggable_tabs(true)
            // .show_tab_name_on_hover(true)
            .show_leaf_close_all_buttons(true)
            .show_leaf_collapse_buttons(true)
            .style({
                let mut style = Style::from_egui(ctx.style().as_ref());
                style.tab_bar.fill_tab_bar = true;
                style
            })
            .show(
                ctx,
                &mut MxTabViewer {
                    added_nodes: &mut added_nodes,
                    cx: &mut self.cx,
                },
            );

        added_nodes.drain(..).for_each(|node| {
            self.state
                .dock_state
                .set_focused_node_and_surface((node.surface, node.node));
            self.state.dock_state.push_to_focused_leaf(Tab {
                kind: node.kind,
                surface: node.surface,
                node: NodeIndex(self.state.tab_counter),
            });
            self.state.tab_counter += 1;
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.state);
        for (_, tab) in self.state.dock_state.iter_all_tabs_mut() {
            tab.save_state(storage);
        }
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }
}
