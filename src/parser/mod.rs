pub mod resource;

use crate::lexer::token::{Token, TokenKind, LiteralKind, Base, Lit, TokenStream, Span, TreeIndent, IdentKind};
use crate::lexer::{get_some_tokens};
use nom::bytes::complete::tag;

use crate::{tok, tstok, ts};
use resource::ResourceDeclaration;
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

// fn braced<T>(ts: TokenStream) -> nom::IResult<TokenStream, T> {
//     delimited(tag(tstok!(OpenBrace)), inclusive_range, tag(tstok!(CloseBrace)))
// }

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
        // match delimited(tag(tstok!(OpenBrace)), f, tag(tstok!(CloseBrace)))(input) {
        //     Ok((i, o)) => Ok((i, o)),
        //     //Err(Err::Error(_)) => Ok((i, None)),
        //     Err(e) => Err(e),
        // }
    }
}

fn resource_declaration(ts: TokenStream) -> nom::IResult<TokenStream, ResourceDeclaration> {
    let (ts, _) = tag(tstok!(Slash))(ts)?;
    println!("1");
    let (ts, lp) = opt(tag(tstok!(Ident/Normal)))(ts)?;
    println!("2");
    let (ts, set_rp) = if lp.is_some() {
        println!("3");
        map(opt(pair(map(delimited(tag(tstok!(OpenBrace)), inclusive_range, tag(tstok!(CloseBrace))), |r| Some(r) ), opt(tag(tstok!(Ident/Normal))))), |o| o.unwrap_or((None, None)))(ts)?
    } else {
        println!("4");
        pair(opt(delimited(tag(tstok!(OpenBrace)), inclusive_range, tag(tstok!(CloseBrace)))), opt(tag(tstok!(Ident/Normal))))(ts)?
    };
    println!("{:?}", set_rp);
    let (ts, r#type) = opt(delimited(tag(tstok!(OpenParen)), tag(tstok!(Ident/Normal)), tag(tstok!(CloseParen))))(ts)?;
    println!("5");
    let (ts, id) = opt(delimited(tag(tstok!(OpenBracket)), tag(tstok!(l/Int/Any)), tag(tstok!(CloseBracket))))(ts)?;
    println!("6");

    Ok( (ts, ResourceDeclaration{
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
    let r = resource_declaration(ts);
    println!("tparser result: {:?}", r);
}