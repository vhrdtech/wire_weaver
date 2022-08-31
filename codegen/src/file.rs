use std::collections::HashSet;
use mtoken::{TokenStream, ToTokens};
use vhl::span::Span;
use crate::dependencies::{Dependencies, Depends, Package};
use crate::error::CodegenError;

/// Collection of code blocks with dependencies and source information.
///
/// Used to render a whole file for one target language while including all the requested dependencies
/// and packages exactly once.
pub struct File {
    pub code_pieces: Vec<(TokenStream, Dependencies, Span)>,
}

impl File {
    pub fn new() -> Self {
        File {
            code_pieces: vec![]
        }
    }

    /// Adds code piece into this file
    pub fn push<T: ToTokens + Depends>(&mut self, tokens: &T, source: vhl::span::Span) {
        let mut ts = TokenStream::new();
        tokens.to_tokens(&mut ts);
        self.code_pieces.push((ts, tokens.dependencies(), source))
    }

    pub fn render(&self) -> Result<(String, HashSet<Package>), CodegenError> {
        let mut rendered = String::new();
        let mut depends_on = HashSet::new();

        for (ts, deps, source) in &self.code_pieces {
            // TODO: get target language and generate comments for it specifically
            rendered.push_str(format!("// Generated from {:#}\n", source).as_str());
            rendered.push_str(format!("{}\n\n", ts).as_str());
            for pkg in &deps.depends {
                depends_on.insert(pkg.clone());
            }
        }

        Ok((rendered, depends_on))
    }
}