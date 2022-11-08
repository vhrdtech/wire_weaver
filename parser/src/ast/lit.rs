use super::prelude::*;
use crate::ast::paths::PathParse;
use crate::ast::ty::{DiscreteTyParse, FloatTyParse};
use crate::error::{ParseError, ParseErrorKind};
use ast::lit::{ArrayLit, DiscreteLit, LitKind, NumberLit, NumberLitKind, StructLit, StructLitItem};
use ast::{DiscreteTy, Lit, NumBound};

pub struct LitParse(pub Lit);

pub struct NumberLitParse(pub NumberLit);

pub struct LitKindParse(pub LitKind);

pub struct StructLitItemParse(pub StructLitItem);

impl<'i> Parse<'i> for LitParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1_either(Rule::lit, Rule::number_lit)?, input);
        // crate::util::pest_print_tree(input.pairs.clone());
        let lit = input
            .pairs
            .peek()
            .ok_or_else(|| ParseErrorSource::internal("empty any_lit"))?;
        let span = input.span.clone();
        let ast_lit = match lit.as_rule() {
            Rule::bool_lit => {
                let bool_lit = input.expect1(Rule::bool_lit)?;
                Lit {
                    kind: LitKind::Bool(bool_lit.as_str() == "true"),
                    span,
                }
            }
            Rule::float_lit => parse_float_lit(&mut input)?,
            Rule::discrete_lit => parse_discrete_lit(&mut input)?,
            Rule::char_lit => parse_char_lit(&mut input)?,
            Rule::string_lit => parse_string_lit(&mut input)?,
            Rule::tuple_lit => parse_tuple_lit(&mut input)?,
            Rule::struct_lit => parse_struct_lit(&mut input)?,
            Rule::array_lit => parse_array_lit(&mut input)?,
            Rule::xpi_serial => parse_xpi_serial(&mut input)?,
            _ => {
                return Err(ParseErrorSource::internal_with_rule(
                    lit.as_rule(),
                    "Lit::parse: expected any_lit",
                ));
            }
        };
        Ok(LitParse(ast_lit))
    }
}

impl<'i> Parse<'i> for NumberLitParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let lit: LitParse = input.parse()?;
        match lit.0.kind {
            LitKind::Discrete(ds) => Ok(NumberLitParse(NumberLit {
                kind: NumberLitKind::Discrete(ds),
                span: lit.0.span,
            })),
            LitKind::Fixed(fx) => Ok(NumberLitParse(NumberLit {
                kind: NumberLitKind::Fixed(fx),
                span: lit.0.span,
            })),
            LitKind::Float(fl) => Ok(NumberLitParse(NumberLit {
                kind: NumberLitKind::Float(fl),
                span: lit.0.span,
            })),
            _ => Err(ParseErrorSource::internal("wrong any_number_lit rule")),
        }
    }
}

fn parse_discrete_lit(input: &mut ParseInput) -> Result<Lit, ParseErrorSource> {
    let mut input = ParseInput::fork(input.expect1(Rule::discrete_lit)?, input);
    let span = input.span.clone();
    let x_lit_raw = input.expect1_any()?;
    let (radix, x_str_raw) = match x_lit_raw.as_rule() {
        Rule::bin_lit_raw => (2, &x_lit_raw.as_str()[2..]),
        Rule::oct_lit_raw => (8, &x_lit_raw.as_str()[2..]),
        Rule::dec_lit_raw => (10, x_lit_raw.as_str()),
        Rule::hex_lit_raw => (16, &x_lit_raw.as_str()[2..]),
        _ => {
            return Err(ParseErrorSource::internal("wrong discrete_lit rule"));
        }
    };
    let x_str_raw = x_str_raw.replace("_", ""); // TODO: improve parsing speed?
    let val = u128::from_str_radix(&x_str_raw, radix).map_err(|_| {
        input.errors.push(ParseError {
            kind: ParseErrorKind::IntParseError,
            rule: Rule::dec_lit_raw,
            span: (span.start, span.end),
        });
        ParseErrorSource::UserError
    })?;
    let (ty, is_ty_forced) = if input.pairs.peek().is_some() {
        let ty: DiscreteTyParse = input.parse()?;
        (ty.0, true)
    } else {
        (
            DiscreteTy {
                is_signed: true,
                bits: 32,
                num_bound: NumBound::Unbound,
                unit: (),
            },
            false,
        )
    };
    Ok(Lit {
        kind: LitKind::Discrete(DiscreteLit {
            val,
            ty,
            is_ty_forced,
        }),
        span,
    })
}

fn parse_float_lit(input: &mut ParseInput) -> Result<Lit, ParseErrorSource> {
    let mut input = ParseInput::fork(input.expect1(Rule::float_lit)?, input);
    let fx = input.expect1(Rule::float_lit_internal)?.as_str();
    let fx = fx
        .to_owned()
        .chars()
        .filter(|c| *c != '_')
        .collect::<String>();
    let ty: FloatTyParse = input
        .parse_or_skip()?
        .unwrap_or(FloatTyParse(ast::ty::FloatTy {
            bits: 64,
            num_bound: ast::NumBound::Unbound,
            unit: (),
        }));

    Ok(Lit {
        kind: LitKind::Float(ast::lit::FloatLit {
            digits: fx.to_owned(),
            ty: ty.0,
            is_ty_forced: false,
        }),
        span: input.span.clone(),
    })
}

