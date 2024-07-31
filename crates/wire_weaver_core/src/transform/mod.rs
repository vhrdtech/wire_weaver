use std::collections::{HashMap, VecDeque};

use crate::ast::{Context, ItemEnum, ItemStruct, Module, Source, Version};
use crate::transform::collect_and_convert::CollectAndConvertPass;
use crate::transform::syn_util::collect_docs_attrs;

mod collect_and_convert;
mod syn_util;
// mod visit_user_types;
// TODO: check that no fields and no variants have the same name
// TODO: check that variants fit within chosen repr

#[derive(Debug, Clone)]
pub enum SynConversionWarning {
    UnknownAttribute(String),
    UnknownFileItem,
}

#[derive(Debug, Clone)]
pub enum SynConversionError {
    UnknownType(String),
    UnsupportedType(String),
    WrongDefaultAttr(String),
    WrongDiscriminant,
    WrongReprAttr(String),
    FlagTypeIsNotBool,
    RecursionLimitReached,
}

#[derive(Default)]
pub struct Messages {
    messages: Vec<Message>,
}

#[derive(Debug, Clone)]
pub enum Message {
    SynConversionWarning(SynConversionWarning),
    SynConversionError(SynConversionError),
}

impl Messages {
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn error_count(&self) -> usize {
        let mut error_count = 0;
        for msg in &self.messages {
            if matches!(msg, Message::SynConversionError(_)) {
                error_count += 1;
            }
        }
        error_count
    }

    fn push_conversion_warning(&mut self, warning: SynConversionWarning) {
        self.messages.push(Message::SynConversionWarning(warning))
    }

    fn push_conversion_error(&mut self, error: SynConversionError) {
        self.messages.push(Message::SynConversionError(error))
    }
}

pub fn dependencies(_ast: syn::File) -> Vec<Source> {
    todo!()
}

#[derive(Default)]
pub struct Transform {
    files: Vec<SynFile>,
    messages: HashMap<Source, Messages>,
}

pub(crate) struct SynFile {
    source: Source,
    _shebang: Option<String>,
    _attrs: Vec<syn::Attribute>,
    items: VecDeque<SynItemWithContext>,
}

enum SynItemWithContext {
    Enum {
        item_enum: syn::ItemEnum,
        transformed: Option<ItemEnum>,
        is_lifetime: Option<bool>,
    },
    Struct {
        item_struct: syn::ItemStruct,
        transformed: Option<ItemStruct>,
        is_lifetime: Option<bool>,
    },
}

impl SynItemWithContext {
    pub fn ident(&self) -> syn::Ident {
        match self {
            SynItemWithContext::Enum { item_enum, .. } => item_enum.ident.clone(),
            SynItemWithContext::Struct { item_struct, .. } => item_struct.ident.clone(),
        }
    }
}

impl Transform {
    pub fn new() -> Self {
        Transform::default()
    }

    pub fn push_file(&mut self, source: Source, syn_file: syn::File) {
        let mut items = VecDeque::new();
        for item in syn_file.items {
            match item {
                syn::Item::Struct(item_struct) => {
                    items.push_back(SynItemWithContext::Struct {
                        item_struct,
                        transformed: None,
                        is_lifetime: None,
                    });
                }
                syn::Item::Enum(item_enum) => {
                    items.push_back(SynItemWithContext::Enum {
                        item_enum,
                        transformed: None,
                        is_lifetime: None,
                    });
                }
                _ => {}
            }
        }
        self.files.push(SynFile {
            source,
            _shebang: syn_file.shebang,
            _attrs: syn_file.attrs,
            items,
        });
    }

    pub fn load_and_push(&mut self, source: Source) -> Result<(), String> {
        let contents = match &source {
            Source::File { path } => {
                std::fs::read_to_string(path.as_str()).map_err(|e| format!("{e:?}"))?
            }
            Source::Registry { .. } => unimplemented!(),
            Source::Git { .. } => unimplemented!(),
        };
        let ast = syn::parse_file(contents.as_str()).map_err(|e| format!("{e:?}"))?;
        self.push_file(source, ast);
        Ok(())
    }

    pub fn transform(&mut self) -> Option<Context> {
        let mut modules = vec![];
        // let mut visit_user_types = VisitUserTypes {
        //     files: &mut self.files
        // };
        let mut item_counts = vec![];
        for syn_file in &self.files {
            item_counts.push(syn_file.items.len());
        }
        for k in 0..8 {
            // Take each item and run collect and convert pass, then put it back. (To not disturb borrow checker).
            for i in 0..self.files.len() {
                for _ in 0..item_counts[i] {
                    let mut item = self.files[i].items.pop_front().expect("");
                    let current_file = self.files.get(i).expect("");
                    let mut finalize = CollectAndConvertPass {
                        _files: &self.files,
                        current_file,
                        messages: self
                            .messages
                            .entry(current_file.source.clone())
                            .or_default(),
                        _source: current_file.source.clone(),
                    };
                    finalize.transform(&mut item);
                    self.files[i].items.push_back(item);
                }
            }
            // Check if more passes are needed (each time a type references another type, one more pass is required)
            println!("After pass {}", k + 1);
            if !self.need_more_passes() {
                break;
            }
        }
        if self.need_more_passes() {
            if let Some((_, messages)) = self.messages.iter_mut().next() {
                messages.push_conversion_error(SynConversionError::RecursionLimitReached);
            }
            return None;
        }
        println!("Done");
        for syn_file in self.files.drain(..) {
            let mut items = vec![];
            for item in syn_file.items {
                match item {
                    SynItemWithContext::Enum { transformed, .. } => {
                        if let Some(item_enum) = transformed {
                            items.push(crate::ast::Item::Enum(item_enum));
                        }
                    }
                    SynItemWithContext::Struct { transformed, .. } => {
                        if let Some(item_struct) = transformed {
                            items.push(crate::ast::Item::Struct(item_struct));
                        }
                    }
                }
            }
            let mut attrs = syn_file._attrs;
            let docs = collect_docs_attrs(&mut attrs);
            modules.push(Module {
                docs,
                source: syn_file.source.clone(),
                version: Version {
                    major: 0,
                    minor: 1,
                    patch: 0,
                },
                items,
            });
        }
        let error_count = self.messages.iter().fold(0, |error_count, (_, messages)| {
            error_count + messages.error_count()
        });
        if error_count == 0 {
            Some(Context { modules })
        } else {
            None
        }
    }

    fn need_more_passes(&self) -> bool {
        for file in &self.files {
            for item in &file.items {
                let item_not_transformed = match item {
                    SynItemWithContext::Enum { transformed, .. } => transformed.is_none(),
                    SynItemWithContext::Struct { transformed, .. } => transformed.is_none(),
                };
                if item_not_transformed {
                    return true;
                }
            }
        }
        false
    }

    pub fn messages(&self) -> impl Iterator<Item = (&Source, &Messages)> {
        self.messages.iter()
    }
}
