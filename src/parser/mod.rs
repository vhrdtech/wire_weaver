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
    Ok( (ts, InclusiveRange{ from: range.0[0], to: range.1[0] } ))
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

/// Eats `/CTRL{0-3}ABC(register)[7]`, where `{inc.range}`, `(type)`, `[unique id]` is optional,
/// but must come in the specified order.
///
/// `{'A'-'B'}REG` and `/{'A'-'B'}` is also allowed.
///
/// TODO: Allow digits in the beginning of a second part of the name.
fn resource_header(ts: TokenStream) -> nom::IResult<TokenStream, ResourceHeader> {
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
        left_part: lp.map(|lp| lp[0]),
        set: set_rp.0,
        right_part: set_rp.1.map(|rp| rp[0]),
        r#type: r#type.map(|t| t[0]),
        id: id.map(|id| id[0])
    } ))
}

pub fn parser_play() {
    // println!("ts macro: {:?}", tstok!(l/Int/Any/10-12));
    let tokens = get_some_tokens();
    let ts = TokenStream::new(&tokens);

    println!("tparser called with: {:?}", ts);
    let r = resource_header(ts);
    println!("tparser result: {:?}", r);
}