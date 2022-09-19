use std::fmt::{Display, Formatter};
use parser::ast::lit::Lit as LitParser;
use parser::ast::lit::LitKind as LitKindParser;
use parser::span::Span;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lit {
    pub kind: LitKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LitKind {
    Bool(bool),
    UDec { bits: u32, val: u128 },
    // Float32(f32), Eq needed ?
    // Float64(f64),
    Char(char),
    String(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VecLit(pub Vec<Lit>);

impl<'i> From<LitParser<'i>> for Lit {
    fn from(lit: LitParser) -> Self {
        let kind = match lit.kind {
            LitKindParser::BoolLit(val) => LitKind::Bool(val),
            LitKindParser::UDecLit { bits, val } => LitKind::UDec { bits, val },
            // LitParser::Float32Lit(val) => Lit::Float32(val),
            // LitParser::Float64Lit(val) => Lit::Float64(val),
            LitKindParser::CharLit(val) => LitKind::Char(val),
            LitKindParser::StringLit(val) => LitKind::String(String::from(val)),
            _ => unimplemented!(),
        };
        Lit {
            kind,
            span: lit.span.into(),
        }
    }
}

impl Display for Lit {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            LitKind::Bool(val) => write!(f, "{}", val),
            LitKind::UDec { val, .. } => write!(f, "{}", val),
            LitKind::Char(c) => write!(f, "'{}'", c),
            LitKind::String(s) => write!(f, "\"{}\"", s),
        }
    }
}

impl Display for VecLit {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.iter().try_for_each(|lit| write!(f, "{}, ", lit))
    }
}
