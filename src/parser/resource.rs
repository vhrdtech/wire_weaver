use crate::lexer::token::{Token, TokenKind, LiteralKind, Base, Lit, TokenStream, Span, TreeIndent, IdentKind};
use super::InclusiveRange;
use super::{braced, bracketed, parenthesized, inclusive_range};
use crate::{tok, tstok};
use crate::ast;
use nom::bytes::complete::tag;
use nom::sequence::{delimited, separated_pair, terminated, pair};
use nom::branch::alt;
use nom::multi::many0;
use nom::combinator::{opt, map};

/// TODO: Add support for arrays ([0, 2, 5] for ex.).
#[derive(Debug)]
pub(crate) struct ResourceHeader {
    pub(crate) left_part: Option<Token>,
    pub(crate) set: Option<InclusiveRange>,
    pub(crate) right_part: Option<Token>,
    pub(crate) r#type: Option<Token>,
    pub(crate) id: Option<Token>
}

/// Eats `/CTRL{0-3}ABC(register)[7]`, where `{inc.range}`, `(type)`, `[unique id]` is optional,
/// but must come in the specified order.
///
/// `{'A'-'B'}REG` and `/{'A'-'B'}` is also allowed.
///
/// TODO: Allow digits in the beginning of a second part of the name.
pub(crate) fn resource_header(ts: TokenStream) -> nom::IResult<TokenStream, ResourceHeader> {
    let (ts, _) = tag(tstok!(Slash))(ts)?;
    let (ts, lp) = opt(tag(tstok!(Ident/Normal)))(ts)?;
    let (ts, set_rp) = if lp.is_some() {
        map(opt(pair(
            map(braced(inclusive_range), |r| Some(r) ),
            opt(tag(tstok!(Ident/Normal))))
        ), |o| o.unwrap_or((None, None)) )(ts)?
    } else {
        pair(opt(braced(inclusive_range)), opt(tag(tstok!(Ident/Normal))) )(ts)?
    };
    let (ts, r#type) = opt(parenthesized(tag(tstok!(Ident/Normal))) )(ts)?;
    let (ts, id) = opt(bracketed(tag(tstok!(l/Int/Any))) )(ts)?;

    Ok( (ts, ResourceHeader{
        left_part: lp.map(|lp| lp[0].clone()),
        set: set_rp.0,
        right_part: set_rp.1.map(|rp| rp[0].clone()),
        r#type: r#type.map(|t| t[0].clone()),
        id: id.map(|id| id[0].clone())
    } ))
}

// pub fn resource(ts: TokenStream) -> nom::IResult<TokenStream, ast::Resource> {
//     let (ts, header) = resource_header(ts)?;
//
// }

#[cfg(test)]
mod tests {
//     use nom::error::{ErrorKind, VerboseError, convert_error};
//     use nom::Err;
//
//     #[test]
//     fn left_part_works() {
//         assert_eq!(super::left_part("cfg1{"), Ok(("{", "cfg1", )));
//         assert_eq!(super::left_part("c}fg1{"), Ok(("}fg1{", "c", )));
//         assert_eq!(super::left_part("cfg1["), Ok(("[", "cfg1", )));
//         assert_eq!(super::left_part("cfg1("), Ok(("(", "cfg1", )));
//         assert_eq!(super::left_part("_0abc"), Ok(("", "_0abc", )));
//         assert_eq!(super::left_part("0abc"), Err(nom::Err::Error(("0abc", nom::error::ErrorKind::Alpha))));
//     }
//
//     #[test]
//     fn set_works() {
//         assert_eq!(super::set::<(&str, ErrorKind)>("{1-4}"), Ok(("", "1-4")));
//         //println!("{:#?}", super::set::<VerboseError<&str>>("{1-4]"));
// //        let data = "{1-4]";
// //        match super::set::<VerboseError<&str>>(data) {
// //            Err(nom::Err::Error(e)) | Err(nom::Err::Failure(e)) => {
// //                println!("{}", convert_error(data, e));
// //            }
// //            _ => {}
// //        }
//         assert_eq!(super::set::<(&str, ErrorKind)>("{1-4]"), Err(nom::Err::Error(("", nom::error::ErrorKind::Char))));
//     }
//
//     #[test]
//     fn resource_type_works() {
//         assert_eq!(super::resource_type("(register)xy"), Ok(("xy", "register")));
//         assert_eq!(super::resource_type("(012)"), Err(nom::Err::Error(("012)", nom::error::ErrorKind::Alpha))));
//     }
//
//     #[test]
//     fn resource_id_works() {
//         assert_eq!(super::resource_id("[7]xy"), Ok(("xy", "7")));
//         assert_eq!(super::resource_id("[x]"), Err(nom::Err::Error(("x]", nom::error::ErrorKind::Digit))));
//     }
}