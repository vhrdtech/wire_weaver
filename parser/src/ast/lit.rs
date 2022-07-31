use pest::iterators::Pair;
use crate::error::{ParseError, ParseErrorKind};
use super::prelude::*;

#[derive(Debug)]
pub enum Lit<'i> {
    BoolLit(bool),
    UDecLit {
        bits: u32,
        lit: u128,
    },
    IDecLit {
        bits: u32,
        lit: i128,
    },
    HexLit(u128),
    OctLit(u128),
    BinLit(u128),
    FixedLit(FixedLit),
    Float32Lit(f32),
    Float64Lit(f64),
    CharLit(char),
    StringLit(&'i str),
    TupleLit,
    StructLit,
    EnumLit,
    ArrayLit,
}

impl<'i> Lit<'i> {
    pub fn is_a_number(&self) -> bool {
        use Lit::*;
        match self {
            UDecLit {..} => true,
            IDecLit {..} => true,
            HexLit(_) => true,
            OctLit(_) => true,
            BinLit(_) => true,
            FixedLit(_) => true,
            Float32Lit(_) => true,
            Float64Lit(_) => true,
            _ => false
        }
    }

    pub fn is_same_kind(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

#[derive(Debug)]
pub enum FixedLit {
    Explicit {
        m: u32,
        n: u32,
        data: u128,
    },
    Implicit(f64)
}

impl<'i> Parse<'i> for Lit<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let any_lit = input.expect1(Rule::any_lit)?;
        let any_lit = any_lit.into_inner().next().unwrap();
        match any_lit.as_rule() {
            Rule::bool_lit => {
                Ok(Lit::BoolLit(any_lit.as_str() == "true"))
            }
            Rule::float_lit => {
                parse_float_lit(input, any_lit)
            }
            Rule::discrete_lit => {

                Err(ParseErrorSource::Unimplemented("discrete lit"))
            }
            Rule::char_lit => {

                Err(ParseErrorSource::Unimplemented("char lit"))
            }
            Rule::string_lit => {

                Err(ParseErrorSource::Unimplemented("string lit"))
            }
            Rule::tuple_lit => {

                Err(ParseErrorSource::Unimplemented("tuple lit"))
            }
            Rule::struct_lit => {

                Err(ParseErrorSource::Unimplemented("struct lit"))
            }
            Rule::enum_lit => {

                Err(ParseErrorSource::Unimplemented("enum lit"))
            }
            Rule::array_lit => {

                Err(ParseErrorSource::Unimplemented("array lit"))
            }
            _ => {
                Err(ParseErrorSource::internal())
            }
        }
    }
}

fn parse_float_lit<'i, 'm>(input: &mut ParseInput<'i, 'm>, any_lit: Pair<'i, Rule>) -> Result<Lit<'i>, ParseErrorSource> {
    let fx = any_lit.as_str();
    let (fx, bits) = fx
        .strip_suffix("f32")
        .map(|fx| (fx, 32))
        .or(fx.strip_suffix("f64")
            .map(|fx| (fx, 64)))
        .unwrap_or((fx, 64));

    let fx = fx.to_owned().chars().filter(|c| *c != '_').collect::<String>();
    if bits == 32 {
        let f: f32 = fx.parse().map_err(|_| {
            input.errors.push(ParseError {
                kind: ParseErrorKind::FloatParseError,
                rule: Rule::float_lit,
                span: (any_lit.as_span().start(), any_lit.as_span().end())
            });
            ParseErrorSource::UserError
        })?;
        Ok(Lit::Float32Lit(f))
    } else {
        let f: f64 = fx.parse().map_err(|_| {
            input.errors.push(ParseError {
                kind: ParseErrorKind::FloatParseError,
                rule: Rule::float_lit,
                span: (any_lit.as_span().start(), any_lit.as_span().end())
            });
            ParseErrorSource::UserError
        })?;
        Ok(Lit::Float64Lit(f))
    }
}