use ast::{DiscreteTy, Ty, TyKind};
use ast::ty::FloatTy;
use super::prelude::*;
use crate::ast::def_fn::FnArgumentsParse;
use crate::ast::expr::ExprParse;
use crate::ast::generics::GenericsParse;
use crate::ast::lit::LitParse;
use crate::ast::num_bound::NumBoundParse;
use crate::ast::ops::BinaryOpParse;
use crate::error::{ParseError, ParseErrorKind, ParseErrorSource};
use crate::ast::unit::UnitParse;

pub struct TyParse(pub Ty);

pub struct TyKindParse(pub TyKind);

pub struct TupleTyParse(pub Vec<Ty>);

pub struct DiscreteTyParse(pub DiscreteTy);

pub struct FloatTyParse(pub FloatTy);

impl<'i> Parse<'i> for TyParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        crate::util::pest_print_tree(input.pairs.clone());
        let any_ty = input.expect1(Rule::any_ty)?;
        let span = input.span.clone();
        let ty = any_ty
            .clone()
            .into_inner()
            .peek()
            .ok_or_else(|| ParseErrorSource::internal("Wrong any_ty grammar"))?;
        let mut input = ParseInput::fork(any_ty, input);
        let ast_ty = match ty.as_rule() {
            Rule::bool_ty => {
                Ty {
                    kind: TyKind::Boolean,
                    span,
                }
            },
            Rule::discrete_any_ty => {
                let discrete_ty: DiscreteTyParse = input.parse()?;
                Ty {
                    kind: TyKind::Discrete(discrete_ty.0),
                    span,
                }
            }
            Rule::fixed_any_ty => {
                return Err(ParseErrorSource::Unimplemented("fixed ty"));
            },
            Rule::floating_any_ty => {
                let float_ty: FloatTyParse = input.parse()?;
                Ty {
                    kind: TyKind::Float(float_ty.0),
                    span,
                }
            },
            Rule::textual_any_ty => {
                if ty.as_str() == "char" {
                    Ty {
                        kind: TyKind::Char,
                        span,
                    }
                } else if ty.as_str() == "str" {
                    Ty {
                        kind: TyKind::String { len_bound: ast::NumBound::Unbound },
                        span,
                    }
                } else {
                    return Err(ParseErrorSource::Unimplemented("textual ty"));
                }
            }
            Rule::tuple_ty => parse_tuple_ty(&mut input)?,
            Rule::array_ty => parse_array_ty(&mut input)?,
            Rule::identifier => {
                let ident: IdentifierParse<identifier::VariableRefName> = ty.into();
                Ty {
                    kind: TyKind::UserDefined(ident.0),
                    span,
                }
            },
            Rule::generic_ty => {
                parse_generic_ty(&mut input, span)?
            }
            Rule::derive => Ty {
                kind: TyKind::Derive,
                span,
            },
            Rule::fn_ty => {
                let args: FnArgumentsParse = input.parse()?;
                let ret_ty: Option<TyParse> = input.parse_or_skip()?;
                Ty {
                    kind: TyKind::Fn {
                        args: args.0,
                        ret_ty: ret_ty.map(|ty| Box::new(ty.0)).unwrap_or(Box::new(Ty {
                            kind: TyKind::Unit,
                            span: span.clone(),
                        })),
                    },
                    span,
                }
            }
            _ => {
                return Err(ParseErrorSource::internal_with_rule(
                    ty.as_rule(),
                    "Ty::parse: unimplemented ty",
                ));
            },
        };
        Ok(TyParse(ast_ty))
    }
}

impl<'i> Parse<'i> for DiscreteTyParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::discrete_any_ty)?, input);
        let discrete_x_ty = input.pairs
            .next()
            .ok_or(ParseErrorSource::internal("empty discrete_any_ty"))?;
        let bits: u32 = discrete_x_ty
            .as_str()
            .strip_prefix("u")
            .or(discrete_x_ty.as_str().strip_prefix("i"))
            .ok_or(ParseErrorSource::internal("wrong discrete prefix"))?
            .parse()
            .map_err(|_| {
                input.push_error(&discrete_x_ty, ParseErrorKind::IntParseError);
                ParseErrorSource::UserError
            })?;
        let is_signed = discrete_x_ty.as_rule() == Rule::discrete_signed_ty;
        let num_bound: Option<NumBoundParse> = input.parse_or_skip()?;
        let unit: Option<UnitParse> = input.parse_or_skip()?;
        Ok(DiscreteTyParse(DiscreteTy {
            is_signed,
            bits,
            num_bound: num_bound.map(|b| b.0).unwrap_or(ast::NumBound::Unbound),
            unit: unit.map(|u| u.0).unwrap_or(()),
        }
        ))
    }
}

