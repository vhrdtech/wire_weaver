use crate::ast::ident::Ident;
use crate::ast::path::Path;
use crate::ast::syn_convert::{SynConversionError, SynConversionWarning};

#[derive(Debug)]
pub enum Type {
    // Array,
    Bool,
    Discrete(TypeDiscrete),
    // VariableLength,
    Floating(TypeFloating),
    String,
    Path(Path),
    // Option(Path),
    // Result(Path, Path),
}

#[derive(Debug)]
pub struct TypeDiscrete {
    pub is_signed: bool,
    pub bits: u16,
    // unit
    // bounds
}

#[derive(Debug)]
pub struct TypeFloating {
    pub bits: u16, // unit
                   // bounds
}

impl Type {}
