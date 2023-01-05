use super::prelude::*;
use crate::ast::expr::VecExprParse;
use crate::ast::lit::NumberLitParse;
use ast::NumBound;

pub struct NumBoundParse(pub NumBound);

impl<'i> Parse<'i> for NumBoundParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::num_bound, "NumBoundParse")?, input);
        let bound = input
            .pairs
            .next()
            .ok_or_else(|| ParseErrorSource::internal("wrong num_bound rule"))?;
        match bound.as_rule() {
            Rule::num_unbound => Ok(NumBoundParse(ast::NumBound::Unbound)),
            Rule::num_bound_min => {
                let mut input = ParseInput::fork(bound, &mut input);
                let number: NumberLitParse = input.parse()?;
                Ok(NumBoundParse(NumBound::MinBound(Box::new(number.0))))
            }
            Rule::num_bound_max => {
                let mut input = ParseInput::fork(bound, &mut input);
                let number: NumberLitParse = input.parse()?;
                Ok(NumBoundParse(NumBound::MaxBound(Box::new(number.0))))
            }
            Rule::num_bound_list => {
                let mut input = ParseInput::fork(bound, &mut input);
                let exprs: VecExprParse = input.parse()?;
                Ok(NumBoundParse(NumBound::Set(
                    ast::TryEvaluateInto::NotResolved(exprs.0),
                )))
            }
            _ => Err(ParseErrorSource::internal("wrong num_bound rule")),
        }
    }
}
