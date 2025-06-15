use std::fs;
use std::panic::catch_unwind;
use std::path::PathBuf;

use eframe::Storage;
use egui::{CentralPanel, Color32, Id, ScrollArea, SidePanel, TopBottomPanel, Ui, WidgetText};
use egui_file::FileDialog;
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, EnumIter, IntoEnumIterator};
use syn::__private::quote::__private::Span;
use syn::Ident;
use wire_weaver_core::ast::{Item, Source};
use wire_weaver_core::method_model::{MethodModel, MethodModelKind};
use wire_weaver_core::property_model::{PropertyModel, PropertyModelKind};
use wire_weaver_core::transform::Transform;

use crate::context::Context;
use crate::tab::TabUi;
use crate::tab_kind::TabKind;
use crate::util::format_rust;

pub struct DebugView {
    id: usize,
    state: State,
    file_dialog: Option<FileDialog>,
    transient: TransientState,
}

#[derive(Default)]
struct TransientState {
    source: String,
    syn_str: Option<String>,
    ast: Option<wire_weaver_core::ast::Context>,
    ast_str: Option<String>,
    def_code: Option<String>,
    serdes_code: Option<String>,
    api_code: Option<String>,
    messages: Vec<Message>,
}

#[derive(Default, Serialize, Deserialize)]
struct State {
    path: Option<PathBuf>,
    active_view: View,
    no_alloc: bool,
    use_async: bool,
    method_model: String,
    property_model: String,
    is_shrink_wrap_attr_macro: bool,
}

#[derive(Default, Eq, PartialEq, Copy, Clone, Serialize, Deserialize, AsRefStr, EnumIter)]
enum View {
    Source,
    Syn,
    Ast,
    #[default]
    Def,
    SerDes,
    Api,
}

impl DebugView {
    pub fn new_with_id(id: usize) -> Self {
        DebugView {
            id,
            state: State::default(),
            file_dialog: None,
            transient: TransientState::default(),
        }
    }
}

#[derive(Debug)]
enum Message {
    FsError(String),
    FileLoaded,
    SynError(String),
    FileParsed,
    // TODO: refactor
    Transform(Source, wire_weaver_core::transform::Message),
    Info(String),
    Panicked(String),
}

impl TabUi for DebugView {
    fn kind(&self) -> TabKind {
        TabKind::DebugView
    }

    fn title(&self) -> WidgetText {
        "DebugView".into()
    }

