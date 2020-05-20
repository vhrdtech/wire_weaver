pub mod resource;

use crate::lexer::token::{Token, TokenKind, LiteralKind, Base, Lit, TokenStream, Span, TreeIndent, IdentKind};
use crate::lexer::{get_some_tokens};
use nom::bytes::complete::tag;

use crate::{tok, tstok, ts};
use resource::ResourceHeader;
use nom::sequence::{delimited, separated_pair, terminated, pair};
use nom::branch::alt;
use nom::multi::many0;
use nom::combinator::{opt, map};

#[derive(Debug)]
pub(crate) struct InclusiveRange {
    from: Token,
    to: Token
}

fn space(ts: TokenStream) -> nom::IResult<TokenStream, ()> {
    let (ts, _) = many0(alt((
        tag(tstok!(Whitespace)),
        tag(tstok!(Comment)),
        tag(tstok!(TreeIndent))
    )))(ts)?;
    Ok ((ts, ()))
}

/// Eats two integer literals separated with `-`.
/// TODO: Add Char support.
fn inclusive_range(ts: TokenStream) -> nom::IResult<TokenStream, InclusiveRange> {
    let (ts, range) = separated_pair(
        tag(tstok!(l/Int/Any)),
        delimited(space, tag(tstok!(Minus)), space),
      tag(tstok!(l/Int/Any)) )(ts)?;
    Ok( (ts, InclusiveRange{ from: range.0[0].clone(), to: range.1[0].clone() } ))
}

/// Eats `{`, then gets an object from the `f` parser, then eats `}`.
fn braced<'a, O, E: nom::error::ParseError<TokenStream<'a>>, F>(f: F) -> impl Fn(TokenStream<'a>) -> nom::IResult<TokenStream<'a>, O, E>
    where
        F: Fn(TokenStream<'a>) -> nom::IResult<TokenStream<'a>, O, E>,
{
    move |input: TokenStream| {
        let i = input.clone();
        let (input, _) = tag(tstok!(OpenBrace))(input)?;
        let (input, output) = f(input)?;
        let (input, _) = tag(tstok!(CloseBrace))(input)?;
        Ok((input, output))
    }
}

/// Eats `[`, then gets an object from the `f` parser, then eats `]`.
fn bracketed<'a, O, E: nom::error::ParseError<TokenStream<'a>>, F>(f: F) -> impl Fn(TokenStream<'a>) -> nom::IResult<TokenStream<'a>, O, E>
    where
        F: Fn(TokenStream<'a>) -> nom::IResult<TokenStream<'a>, O, E>,
{
    move |input: TokenStream| {
        let i = input.clone();
        let (input, _) = tag(tstok!(OpenBracket))(input)?;
        let (input, output) = f(input)?;
        let (input, _) = tag(tstok!(CloseBracket))(input)?;
        Ok((input, output))
    }
}

/// Eats `(`, then gets an object from the `f` parser, then eats `)`.
fn parenthesized<'a, O, E: nom::error::ParseError<TokenStream<'a>>, F>(f: F) -> impl Fn(TokenStream<'a>) -> nom::IResult<TokenStream<'a>, O, E>
    where
        F: Fn(TokenStream<'a>) -> nom::IResult<TokenStream<'a>, O, E>,
{
    move |input: TokenStream| {
        let i = input.clone();
        let (input, _) = tag(tstok!(OpenParen))(input)?;
        let (input, output) = f(input)?;
        let (input, _) = tag(tstok!(CloseParen))(input)?;
        Ok((input, output))
    }
}



pub fn parser_play() {
    // println!("ts macro: {:?}", tstok!(l/Int/Any/10-12));
    let tokens = get_some_tokens();
    let ts = TokenStream::new(&tokens);

    println!("tparser called with: {:?}", ts);
    let r = resource::resource_header(ts);
    println!("tparser result: {:?}", r);
}