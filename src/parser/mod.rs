pub mod resource;

use crate::lexer::token::{Token, TokenKind, LiteralKind, Base, Lit, TokenStream, Span, TreeIndent};
use crate::lexer::{get_some_tokens};
use nom::bytes::complete::tag;

use crate::{tok, tstok, ts};
use resource::ResourceDeclaration;
use nom::sequence::{delimited, separated_pair};
use nom::branch::alt;
use nom::multi::many0;

#[derive(Debug)]
struct InclusiveIntRange {
    from: i32,
    to: i32
}

#[derive(Debug)]
struct InclusiveCharRange {
    from: char,
    to: char
}

#[derive(Debug)]
enum InclusiveRange {
    Int(InclusiveIntRange),
    Char(InclusiveCharRange)
}

fn space(ts: TokenStream) -> nom::IResult<TokenStream, ()> {
    let (ts, _) = many0(alt((
        tag(tstok!(Whitespace)),
        tag(TokenStream::new_with_slice(&[ tok!(Comment) ])),
        tag(TokenStream::new_with_slice(&[ tok!(TreeIndent) ]))
    )))(ts)?;
    Ok ((ts, ()))
}

fn inclusive_range(ts: TokenStream) -> nom::IResult<TokenStream, InclusiveRange> {
    let (ts, x) = separated_pair(
        tag(TokenStream::new_with_slice(&[ tok!(l/Int/Any) ])),
        delimited(space, tag(TokenStream::new_with_slice(&[ tok!(Minus) ])), space),
     tag(TokenStream::new_with_slice(&[ tok!(l/Int/Any) ])))(ts)?;

    println!("range parsed: {:?}", x);
    Ok( (ts, InclusiveRange::Int(InclusiveIntRange{from: 0, to: 123}) ))
}

fn tparser(ts: TokenStream) -> nom::IResult<TokenStream, ResourceDeclaration> {
    let (ts, x) = tag(TokenStream::new_with_slice(&[ tok!(Slash) ]))(ts)?;
    //let (ts, x) = tag(ts!(Slash))(ts)?;

    println!("x: {:?}", x);

    Err(nom::Err::Error((ts, nom::error::ErrorKind::Alpha)))
}

pub fn parser_play() {
    println!("ts macro: {:?}", tstok!(Slash, l/Int/Any));
    // let tokens = get_some_tokens();
    // let ts = TokenStream::new(&tokens);
    //
    // println!("tparser called with: {:?}", ts);
    // let r = inclusive_range(ts);
    // println!("tparser result: {:?}", r);
}