    fn ui(&mut self, ui: &mut Ui, _cx: &mut Context) {
        TopBottomPanel::top(Id::new("top_panel").with(self.id)).show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                for view in View::iter() {
                    if ui
                        .selectable_label(self.state.active_view == view, view.as_ref())
                        .clicked()
                    {
                        self.state.active_view = view;
                    }
                }
            });
            ui.add_space(4.0);
        });
        TopBottomPanel::bottom(Id::new("bottom_panel").with(self.id))
            .resizable(true)
            .min_height(25.0)
            .max_height(300.0)
            .show_inside(ui, |ui| {
                self.show_messages(ui);
            });
        SidePanel::left(Id::new("left_panel").with(self.id)).show_inside(ui, |ui| {
            if ui.button("Load").clicked() {
                // let filter = Box::new({
                //     let ext = Some(OsStr::new("ww"));
                //     move |path: &Path| ->  bool { path.extension() == ext }
                // });
                let mut dialog = FileDialog::open_file(self.state.path.clone());
                dialog.open();
                self.file_dialog = Some(dialog);
            }

            if ui.button("Reload").clicked() {
                self.reload();
            }

            if ui.checkbox(&mut self.state.no_alloc, "no_alloc").changed() {
                self.generate_code();
            }

            if ui
                .checkbox(&mut self.state.use_async, "use_async")
                .changed()
            {
                self.generate_code();
            }

            if ui
                .checkbox(
                    &mut self.state.is_shrink_wrap_attr_macro,
                    "shrink_wrap_attr_macro",
                )
                .changed()
            {
                self.generate_code();
            }

            ui.label("Method model:");
            if ui
                .text_edit_singleline(&mut self.state.method_model)
                .lost_focus()
            {
                self.generate_code();
            }

            ui.label("Property model:");
            if ui
                .text_edit_singleline(&mut self.state.property_model)
                .lost_focus()
            {
                self.generate_code();
            }
        });
        CentralPanel::default().show_inside(ui, |ui| {
            if let Some(dialog) = &mut self.file_dialog {
                if dialog.show(ui.ctx()).selected() {
                    if let Some(path) = dialog.path() {
                        self.state.path = Some(path.to_path_buf());
                        self.reload();
                    }
                }
            }

            match self.state.active_view {
                View::Source => {
                    Self::source_code_edit(&mut self.transient.source, "rs", ui);

                    ui.add_space(4.0);
                    if ui.button("Reparse").clicked() {
                        self.transient.messages.clear();
                        self.parse();
                        self.generate_code();
                    }
                }
                View::Syn => {
                    if let Some(syn) = &self.transient.syn_str {
                        Self::source_code_view(syn.as_str(), "rb", ui);
                    }
                }
                View::Ast => {
                    if let Some(ast) = &self.transient.ast_str {
                        Self::source_code_view(ast.as_str(), "rb", ui);
                    }
                }
                View::Def => {
                    if let Some(code) = &self.transient.def_code {
                        Self::source_code_view(code, "rs", ui);
                    }
                }
                View::SerDes => {
                    if let Some(code) = &self.transient.serdes_code {
                        Self::source_code_view(code, "rs", ui);
                    }
                }
                View::Api => {
                    if let Some(code) = &self.transient.api_code {
                        Self::source_code_view(code, "rs", ui);
                    }
                }
            }
        });
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

impl DebugView {
    fn reload(&mut self) {
        self.transient = TransientState::default();
        let Some(path) = &self.state.path else { return };
        let source = match fs::read_to_string(path) {
            Ok(path) => path,
            Err(error) => {
                self.transient
                    .messages
                    .push(Message::FsError(format!("{error}")));
                return;
            }
        };
        self.transient.messages.push(Message::FileLoaded);
        self.transient.source = source;

        self.parse();
        self.generate_code();
    }

    fn parse(&mut self) {
        let ast = match syn::parse_file(self.transient.source.as_str()) {
            Ok(ast) => ast,
            Err(error) => {
                self.transient
                    .messages
                    .push(Message::SynError(format!("{error}")));
                return;
            }
        };
        self.transient.syn_str = Some(format!("{ast:#?}"));
        self.transient.messages.push(Message::FileParsed);

        let ww_cx = catch_unwind(|| {
            let mut transform = Transform::new();
            let path = self
                .state
                .path
                .as_ref()
                .map(|p| p.to_str().map(|s| s.to_string()))
                .flatten()
                .unwrap_or("editor".to_string());
            transform.push_file(Source::File { path }, ast);
            let ww_cx = transform.transform(&[], self.state.is_shrink_wrap_attr_macro);
            (transform, ww_cx)
        });
        let (transform, ww_cx) = match ww_cx {
            Ok((transform, ww_cx)) => (transform, ww_cx),
            Err(e) => {
                self.transient
                    .messages
                    .push(Message::Panicked(format!("{e:?}")));
                return;
            }
        };
        for (source, messages) in transform.messages() {
            for message in messages.messages() {
                self.transient
                    .messages
                    .push(Message::Transform(source.clone(), message.clone()));
            }
        }
        self.transient.ast_str = ww_cx.as_ref().map(|ww_cx| format!("{ww_cx:#?}"));
        self.transient.ast = ww_cx;
        self.transient
            .messages
            .push(Message::Info("AST transform done".into()));
    }

