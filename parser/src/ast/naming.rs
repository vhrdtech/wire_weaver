use pest::iterators::Pair;
use crate::warning::{ParseWarning, ParseWarningKind};
use super::prelude::*;

#[derive(Debug)]
pub struct Typename<'i> {
    pub typename: &'i str,
}

impl<'i> Parse<'i> for Typename<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Typename<'i>, ParseErrorSource> {
        let ident = input.next1(Rule::identifier).ok_or(ParseErrorSource::Internal)?;
        //check_camel_case(&ident, &mut input.warnings);
        Ok(Typename {
            typename: ident.as_str()
        })
    }
}

#[derive(Debug)]
pub struct PathSegment<'i> {
    pub segment: &'i str,
}

impl<'i> Parse<'i> for PathSegment<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let ident = input.next1(Rule::identifier).ok_or(ParseErrorSource::Internal)?;
        //check_lower_snake_case(&ident, &mut input.warnings);
        Ok(PathSegment {
            segment: ident.as_str()
        })
    }
}

#[derive(Debug)]
pub struct EnumEntryName<'i> {
    pub name: &'i str,
}

impl<'i> Parse<'i> for EnumEntryName<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let ident = input.next1(Rule::identifier).ok_or(ParseErrorSource::Internal)?;
        //check_lower_snake_case(&ident, &mut input.warnings);
        Ok(EnumEntryName {
            name: ident.as_str()
        })
    }
}

fn check_camel_case(pair: &Pair<Rule>, warnings: &mut Vec<ParseWarning>) {
    let contains_underscore = pair.as_str().find("_").map(|_| true).unwrap_or(false);
    if pair.as_str().chars().next().unwrap().is_lowercase() || contains_underscore {
        warnings.push(ParseWarning {
            kind: ParseWarningKind::NonCamelCaseTypename,
            rule: pair.as_rule(),
            span: (pair.as_span().start(), pair.as_span().end())
        });
    }
}

fn check_lower_snake_case(_pair: &Pair<Rule>, _warnings: &mut Vec<ParseWarning>) {

}