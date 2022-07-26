use pest::iterators::Pair;
use crate::warning::{ParseWarning, ParseWarningKind};
use super::prelude::*;

#[derive(Debug)]
pub struct Typename<'i> {
    pub typename: &'i str,
}

impl<'i> Parse<'i> for Typename<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Typename<'i>, ParseErrorSource> {
        if let Some(p) = input.pairs.peek() {
            return if p.as_rule() == Rule::identifier {
                let p = input.pairs.next().unwrap();
                check_camel_case(&p, &mut input.warnings);
                Ok(Typename {
                    typename: p.as_str()
                })
            } else {
                Err(ParseErrorSource::Internal)
            };
        }
        Err(ParseErrorSource::Internal)
    }
}

#[derive(Debug)]
pub struct PathSegment<'i> {
    pub segment: &'i str,
}

impl<'i> Parse<'i> for PathSegment<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        match input.pairs.next() {
            Some(identifier) => {
                if identifier.as_rule() != Rule::identifier {
                    return Err(ParseErrorSource::Internal)
                }
                Ok(PathSegment {
                    segment: identifier.as_str()
                })
            },
            None => Err(ParseErrorSource::Internal)
        }
    }
}

fn check_camel_case(pair: &Pair<Rule>, warnings: &mut Vec<ParseWarning>) {
    if pair.as_str().chars().next().unwrap().is_lowercase() {
        warnings.push(ParseWarning {
            kind: ParseWarningKind::NonCamelCaseTypename,
            rule: pair.as_rule(),
            span: (pair.as_span().start(), pair.as_span().end())
        });
    }
}