    fn generate_code(&mut self) {
        let Some(ww_cx) = &self.transient.ast else {
            return;
        };

        let mut def_code = String::new();
        let mut serdes_code = String::new();
        let no_alloc = self.state.no_alloc;
        for module in &ww_cx.modules {
            for item in &module.items {
                match item {
                    Item::Struct(item_struct) => {
                        let code = catch_unwind(|| {
                            wire_weaver_core::codegen::item_struct::struct_def(
                                item_struct,
                                no_alloc,
                            )
                        });
                        let code = match code {
                            Ok(code) => code,
                            Err(e) => {
                                self.transient
                                    .messages
                                    .push(Message::Panicked(format!("{e:?}")));
                                return;
                            }
                        };
                        def_code += format!("// {}\n", item_struct.ident.sym).as_str();
                        def_code += format!("{code}").as_str();
                        def_code += "\n\n";

                        let code = catch_unwind(|| {
                            wire_weaver_core::codegen::item_struct::struct_serdes(
                                item_struct,
                                no_alloc,
                            )
                        });
                        let code = match code {
                            Ok(code) => code,
                            Err(e) => {
                                self.transient
                                    .messages
                                    .push(Message::Panicked(format!("{e:?}")));
                                return;
                            }
                        };
                        serdes_code += format!("// {}\n", item_struct.ident.sym).as_str();
                        serdes_code += format!("{code}").as_str();
                        serdes_code += "\n\n";
                    }
                    Item::Enum(item_enum) => {
                        let code = catch_unwind(|| {
                            wire_weaver_core::codegen::item_enum::enum_def(item_enum, no_alloc)
                        });
                        let code = match code {
                            Ok(code) => code,
                            Err(e) => {
                                self.transient
                                    .messages
                                    .push(Message::Panicked(format!("{e:?}")));
                                return;
                            }
                        };
                        def_code += format!("// {}\n", item_enum.ident.sym).as_str();
                        def_code += format!("{code}").as_str();
                        def_code += "\n\n";

                        let code = catch_unwind(|| {
                            wire_weaver_core::codegen::item_enum::enum_serdes(item_enum, no_alloc)
                        });
                        let code = match code {
                            Ok(code) => code,
                            Err(e) => {
                                self.transient
                                    .messages
                                    .push(Message::Panicked(format!("{e:?}")));
                                return;
                            }
                        };
                        serdes_code += format!("// {}\n", item_enum.ident.sym).as_str();
                        serdes_code += format!("{code}").as_str();
                        serdes_code += "\n\n";
                    }
                    Item::Const(item_const) => {
                        let code = catch_unwind(|| {
                            wire_weaver_core::codegen::item_const::const_def(item_const)
                        });
                        let code = match code {
                            Ok(code) => code,
                            Err(e) => {
                                self.transient
                                    .messages
                                    .push(Message::Panicked(format!("{e:?}")));
                                return;
                            }
                        };
                        def_code += format!("{code}").as_str();
                        def_code += "\n\n";
                    }
                }
            }
        }
        let def_code = format_rust(def_code);
        self.transient.def_code = Some(def_code);
        let serdes_code = format_rust(serdes_code);
        self.transient.serdes_code = Some(serdes_code);
        self.transient
            .messages
            .push(Message::Info("Code generation done".into()));

        self.generate_api_code();
    }

