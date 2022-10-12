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
    Float32(String),
    Float64(String),
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
            LitKindParser::Float32Lit(val) => LitKind::Float32(val),
            LitKindParser::Float64Lit(val) => LitKind::Float64(val),
            LitKindParser::CharLit(val) => LitKind::Char(val),
            LitKindParser::StringLit(val) => LitKind::String(String::from(val)),
            u => unimplemented!("{:?}", u),
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
            LitKind::Float32(val) => write!(f, "{}f32", val),
            LitKind::Float64(val) => write!(f, "{}f64", val),
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
