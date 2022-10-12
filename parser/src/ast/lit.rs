use super::prelude::*;
use crate::error::{ParseError, ParseErrorKind};
use pest::iterators::Pair;
use pest::Span;

#[derive(Debug, Clone)]
pub struct Lit<'i> {
    pub kind: LitKind<'i>,
    pub span: Span<'i>,
}

#[derive(Debug, Clone)]
pub enum LitKind<'i> {
    BoolLit(bool),
    UDecLit { bits: u32, val: u128 },
    IDecLit { bits: u32, val: i128 },
    HexLit(u128),
    OctLit(u128),
    BinLit(u128),
    FixedLit(FixedLit),
    Float32Lit(String),
    Float64Lit(String),
    CharLit(char),
    StringLit(&'i str),
    TupleLit,
    StructLit,
    EnumLit,
    ArrayLit,
}

impl<'i> Lit<'i> {
    pub fn is_a_number(&self) -> bool {
        use LitKind::*;
        match self.kind {
            UDecLit { .. } => true,
            IDecLit { .. } => true,
            HexLit(_) => true,
            OctLit(_) => true,
            BinLit(_) => true,
            FixedLit(_) => true,
            Float32Lit(_) => true,
            Float64Lit(_) => true,
            _ => false,
        }
    }

    pub fn is_same_kind(&self, other: &Self) -> bool {
        std::mem::discriminant(&self.kind) == std::mem::discriminant(&other.kind)
    }
}

#[derive(Debug, Clone)]
pub enum FixedLit {
    Explicit { m: u32, n: u32, data: u128 },
    Implicit(f64),
}

impl<'i> Parse<'i> for Lit<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::any_lit)?, input);
        // crate::util::pest_print_tree(input.pairs.clone());
        let x_lit = input
            .pairs
            .next()
            .ok_or_else(|| ParseErrorSource::internal("empty any_lit"))?;
        let span = input.span.clone();
        let mut input = ParseInput::fork(x_lit.clone(), &mut input);
        match x_lit.as_rule() {
            Rule::bool_lit => Ok(Lit {
                kind: LitKind::BoolLit(x_lit.as_str() == "true"),
                span,
            }),
            Rule::float_lit => parse_float_lit(&mut input, x_lit),
            Rule::discrete_lit => {
                let num: u32 = x_lit.as_str().parse().map_err(|_| {
                    input.errors.push(ParseError {
                        kind: ParseErrorKind::IntParseError,
                        rule: Rule::dec_lit,
                        span: (x_lit.as_span().start(), x_lit.as_span().end()),
                    });
                    ParseErrorSource::UserError
                })?;
                Ok(Lit {
                    kind: LitKind::UDecLit {
                        bits: 32,
                        val: num as u128,
                    },
                    span,
                })
            }
            Rule::hex_lit => Err(ParseErrorSource::Unimplemented("hex_lit")),
            Rule::bin_lit => Err(ParseErrorSource::Unimplemented("bin_lit")),
            Rule::oct_lit => Err(ParseErrorSource::Unimplemented("oct_lit")),
            Rule::char_lit => {
                let c = x_lit
                    .as_str()
                    .chars()
                    .skip(1)
                    .next()
                    .ok_or(ParseErrorSource::internal("char_lit grammar error"))?;
                Ok(Lit {
                    kind: LitKind::CharLit(c),
                    span,
                })
            }
            Rule::string_lit => {
                let string_inner = x_lit
                    .into_inner()
                    .next()
                    .ok_or_else(|| ParseErrorSource::internal("wrong string_lit rule"))?;
                Ok(Lit {
                    kind: LitKind::StringLit(string_inner.as_str()),
                    span,
                })
            }
            Rule::tuple_lit => Err(ParseErrorSource::Unimplemented("tuple lit")),
            Rule::struct_lit => Err(ParseErrorSource::Unimplemented("struct lit")),
            Rule::enum_lit => Err(ParseErrorSource::Unimplemented("enum lit")),
            Rule::array_lit => Err(ParseErrorSource::Unimplemented("array lit")),
            _ => Err(ParseErrorSource::internal_with_rule(
                x_lit.as_rule(),
                "Lit::parse: expected any_lit",
            )),
        }
    }
}

fn parse_float_lit<'i, 'm>(
    input: &mut ParseInput<'i, 'm>,
    any_lit: Pair<'i, Rule>,
) -> Result<Lit<'i>, ParseErrorSource> {
    let fx = any_lit.as_str();
    let (fx, bits) = fx
        .strip_suffix("f32")
        .map(|fx| (fx, 32))
        .or(fx.strip_suffix("f64").map(|fx| (fx, 64)))
        .unwrap_or((fx, 64));

    let fx = fx
        .to_owned()
        .chars()
        .filter(|c| *c != '_')
        .collect::<String>();
    if bits == 32 {
        // let f: f32 = fx.parse().map_err(|_| {
        //     input.errors.push(ParseError {
        //         kind: ParseErrorKind::FloatParseError,
        //         rule: Rule::float_lit,
        //         span: (any_lit.as_span().start(), any_lit.as_span().end()),
        //     });
        //     ParseErrorSource::UserError
        // })?;
        Ok(Lit {
            kind: LitKind::Float32Lit(fx),
            span: input.span.clone(),
        })
    } else {
        // let f: f64 = fx.parse().map_err(|_| {
        //     input.errors.push(ParseError {
        //         kind: ParseErrorKind::FloatParseError,
        //         rule: Rule::float_lit,
        //         span: (any_lit.as_span().start(), any_lit.as_span().end()),
        //     });
        //     ParseErrorSource::UserError
        // })?;
        Ok(Lit {
            kind: LitKind::Float64Lit(fx),
            span: input.span.clone(),
        })
    }
}