    fn generate_api_code(&mut self) {
        let Some(ww_cx) = &self.transient.ast else {
            return;
        };

        let property_model = if self.state.property_model.is_empty() {
            PropertyModel {
                default: Some(PropertyModelKind::GetSet),
                items: vec![],
            }
        } else {
            PropertyModel::parse(&self.state.property_model).unwrap()
        };
        let method_model = if self.state.method_model.is_empty() {
            MethodModel {
                default: Some(MethodModelKind::Immediate),
                items: vec![],
            }
        } else {
            MethodModel::parse(&self.state.method_model).unwrap()
        };

        let mut api_code = String::new();
        for module in &ww_cx.modules {
            for api_level in &module.api_levels {
                // let location = syn::Path::from(Ident::new("api_model_location", Span::call_site()));
                let code = catch_unwind(|| {
                    wire_weaver_core::codegen::api_server::server_dispatcher(
                        api_level,
                        // &Some(location.clone()),
                        self.state.no_alloc,
                        self.state.use_async,
                        &method_model,
                        &property_model,
                    )
                });
                let code = match code {
                    Ok(code) => code,
                    Err(e) => {
                        self.transient
                            .messages
                            .push(Message::Panicked(format!("{e:?}")));
                        return;
                    }
                };
                api_code += format!("// Server side\n{code}\n\n").as_str();
                let code = catch_unwind(|| {
                    wire_weaver_core::codegen::api_client::client(
                        api_level,
                        // &Some(location),
                        self.state.no_alloc,
                        true,
                    )
                });
                let code = match code {
                    Ok(code) => code,
                    Err(e) => {
                        self.transient
                            .messages
                            .push(Message::Panicked(format!("{e:?}")));
                        return;
                    }
                };
                api_code += format!("// Client side\n{code}\n").as_str();
            }
        }
        let api_code = format_rust(api_code);
        self.transient.api_code = Some(api_code);
    }

    fn source_code_edit(source: &mut String, language: &str, ui: &mut Ui) {
        let style = egui::Style::default();
        let theme = egui_extras::syntax_highlighting::CodeTheme::from_memory(ui.ctx(), &style);
        let mut layouter = |ui: &Ui, string: &str, wrap_width: f32| {
            let mut layout_job = egui_extras::syntax_highlighting::highlight(
                ui.ctx(),
                &style,
                &theme,
                string,
                language,
            );
            layout_job.wrap.max_width = wrap_width;
            ui.fonts(|f| f.layout_job(layout_job))
        };
        ScrollArea::vertical()
            .max_height(ui.max_rect().height() - 20.0)
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(source)
                        .font(egui::TextStyle::Monospace) // for cursor height
                        .code_editor()
                        .desired_rows(10)
                        .lock_focus(true)
                        .desired_width(f32::INFINITY)
                        .layouter(&mut layouter),
                );
            });
    }

    fn source_code_view(source: &str, language: &str, ui: &mut Ui) {
        let style = egui::Style::default();
        let theme = egui_extras::syntax_highlighting::CodeTheme::from_memory(ui.ctx(), &style);
        ScrollArea::vertical()
            .max_width(f32::INFINITY)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut layout_job = egui_extras::syntax_highlighting::highlight(
                    ui.ctx(),
                    &style,
                    &theme,
                    source,
                    language,
                );
                layout_job.wrap.max_width = f32::INFINITY;
                ui.add(egui::Label::new(layout_job).selectable(true))
            });
    }

    fn show_messages(&self, ui: &mut Ui) {
        ScrollArea::vertical()
            .max_width(f32::MAX)
            .max_height(ui.max_rect().height())
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for message in &self.transient.messages {
                    let err_color = ui.ctx().style().visuals.error_fg_color;
                    let warn_color = ui.ctx().style().visuals.warn_fg_color;
                    let info_color = Color32::LIGHT_GREEN;
                    match message {
                        Message::FsError(e) => {
                            ui.colored_label(err_color, e.as_str());
                        }
                        Message::FileLoaded => {
                            ui.colored_label(info_color, "File loaded");
                        }
                        Message::SynError(e) => {
                            ui.colored_label(err_color, e.as_str());
                        }
                        Message::FileParsed => {
                            ui.colored_label(info_color, "File parsed");
                        }
                        Message::Transform(source, msg) => match msg {
                            wire_weaver_core::transform::Message::SynConversionWarning(w) => {
                                ui.colored_label(warn_color, format!("{source:?}: {w:?}").as_str());
                            }
                            wire_weaver_core::transform::Message::SynConversionError(e) => {
                                ui.colored_label(err_color, format!("{source:?} {e:?}").as_str());
                            }
                        },
                        Message::Info(i) => {
                            ui.colored_label(info_color, i.as_str());
                        }
                        Message::Panicked(e) => {
                            ui.colored_label(err_color, e);
                        }
                    }
                }
            });
    }
}