impl<'i> Parse<'i> for FloatTyParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let float_ty_inner = input.expect1(Rule::float_ty_inner)?;
        let bits: u32 = float_ty_inner
            .clone()
            .into_inner()
            .next().ok_or(ParseErrorSource::internal("wrong float_ty"))?
            .as_str()
            .parse()
            .map_err(|_| {
                input.push_error(&float_ty_inner, ParseErrorKind::IntParseError);
                ParseErrorSource::UserError
            })?;
        let num_bound: Option<NumBoundParse> = input.parse_or_skip()?;
        let unit: Option<UnitParse> = input.parse_or_skip()?;
        Ok(FloatTyParse(FloatTy {
            bits,
            num_bound: num_bound.map(|b| b.0).unwrap_or(ast::NumBound::Unbound),
            unit: unit.map(|u| u.0).unwrap_or(()),
        }))
    }
}

fn parse_generic_ty(input: &mut ParseInput, span: ast::Span) -> Result<Ty, ParseErrorSource> {
    let mut input = ParseInput::fork(input.expect1(Rule::generic_ty)?, input);
    let typename: IdentifierParse<identifier::GenericName> = input.parse()?;
    match typename.0.symbols.as_str() {
        "autonum" => parse_autonum_ty(
            &mut ParseInput::fork(input.expect1(Rule::generics)?, &mut input),
            span,
        ),
        "indexof" => parse_indexof_ty(
            &mut ParseInput::fork(input.expect1(Rule::generics)?, &mut input),
            span,
        ),
        _ => {
            let params: GenericsParse = input.parse()?;
            Ok(Ty {
                kind: TyKind::Generic {
                    id: typename.0,
                    params: params.0,
                },
                span,
            })
        },
    }
}

fn parse_autonum_ty(input: &mut ParseInput, span: ast::Span) -> Result<Ty, ParseErrorSource> {
    let (ex1, ex2) = input
        .expect2(Rule::expression, Rule::expression)
        .map_err(|e| {
            // escalate unexpected input to user error
            input.errors.push(ParseError {
                kind: ParseErrorKind::AutonumWrongArguments,
                rule: Rule::generic_ty,
                span: (span.start, span.end),
            });

            match e {
                ParseErrorSource::UnexpectedInput => ParseErrorSource::UserError,
                e => e,
            }
        })?;

    let mut ex1 = ParseInput::fork(ex1, input);
    let start: LitParse = ex1.parse()?;
    let mut ex2 = ParseInput::fork(ex2, input);
    let step: LitParse = ex2.parse()?;
    let range_op: BinaryOpParse = ex2.parse()?;
    let end: LitParse = ex2.parse()?;

    if !start.0.is_a_number()
        || !step.0.is_a_number()
        || !end.0.is_a_number()
        || !start.0.is_same_kind(&step.0)
        || !step.0.is_same_kind(&end.0)
        || !range_op.0.is_range_op()
    {
        input.errors.push(ParseError {
            kind: ParseErrorKind::AutonumWrongArguments,
            rule: Rule::generic_ty,
            span: (span.start, span.end),
        });
        return Err(ParseErrorSource::UserError);
    }

    let inclusive = range_op.0 == ast::ops::BinaryOp::ClosedRange;

    Ok(Ty {
        kind: TyKind::AutoNumber(ast::AutoNumber {
            start: start.0,
            step: step.0,
            end: end.0,
            inclusive,
        }),
        span,
    })
}

fn parse_indexof_ty(input: &mut ParseInput, span: ast::Span) -> Result<Ty, ParseErrorSource> {
    if !input
        .pairs
        .peek()
        .map(|p| p.as_rule() == Rule::expression)
        .unwrap_or(false)
    {
        input.errors.push(ParseError {
            kind: ParseErrorKind::IndexOfWrongForm,
            rule: Rule::generic_ty,
            span: (span.start, span.end),
        });
        return Err(ParseErrorSource::UserError);
    }
    let expr: ExprParse = input.parse()?;
    Ok(Ty {
        kind: TyKind::IndexTyOf(expr.0),
        span,
    })
}

fn parse_array_ty(input: &mut ParseInput) -> Result<Ty, ParseErrorSource> {
    let ty: TyParse = input.parse()?;
    let len_bound: NumBoundParse = input.parse()?;
    Ok(Ty {
        kind: TyKind::Array {
            ty: Box::new(ty.0),
            len_bound: len_bound.0,
        },
        span: input.span.clone(),
    })
}

fn parse_tuple_ty(input: &mut ParseInput) -> Result<Ty, ParseErrorSource> {
    let mut input = ParseInput::fork(input.expect1(Rule::tuple_fields)?, input);
    let mut types = Vec::new();
    while let Some(_) = input.pairs.peek() {
        let ty: TyParse = input.parse()?;
        types.push(ty.0);
    }
    Ok(Ty {
        kind: TyKind::Tuple { types },
        span: input.span.clone(),
    })
}

impl<'i> Parse<'i> for TupleTyParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::tuple_fields)?, input);
        let mut tys = Vec::new();
        while let Some(_) = input.pairs.peek() {
            let ty: TyParse = input.parse()?;
            tys.push(ty.0);
        }
        Ok(TupleTyParse(tys))
    }
}