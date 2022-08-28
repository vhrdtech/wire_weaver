use crate::ast::expr::Expr;
use crate::error::{ParseError, ParseErrorKind};
use super::prelude::*;

#[derive(Debug, Clone)]
pub enum NumBound<'i> {
    Unbound,
    MaxBound(u64),
    Set(Vec<Expr<'i>>)
}

impl<'i> Parse<'i> for NumBound<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::num_bound)?, input);
        let bound = input.pairs.next().ok_or_else(|| ParseErrorSource::internal("wrong num_bound rule"))?;
        match bound.as_rule() {
            Rule::num_unbound => Ok(NumBound::Unbound),
            Rule::num_bound_max => {
                let dec_lit_raw = bound.into_inner().next().ok_or_else(||
                    ParseErrorSource::internal("wrong num_bound_list rule")
                )?;
                let max: u64 = dec_lit_raw.as_str().parse().map_err(|_| {
                    input.errors.push(ParseError {
                        kind: ParseErrorKind::IntParseError,
                        rule: Rule::dec_lit,
                        span: (dec_lit_raw.as_span().start(), dec_lit_raw.as_span().end())
                    });
                    ParseErrorSource::UserError
                })?;
                Ok(NumBound::MaxBound(max))
            }
            Rule::num_bound_list => {
                let mut input = ParseInput::fork(bound, &mut input);
                Ok(NumBound::Set(input.parse()?))
            }
            _ => {
                return Err(ParseErrorSource::internal("wrong num_bound rule"))
            }
        }
    }
}

