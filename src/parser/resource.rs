use crate::lexer::token::Token;
use super::InclusiveRange;

/// TODO: Add support for arrays ([0, 2, 5] for ex.).
#[derive(Debug)]
pub(crate) struct ResourceDeclaration {
    pub(crate) left_part: Option<Token>,
    pub(crate) set: Option<InclusiveRange>,
    pub(crate) right_part: Option<Token>,
    pub(crate) r#type: Option<Token>,
    pub(crate) id: Option<Token>
}

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