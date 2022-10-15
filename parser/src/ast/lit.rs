use ast::Lit;
use ast::lit::LitKind;
use super::prelude::*;
use crate::ast::ty::FloatTyParse;

pub struct LitParse(pub Lit);

pub struct LitKindParse(pub LitKind);

impl<'i> Parse<'i> for LitParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::any_lit)?, input);
        // crate::util::pest_print_tree(input.pairs.clone());
        let lit = input
            .pairs
            .peek()
            .ok_or_else(|| ParseErrorSource::internal("empty any_lit"))?;
        let span = ast_span_from_pest(input.span.clone());
        let ast_lit = match lit.as_rule() {
            Rule::bool_lit => {
                let bool_lit = input.expect1(Rule::bool_lit)?;
                Lit {
                    kind: LitKind::Bool(bool_lit.as_str() == "true"),
                    span,
                }
            },
            Rule::float_lit => parse_float_lit(&mut input)?,
            Rule::discrete_lit => parse_discrete_lit(&mut input)?,
            Rule::char_lit => {
                return Err(ParseErrorSource::Unimplemented("char_lit"));
            },
            Rule::string_lit => {
                return Err(ParseErrorSource::Unimplemented("string_lit"));
            },
            Rule::tuple_lit => {
                return Err(ParseErrorSource::Unimplemented("tuple lit"));
            },
            Rule::struct_lit => {
                return Err(ParseErrorSource::Unimplemented("struct lit"));
            },
            Rule::enum_lit => {
                return Err(ParseErrorSource::Unimplemented("enum lit"));
            },
            Rule::array_lit => {
                return Err(ParseErrorSource::Unimplemented("array lit"));
            },
            _ => {
                return Err(ParseErrorSource::internal_with_rule(
                    lit.as_rule(),
                    "Lit::parse: expected any_lit",
                ));
            },
        };
        Ok(LitParse(ast_lit))
    }
}

fn parse_discrete_lit(input: &mut ParseInput) -> Result<Lit, ParseErrorSource> {
    let _discrete_lit = ParseInput::fork(input.expect1(Rule::discrete_lit)?, input);
    todo!()
}

fn parse_float_lit(input: &mut ParseInput) -> Result<Lit, ParseErrorSource> {
    let mut input = ParseInput::fork(input.expect1(Rule::float_lit)?, input);
    let fx = input.expect1(Rule::float_lit_internal)?.as_str();
    let fx = fx
        .to_owned()
        .chars()
        .filter(|c| *c != '_')
        .collect::<String>();
    let ty: FloatTyParse = input.parse_or_skip()?.unwrap_or(FloatTyParse(ast::ty::FloatTy {
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
        span: ast_span_from_pest(input.span.clone()),
    })
}
