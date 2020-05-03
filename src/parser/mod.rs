pub mod resource;

use crate::lexer::token::{Token, TokenKind, TokenStream, Span};

use crate::lexer::{get_some_tokens};

use nom::bytes::complete::tag;

// fn tparser(ts: TokenStream) -> nom::IResult<TokenStream, bool> {
//     let (ts, x) = tag(TokenStream::new_with_slice(&[ Token{kind: tok!(+Plus), span: Span::any()} ]))(ts)?;
//
//     println!("ts: {:?}", ts);
//     println!("x: {:?}", x);
//
//     Err(nom::Err::Error((ts, nom::error::ErrorKind::Alpha)))
// }

pub fn parser_play() {
    // let tokens = get_some_tokens();
    // let ts = TokenStream::new(&tokens);
    //
    // println!("tparser called with: {:?}", ts);
    // let r = tparser(ts);
    // println!("tparser result: {:?}", r);
}