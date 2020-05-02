pub mod resource;

use crate::lexer::token::{Token, TokenStream, TokenKind, BoolOpToken, Span};
use crate::lexer::{get_some_tokens, get_some_more};
use nom::bytes::complete::tag;

macro_rules! ts {
    ($a:ident) => {
        TokenStream::new_with_slice(&[Token{kind: TokenKind::$a, span: Span::any()}])
    }
}

fn tparser(ts: TokenStream) -> nom::IResult<TokenStream, bool> {
    let more_toks = get_some_more();
    let ts_tag = TokenStream::new(&more_toks);

    println!("ts_tag is: {:?}", ts_tag);

    let (ts, x) = tag(ts!(At))(ts)?;

    println!("ts: {:?}", ts);
    println!("x: {:?}", x);

    Err(nom::Err::Error((ts, nom::error::ErrorKind::Alpha)))
}

pub fn parser_play() {
    let tokens = get_some_tokens();
    let ts = TokenStream::new(&tokens);
    let r = tparser(ts);
    println!("tparser result: {:?}", r);
}