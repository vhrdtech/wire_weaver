use std::fmt::{Display, Formatter};
use parser::ast::lit::Lit as LitParser;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Lit {
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
        match lit {
            LitParser::BoolLit(val) => Lit::Bool(val),
            LitParser::UDecLit { bits, val } => Lit::UDec { bits, val },
            // LitParser::Float32Lit(val) => Lit::Float32(val),
            // LitParser::Float64Lit(val) => Lit::Float64(val),
            LitParser::CharLit(val) => Lit::Char(val),
            LitParser::StringLit(val) => Lit::String(String::from(val)),
            _ => unimplemented!(),
        }
    }
}

impl Display for Lit {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Lit::Bool(val) => write!(f, "{}", val),
            Lit::UDec { val, .. } => write!(f, "{}", val),
            Lit::Char(c) => write!(f, "'{}'", c),
            Lit::String(s) => write!(f, "\"{}\"", s),
        }
    }
}

impl Display for VecLit {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.iter().try_for_each(|lit| write!(f, "{}, ", lit))
    }
}
