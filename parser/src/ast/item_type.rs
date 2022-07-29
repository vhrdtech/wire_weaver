use pest::Span;
use crate::ast::item_lit::ItemLit;
use crate::ast::item_op::ItemOp;
use crate::error::{ParseError, ParseErrorKind, ParseErrorSource};
use super::prelude::*;

#[derive(Debug)]
pub enum Type<'i> {
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
    Textual(&'i str),
    Sequence,
    UserDefined,
    Derive,
}

// pub struct NumberTy<'i> {
//     kind: NumberKind<'i>,
//     bound: Option<NumberBound<'i>>,
// }

impl<'i> Parse<'i> for Type<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        // crate::util::ppt!(input.pairs);
        let ty = input.pairs.next().unwrap();
        match ty.as_rule() {
            Rule::bool_ty => {
                Ok(Type::Boolean)
            }
            Rule::discrete_any_ty => {
                let bits: u32 = ty
                    .as_str().strip_prefix("u")
                    .or(ty.as_str().strip_prefix("i"))
                    .unwrap().parse().unwrap();
                let is_signed = ty
                    .into_inner().next().unwrap().as_rule() == Rule::discrete_signed_ty;
                Ok(Type::Discrete { is_signed, bits, shift: 0 })
            }
            Rule::fixed_any_ty => {
                Err(ParseErrorSource::Unimplemented)
            }
            Rule::floating_any_ty => {
                Err(ParseErrorSource::Unimplemented)
            }
            Rule::textual_any_ty => {
                Err(ParseErrorSource::Unimplemented)
            }
            Rule::tuple_ty => {
                Err(ParseErrorSource::Unimplemented)
            }
            Rule::array_ty => {
                Err(ParseErrorSource::Unimplemented)
            }
            Rule::identifier => {
                Err(ParseErrorSource::Unimplemented)
            }
            Rule::param_ty => {
               parse_param_ty(&mut ParseInput::fork(ty.clone(), input), ty.as_span())
            }
            Rule::derive => {
                Ok(Type::Derive)
            }
            Rule::fn_ty => {

                Err(ParseErrorSource::Unimplemented)
            }
            _ => {
                Err(ParseErrorSource::InternalError)
            }
        }
    }
}

#[derive(Debug)]
pub struct AutoNumber<'i> {
    pub start: ItemLit<'i>,
    pub step: ItemLit<'i>,
    pub end: ItemLit<'i>,
    pub inclusive: bool,
}

fn parse_param_ty<'i, 'm>(input: &mut ParseInput<'i, 'm>, span: Span<'i>) -> Result<Type<'i>, ParseErrorSource> {
    let name = match input.pairs.next() {
        Some(name) => name,
        None => {
            return Err(ParseErrorSource::InternalError)
        }
    };
    if name.as_str() == "autonum" {
        let (ex1, ex2) = input.expect2(Rule::expression, Rule::expression)?;

        let mut ex1 = ParseInput::fork(ex1, input);
        let start: ItemLit = ex1.parse()?;
        let mut ex2 = ParseInput::fork(ex2, input);
        let step: ItemLit = ex2.parse()?;
        let range_op: ItemOp = ex2.parse()?;
        let end: ItemLit = ex2.parse()?;

        if !start.is_a_number() || !step.is_a_number() || !end.is_a_number() ||
            !start.is_same_kind(&step) || !step.is_same_kind(&end) ||
            !range_op.is_range()
        {
            input.errors.push(ParseError {
                kind: ParseErrorKind::AutonumWrongArguments,
                rule: Rule::param_ty,
                span: (span.start(), span.end())
            })
        }

        let inclusive = range_op == ItemOp::ClosedRange;

        Ok(Type::AutoNumber(AutoNumber {
            start,
            step,
            end,
            inclusive,
        }))
    } else {
        println!("not implemented 1");
        let _typename: Typename = input.parse()?;

        Err(ParseErrorSource::Unimplemented)
    }
}