use pest::Span;
use crate::ast::expr::Expr;
use crate::ast::lit::Lit;
use crate::ast::ops::BinaryOp;
use crate::ast::naming::BuiltinTypename;
use crate::error::{ParseError, ParseErrorKind, ParseErrorSource};
use super::prelude::*;

#[derive(Debug)]
pub enum Ty<'i> {
    Boolean,
    Discrete {
        is_signed: bool,
        bits: u32,
        shift: u128,
    },
    FixedPoint {
        is_signed: bool,
        m: u32,
        n: u32,
        shift: u128,
    },
    FloatingPoint {
        bits: u32
    },
    AutoNumber(AutoNumber<'i>),
    IndexOf(Expr<'i>),
    Textual(&'i str),
    Sequence,
    UserDefined,
    Derive,
}

// pub struct NumberTy<'i> {
//     kind: NumberKind<'i>,
//     unit: Option<Unit<'i>>,
//     bound: Option<NumberBound<'i>>,
// }

impl<'i> Parse<'i> for Ty<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        // crate::util::ppt!(input.pairs);
        let ty = input.pairs.next().ok_or_else(|| ParseErrorSource::internal())?;
        match ty.clone().as_rule() {
            Rule::bool_ty => {
                Ok(Ty::Boolean)
            }
            Rule::discrete_any_ty => {
                let discrete_x_ty = ty
                    .into_inner().next().ok_or(ParseErrorSource::internal())?;
                let bits: u32 = discrete_x_ty
                    .as_str().strip_prefix("u")
                    .or(discrete_x_ty.as_str().strip_prefix("i"))
                    .ok_or(ParseErrorSource::internal())?.parse().map_err(|_| {
                        input.push_error(&discrete_x_ty, ParseErrorKind::IntParseError);
                        ParseErrorSource::internal()
                })?;
                let is_signed = discrete_x_ty.as_rule() == Rule::discrete_signed_ty;
                Ok(Ty::Discrete { is_signed, bits, shift: 0 })
            }
            Rule::fixed_any_ty => {
                Err(ParseErrorSource::Unimplemented("fixed ty"))
            }
            Rule::floating_any_ty => {
                Err(ParseErrorSource::Unimplemented("floating ty"))
            }
            Rule::textual_any_ty => {
                Err(ParseErrorSource::Unimplemented("textual ty"))
            }
            Rule::tuple_ty => {
                Err(ParseErrorSource::Unimplemented("tuple ty"))
            }
            Rule::array_ty => {
                Err(ParseErrorSource::Unimplemented("array ty"))
            }
            Rule::identifier => {
                Err(ParseErrorSource::Unimplemented("identifier ty"))
            }
            Rule::generic_ty => {
               parse_generic_ty(&mut ParseInput::fork(ty.clone(), input), ty.as_span())
            }
            Rule::derive => {
                Ok(Ty::Derive)
            }
            Rule::fn_ty => {

                Err(ParseErrorSource::Unimplemented("fn ty"))
            }
            _ => {
                Err(ParseErrorSource::internal_with_rule(ty.as_rule()))
            }
        }
    }
}

#[derive(Debug)]
pub struct AutoNumber<'i> {
    pub start: Lit<'i>,
    pub step: Lit<'i>,
    pub end: Lit<'i>,
    pub inclusive: bool,
}

fn parse_generic_ty<'i, 'm>(input: &mut ParseInput<'i, 'm>, span: Span<'i>) -> Result<Ty<'i>, ParseErrorSource> {
    let typename: BuiltinTypename = input.parse()?;
    match typename.typename {
        "autonum" => parse_autonum_ty(input, span),
        "indexof" => parse_indexof_ty(input, span),
        _ => {
            dbg!("unimpl", typename);
            Err(ParseErrorSource::Unimplemented("generic ty"))
        }
    }
}

fn parse_autonum_ty<'i, 'm>(input: &mut ParseInput<'i, 'm>, span: Span<'i>) -> Result<Ty<'i>, ParseErrorSource> {
    let (ex1, ex2) = input.expect2(Rule::expression, Rule::expression)?;

    let mut ex1 = ParseInput::fork(ex1, input);
    let start: Lit = ex1.parse()?;
    let mut ex2 = ParseInput::fork(ex2, input);
    let step: Lit = ex2.parse()?;
    let range_op: BinaryOp = ex2.parse()?;
    let end: Lit = ex2.parse()?;

    if !start.is_a_number() || !step.is_a_number() || !end.is_a_number() ||
        !start.is_same_kind(&step) || !step.is_same_kind(&end) ||
        !range_op.is_range_op()
    {
        input.errors.push(ParseError {
            kind: ParseErrorKind::AutonumWrongArguments,
            rule: Rule::generic_ty,
            span: (span.start(), span.end())
        })
    }

    let inclusive = range_op == BinaryOp::ClosedRange;

    Ok(Ty::AutoNumber(AutoNumber {
        start,
        step,
        end,
        inclusive,
    }))
}

fn parse_indexof_ty<'i, 'm>(input: &mut ParseInput<'i, 'm>, span: Span<'i>) -> Result<Ty<'i>, ParseErrorSource> {
    if !input.pairs.peek().map(|p| p.as_rule() == Rule::expression).unwrap_or(false) {
        input.errors.push(ParseError {
            kind: ParseErrorKind::IndexOfWrongForm,
            rule: Rule::generic_ty,
            span: (span.start(), span.end())
        });
        return Err(ParseErrorSource::UserError);
    }
    return Ok(Ty::IndexOf(input.parse()?));
}
