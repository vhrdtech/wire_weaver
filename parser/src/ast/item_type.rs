use pest::Span;
use crate::ast::item_attrs::Attr;
use crate::error::{ParseError, ParseErrorKind};
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
    AutoNumber(AutoNumber),
    Textual(&'i str),
    Sequence,
    UserDefined
}

impl<'i> Parse<'i> for Type<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ()> {
        crate::util::ppt!(input.pairs);
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
                Err(())
            }
            Rule::floating_any_ty => {
                Err(())
            }
            Rule::textual_any_ty => {
                Err(())
            }
            Rule::tuple_ty => {
                Err(())
            }
            Rule::array_ty => {
                Err(())
            }
            Rule::identifier => {
                Err(())
            }
            Rule::param_ty => {
               parse_param_ty(&mut ParseInput::fork(ty.clone(), input), ty.as_span())
            }
            _ => {
                Err(())
            }
        }
    }
}

#[derive(Debug)]
pub enum AutoNumber {
    Discrete {
        start: u128,
        step: u128,
        end: u128
    },
    Fixed {
        start: f64,
        step: f64,
        end: f64,
        shift: f64
    }
}

fn parse_param_ty<'i, 'm>(input: &mut ParseInput<'i, 'm>, span: Span<'i>) -> Result<Type<'i>, ()> {
    match input.pairs.peek() {
        Some(name) => {
            if name.as_str() == "autonum" {
                let _ = input.pairs.next();

                let (ex1, ex2) = input.next2(Rule::expression, Rule::expression);
                ex1.zip(ex2).map(|(ex1, ex2)| {

                }).ok_or(()).map_err(|_| {
                    input.errors.push(ParseError {
                        kind: ParseErrorKind::AutonumWrongForm,
                        rule: Rule::param_ty,
                        span: (span.start(), span.end())
                    });
                    ()
                })?;
                let discrete = true;
                if discrete {
                    Ok(Type::AutoNumber(AutoNumber::Discrete {
                        start: 0,
                        step: 0,
                        end: 0
                    }))
                } else {
                    Ok(Type::AutoNumber(AutoNumber::Fixed {
                        start: 0.0,
                        step: 0.0,
                        end: 0.0,
                        shift: 0.0
                    }))
                }
            } else {
                println!("not implemented 1");
                let typename: Typename = input.parse()?;

                Err(())
            }
        },
        None => {
            println!("int e 1");
            Err(())
        }
    }
}