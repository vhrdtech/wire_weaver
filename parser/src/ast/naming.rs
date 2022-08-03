use super::prelude::*;

#[derive(Debug)]
pub struct Typename<'i> {
    pub typename: &'i str,
}

impl<'i> Parse<'i> for Typename<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Typename<'i>, ParseErrorSource> {
        let ident = input.expect1(Rule::identifier)?;
        //check_camel_case(&ident, &mut input.warnings);
        Ok(Typename {
            typename: ident.as_str()
        })
    }
}

/// Builtin types such as u8<...>, autonum<...>, indexof<...>
#[derive(Debug)]
pub struct BuiltinTypename<'i> {
    pub typename: &'i str,
}

impl<'i> Parse<'i> for BuiltinTypename<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<BuiltinTypename<'i>, ParseErrorSource> {
        let ident = input.expect1(Rule::identifier)?;
        //check_camel_case(&ident, &mut input.warnings);
        Ok(BuiltinTypename {
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
        let ident = input.expect1(Rule::identifier)?;
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
        let ident = input.expect1(Rule::identifier)?;
        //check_lower_snake_case(&ident, &mut input.warnings);
        Ok(EnumEntryName {
            name: ident.as_str()
        })
    }
}

#[derive(Debug, Copy, Clone)]
pub struct XpiUriNamedPart<'i> {
    pub name: &'i str
}

impl<'i> Parse<'i> for XpiUriNamedPart<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let ident = input.expect1(Rule::identifier)?;
        Ok(XpiUriNamedPart {
            name: ident.as_str()
        })
    }
}

#[derive(Debug)]
pub struct XpiKeyName<'i> {
    pub name: &'i str
}

impl<'i> Parse<'i> for XpiKeyName<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let ident = input.expect1(Rule::identifier)?;
        Ok(XpiKeyName {
            name: ident.as_str()
        })
    }
}

#[derive(Debug)]
pub struct FnName<'i> {
    pub name: &'i str,
}

impl<'i> Parse<'i> for FnName<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<FnName<'i>, ParseErrorSource> {
        let ident = input.expect1(Rule::identifier)?;
        Ok(FnName {
            name: ident.as_str()
        })
    }
}


#[derive(Debug)]
pub struct FnArgName<'i> {
    pub name: &'i str,
}

impl<'i> Parse<'i> for FnArgName<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<FnArgName<'i>, ParseErrorSource> {
        let ident = input.expect1(Rule::identifier)?;
        Ok(FnArgName {
            name: ident.as_str()
        })
    }
}

#[derive(Debug)]
pub struct LetStmtName<'i> {
    pub name: &'i str,
}

impl<'i> Parse<'i> for LetStmtName<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<LetStmtName<'i>, ParseErrorSource> {
        let ident = input.expect1(Rule::identifier)?;
        Ok(LetStmtName {
            name: ident.as_str()
        })
    }
}

#[derive(Debug)]
pub struct Identifier<'i> {
    pub name: &'i str,
}

impl<'i> Parse<'i> for Identifier<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Identifier<'i>, ParseErrorSource> {
        let ident = input.expect1(Rule::identifier)?;
        Ok(Identifier {
            name: ident.as_str()
        })
    }
}


// fn check_camel_case(pair: &Pair<Rule>, warnings: &mut Vec<ParseWarning>) {
//     let contains_underscore = pair.as_str().find("_").map(|_| true).unwrap_or(false);
//     if pair.as_str().chars().next().unwrap().is_lowercase() || contains_underscore {
//         warnings.push(ParseWarning {
//             kind: ParseWarningKind::NonCamelCaseTypename,
//             rule: pair.as_rule(),
//             span: (pair.as_span().start(), pair.as_span().end())
//         });
//     }
// }
//
// fn check_lower_snake_case(_pair: &Pair<Rule>, _warnings: &mut Vec<ParseWarning>) {
//
// }