fn parse_char_lit(input: &mut ParseInput) -> Result<Lit, ParseErrorSource> {
    let mut input = ParseInput::fork(input.expect1(Rule::char_lit)?, input);
    let char = input.expect1(Rule::char)?;
    if char.as_str().starts_with("\\\\") {
        Err(ParseErrorSource::internal("char escape is unimplemented"))
    } else {
        let c = char.as_str().chars().next().unwrap();
        Ok(Lit {
            kind: LitKind::Char(c),
            span: input.span.clone(),
        })
    }
}

fn parse_string_lit(input: &mut ParseInput) -> Result<Lit, ParseErrorSource> {
    let mut input = ParseInput::fork(input.expect1(Rule::string_lit)?, input);
    let string_inner = input.expect1(Rule::string_inner)?;
    Ok(Lit {
        kind: LitKind::String(string_inner.as_str().to_owned()),
        span: input.span.clone(),
    })
}

fn parse_tuple_lit(input: &mut ParseInput) -> Result<Lit, ParseErrorSource> {
    let mut input = ParseInput::fork(input.expect1(Rule::tuple_lit)?, input);
    let mut lits = vec![];
    while let Some(_) = input.pairs.peek() {
        let lit: LitParse = input.parse()?;
        lits.push(lit.0);
    }
    Ok(Lit {
        kind: LitKind::Tuple(lits),
        span: input.span.clone(),
    })
}

impl<'i> Parse<'i> for StructLitItemParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let name: IdentifierParse<identifier::StructFieldName> = input.parse()?;
        let val: LitParse = input.parse()?;
        Ok(StructLitItemParse(StructLitItem {
            name: name.0,
            val: val.0,
        }))
    }
}

fn parse_struct_lit(input: &mut ParseInput) -> Result<Lit, ParseErrorSource> {
    let mut input = ParseInput::fork(input.expect1(Rule::struct_lit)?, input);
    let path: PathParse = input.parse()?;
    let mut items = vec![];
    while let Some(struct_lit_item) = input.pairs.next() {
        let mut input = ParseInput::fork(struct_lit_item, &mut input);
        let item: StructLitItemParse = input.parse()?;
        items.push(item.0);
    }
    Ok(Lit {
        kind: LitKind::Struct(StructLit {
            path: path.0,
            items,
        }),
        span: input.span,
    })
}

fn parse_array_lit(input: &mut ParseInput) -> Result<Lit, ParseErrorSource> {
    let mut input = ParseInput::fork(input.expect1(Rule::array_lit)?, input);
    let array_lit = input.expect1_either(Rule::array_fill_lit, Rule::vec_lit)?;
    let array_lit_kind = array_lit.as_rule();
    let mut input = ParseInput::fork(array_lit, &mut input);
    if array_lit_kind == Rule::array_fill_lit {
        let val: LitParse = input.parse()?;
        let size: LitParse = input.parse()?;
        let size_span = (size.0.span.start, size.0.span.end);
        let LitKind::Discrete(size) = size.0.kind else {
            input.errors.push(ParseError {
                kind: ParseErrorKind::ArrayFillLitWithNotDiscreteSize,
                rule: Rule::array_fill_lit,
                span: size_span,
            });
            return Err(ParseErrorSource::UserError);
        };
        let size = size.to_usize().ok_or_else(|| {
            input.errors.push(ParseError {
                kind: ParseErrorKind::ArrayFillLitWrongSize,
                rule: Rule::array_fill_lit,
                span: size_span,
            });
            ParseErrorSource::UserError
        })?;
        Ok(Lit {
            kind: LitKind::Array(ArrayLit::Init {
                size,
                val: Box::new(val.0),
            }),
            span: input.span.clone(),
        })
    } else {
        let mut items = vec![];
        while let Some(_) = input.pairs.peek() {
            let val: LitParse = input.parse()?;
            items.push(val.0);
        }
        Ok(Lit {
            kind: LitKind::Array(ArrayLit::List(items)),
            span: input.span.clone(),
        })
    }
}

fn parse_xpi_serial(input: &mut ParseInput) -> Result<Lit, ParseErrorSource> {
    let mut input = ParseInput::fork(input.expect1(Rule::xpi_serial)?, input);
    let serial = input.expect1(Rule::dec_lit_raw)?;
    let serial = serial.as_str().replace("_", ""); // TODO: improve parsing speed?
    let serial = u32::from_str_radix(&serial, 10).map_err(|_| {
        input.errors.push(ParseError {
            kind: ParseErrorKind::IntParseError,
            rule: Rule::xpi_serial,
            span: (input.span.start, input.span.end),
        });
        ParseErrorSource::UserError
    })?;
    Ok(Lit {
        kind: LitKind::XpiSerial(serial),
        span: input.span,
    })
}
