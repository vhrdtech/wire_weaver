use std::collections::HashMap;

use crate::ast::{Context, Module, Source, Version};
use crate::transform::collect_and_convert::CollectAndConvertPass;

mod collect_and_convert;
mod syn_util;

// TODO: check that no fields and no variants have the same name
// TODO: check that variants fit within chosen repr

#[derive(Debug)]
pub enum SynConversionWarning {
    UnknownAttribute(String),
    UnknownFileItem,
}

#[derive(Debug)]
pub enum SynConversionError {
    UnknownType,
    WrongDefaultAttr(String),
    WrongDiscriminant,
    WrongReprAttr(String),
}

#[derive(Default)]
pub struct Messages {
    messages: Vec<Message>,
}

pub enum Message {
    SynConversionWarning(SynConversionWarning),
    SynConversionError(SynConversionError),
}

impl Messages {
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
    ast: syn::File,
}

impl Transform {
    pub fn new() -> Self {
        Transform::default()
    }

    pub fn add_file(&mut self, source: Source, syn_ast: syn::File) {
        self.files.push(SynFile {
            source,
            ast: syn_ast,
        });
    }

    pub fn transform(&mut self) -> Context {
        let mut modules = vec![];
        for syn_file in &self.files {
            let mut items = vec![];
            for syn_item in &syn_file.ast.items {
                let mut finalize = CollectAndConvertPass {
                    files: &self.files,
                    messages: self.messages.entry(syn_file.source.clone()).or_default(),
                    source: syn_file.source.clone(),
                    item: syn_item,
                };
                if let Some(item) = finalize.transform() {
                    items.push(item);
                }
            }
            modules.push(Module {
                source: syn_file.source.clone(),
                version: Version {
                    major: 0,
                    minor: 1,
                    patch: 0,
                },
                items,
            });
        }
        Context { modules }
    }
}
