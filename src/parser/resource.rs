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
struct ResourceNameRaw<'a> {
    left_part: Option<&'a str>,
    set: Option<&'a str>,
    right_part: Option<&'a str>,
    r#type: Option<&'a str>,
    id: Option<&'a str>
}

impl<'a> ResourceNameRaw<'a> {
    fn new() -> Self {
        ResourceNameRaw {
            left_part: None,
            set: None,
            right_part: None,
            r#type: None,
            id: None
        }
    }
}

#[derive(Debug)]
enum ResourceNameParts<'a> {
    Slash,
    LeftPart(&'a str),
    SetPart(&'a str),
    RightPart(&'a str),
    TypePart(&'a str),
    IdPart(&'a str),
    Nothing,
    Junk
}

fn left_part<'a, E: ParseError<&'a str>>(i: &'a str) -> nom::IResult<&'a str, &'a str, E> {
//    take_while1(|c| !"{}[]()".contains(c))(i)
    alt((
        preceded(peek(tag("_")), take_while1(|c| c == '_' || is_alphanumeric(c as u8))),
        preceded(peek(alpha1), alphanumeric0)
    ))(i)
}

fn right_part<'a, E: ParseError<&'a str>>(i: &'a str) -> nom::IResult<&'a str, &'a str, E> {
    alphanumeric1(i)
}

fn set<'a, E: ParseError<&'a str>>(i: &'a str) -> nom::IResult<&'a str, &'a str, E> {
    preceded(nom_char('{'),
             terminated(is_not("}"), nom_char('}')))(i)
}

fn resource_type<'a, E: ParseError<&'a str>>(i: &'a str) -> nom::IResult<&'a str, &'a str, E> {
    preceded(nom_char('('),
             terminated(alpha1, nom_char(')')))(i)
}

fn resource_id<'a, E: ParseError<&'a str>>(i: &'a str) -> nom::IResult<&'a str, &'a str, E> {
    preceded(nom_char('['),
             terminated(digit1, nom_char(']')))(i)
}

fn get_slash<'a, E: ParseError<&'a str>>(i: &'a str) -> nom::IResult<&'a str, &'a str, E> {
    tag("/")(i)
}

fn alt_parser<'a, E: ParseError<&'a str>>(i: &'a str) -> nom::IResult<&'a str, ResourceNameParts<'a>, E> {
    alt((
        nom_map(tag("/"), |_| ResourceNameParts::Slash),
        nom_map(left_part, |l| ResourceNameParts::LeftPart(l)),
        nom_map(set, |s| ResourceNameParts::SetPart(s)),
        nom_map(alphanumeric1, |r| ResourceNameParts::RightPart(r)),
        nom_map(resource_type, |t| ResourceNameParts::TypePart(t)),
        nom_map(resource_id, |id| ResourceNameParts::IdPart(id)),
        //nom_map(rest_len, |l| if l == 0 { Err(nom::Err::Error(("", nom::error::ErrorKind::Eof))) } else { Err(()) } )
    ))(i)
}

fn alt_parser22<'a, E: ParseError<&'a str>>(i: &'a str) -> nom::IResult<&'a str, Vec<ResourceNameParts<'a>>, E> {
    many1(alt_parser)(i)
}

fn alt_parser33<'a, E: ParseError<&'a str>>(i: &'a str) -> nom::IResult<&'a str, (), E> {
    let (i, parts) = alt_parser(i)?;
    println!("{:?}", parts);
    let (i, parts) = alt_parser(i)?;
    println!("{:?}", parts);
    let (_, parts) = alt_parser(i)?;
    println!("{:?}", parts);
    Ok(("", ()))
}

// fn parser<'a, E: ParseError<&'a str>>(i: &'a str) -> nom::IResult<&'a str, ResourceNameRaw<'a>, E> {
//     let (i, _) = tag("/")(i)?;
//     let (i, left_part) = left_part(i)?;
//     let (i, inner_part) = alt((
//         nom_map(set, |s| ResourceNameParts::SetPart(s)),
//         nom_map(resource_type, |t| ResourceNameParts::TypePart(t)),
//         nom_map(resource_id, |id| ResourceNameParts::IdPart(id)),
//         nom_map(rest_len, |l| if l == 0 { ResourceNameParts::Nothing } else { ResourceNameParts::Junk } )
//     ))(i)?;
//     let mut rn_raw = ResourceNameRaw::new();
//     rn_raw.left_part = Some(left_part);
//     if let ResourceNameParts::Nothing = inner_part {
//         return Ok((i, rn_raw));
//     } else if let ResourceNameParts::Junk = inner_part {
//         return Err(nom::Err::Error((i, nom::error::ErrorKind::TooLarge)));
//     }
//     let (i, next_part) = match inner_part {
//         ResourceNameParts::SetPart(s) => {
//             rn_raw.set = Some(s);
//             alt((
//                 nom_map(right_part, |r| ResourceNameParts::RightPart(r)),
//                 nom_map(resource_type, |t| ResourceNameParts::TypePart(t)),
//                 nom_map(resource_id, |id| ResourceNameParts::IdPart(id)),
//                 nom_map(rest_len, |l| if l == 0 { ResourceNameParts::Nothing } else { ResourceNameParts::Junk } )
//                 ))(i)?
//         },
//         ResourceNameParts::TypePart(t) => {
//             rn_raw.r#type = Some(t);
//             alt((
//                 nom_map(resource_id, |id| ResourceNameParts::IdPart(id)),
//                 nom_map(rest_len, |l| if l == 0 { ResourceNameParts::Nothing } else { ResourceNameParts::Junk } )
//                 ))(i)?
//         },
//         ResourceNameParts::IdPart(id) => {
//             let (i, rest) = rest_len(i)?;
//             if rest != 0 {
//                 return Err(nom::Err::Error((i, nom::error::ErrorKind::TooLarge)));
//             }
//             rn_raw.id = Some(id);
//             (i, ResourceNameParts::Nothing)
//         },
//         _ => { unreachable!() }
//     };
//     if let ResourceNameParts::Nothing = next_part {
//         return Ok((i, rn_raw));
//     }
//
//     Ok((i, rn_raw))
// }

pub fn parse_resource(_name: &str, _tok_tree: &Yaml) -> ast::Resource {
    //dbg!(tok_tree);

    //let r = parse_name("TEC(register)[]");
    let data = "/abcd{1..2}0abcd(ty)[1]junk";
    let r1 = alt_parser22::<VerboseError<&str>>(data);
    println!("{:?}", r1);
//    let (i, r2) = alt_parser(data)?;
//    let (i, r3) = alt_parser(data)?;
//    let (i, r4) = alt_parser(data)?;
    //println!("{:?}", r1);
   // match alt_parser22::<VerboseError<&str>>(data) {
   //     Err(nom::Err::Error(e)) | Err(nom::Err::Failure(e)) => {
   //         println!("err {} {:#?}", convert_error(data, e.clone()), e);
   //     }
   //     Ok((i, parts)) => {
   //         println!("{}, {:?}", i, parts);
   //     }
   //     _ => { println!("incomplete"); }
   // }

    //let res = many1(alt_parser)("/abcd{1..2}");
    //println!("{:?}", res);

    ast::Resource{
        id: Some(0u32),
        name: ResourceName::Terminal("res".to_string()),
        children: Vec::new(),
        kind: Some(ResourceKind::Property),
        r#type: Some(Type::Numeric(Numeric::U8))
    }
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