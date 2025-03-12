use crate::context::Context;
use crate::tab_kind::TabKind;
use eframe::Storage;
use egui::{Id, Ui, WidgetText};
use egui_dock::{NodeIndex, SurfaceIndex, TabViewer};
use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::Formatter;
use strum::IntoEnumIterator;

pub struct Tab {
    pub kind: Box<dyn TabUi>,
    pub surface: SurfaceIndex,
    pub node: NodeIndex,
}

pub trait TabUi {
    fn kind(&self) -> TabKind;
    fn title(&self) -> WidgetText;
    fn ui(&mut self, ui: &mut Ui, cx: &mut Context);
    fn save_state(&self, storage: &mut dyn Storage);
    fn load_state(&mut self, storage: &dyn Storage, cx: &mut Context);
}

pub trait TabCommon {
    fn new_with_id(id: usize) -> Self;
    fn id(&self) -> Id;
}

impl Serialize for Tab {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_map(Some(4))?;
        seq.serialize_entry("kind", &self.kind.kind())?;
        seq.serialize_entry("surface", &self.surface)?;
        seq.serialize_entry("node", &self.node)?;
        seq.end()
    }
}

impl<'de> Deserialize<'de> for Tab {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(TabVisitor)
    }
}

struct TabVisitor;

impl<'de> Visitor<'de> for TabVisitor {
    type Value = Tab;

    fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "a map with Tab fields as keys")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut kind: Option<TabKind> = None;
        let mut surface = None;
        let mut node: Option<NodeIndex> = None;

        while let Some(k) = map.next_key::<&str>()? {
            match k {
                "kind" => {
                    kind = Some(map.next_value()?);
                }
                "surface" => {
                    surface = Some(map.next_value()?);
                }
                "node" => {
                    node = Some(map.next_value()?);
                }
                _ => {
                    return Err(serde::de::Error::custom("Invalid Tab field"));
                }
            }
        }

        let (Some(kind), Some(surface), Some(node)) = (kind, surface, node) else {
            return Err(serde::de::Error::custom("Missing Tab fields"));
        };

        Ok(kind.new(surface, node))
    }
}

impl Tab {
    pub fn load_state(&mut self, storage: &dyn Storage, cx: &mut Context) {
        self.kind.load_state(storage, cx);
    }

    pub fn save_state(&self, storage: &mut dyn Storage) {
        self.kind.save_state(storage);
    }
}

pub struct MxTabViewer<'a, 'b> {
    pub added_nodes: &'a mut Vec<Tab>,
    pub cx: &'b mut Context,
}

impl<'a, 'b> TabViewer for MxTabViewer<'a, 'b> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.kind.title()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        tab.kind.ui(ui, self.cx);
    }

    fn id(&mut self, tab: &mut Self::Tab) -> Id {
        Id::new("tab").with(tab.node.0)
    }

    fn add_popup(&mut self, ui: &mut Ui, surface: SurfaceIndex, node: NodeIndex) {
        ui.set_min_width(120.);
        ui.style_mut().visuals.button_frame = false;

        for tab_kind in TabKind::iter() {
            if ui.button(tab_kind.as_ref()).clicked() {
                self.added_nodes.push(tab_kind.new(surface, node));
            }
        }
    }
}
