use yaml_rust::Yaml;
use nom::{
    branch::alt,
    bytes::complete::{tag, },
    error::{ParseError},
    combinator::{map as nom_map}
};

use crate::ast;
use crate::ast::{ResourceName, ResourceKind};
use crate::types::Type;
use crate::types::Numeric;
use nom::character::complete::{char as nom_char, alpha1, digit1, alphanumeric0, alphanumeric1};
use nom::bytes::complete::{is_not, take_while1};
use nom::sequence::{terminated, preceded};
use nom::character::is_alphanumeric;
use nom::combinator::{peek,};
use nom::error::{VerboseError};
use nom::multi::many1;

#[derive(Debug)]
pub(crate) struct ResourceDeclaration<'a> {
    left_part: Option<&'a str>,
    set: Option<&'a str>,
    right_part: Option<&'a str>,
    r#type: Option<&'a str>,
    id: Option<&'a str>
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