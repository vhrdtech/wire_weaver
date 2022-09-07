use parser::ast::lit::Lit as LitParser;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Lit {
    Bool(bool),
    UDec {
        bits: u32,
        val: u128,
    },
    // Float32(f32), Eq needed ?
    // Float64(f64),
    Char(char),
    String(String),
}

impl<'i> From<LitParser<'i>> for Lit {
    fn from(lit: LitParser) -> Self {
        match lit {
            LitParser::BoolLit(val) => Lit::Bool(val),
            LitParser::UDecLit { bits, val } => Lit::UDec { bits, val },
            // LitParser::Float32Lit(val) => Lit::Float32(val),
            // LitParser::Float64Lit(val) => Lit::Float64(val),
            LitParser::CharLit(val) => Lit::Char(val),
            LitParser::StringLit(val) => Lit::String(String::from(val)),
            _ => unimplemented!()
        }
    }
}