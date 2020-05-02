pub mod resource;

use crate::lexer::token::{*};
use crate::lexer::{get_some_tokens};
use nom::bytes::complete::tag;

macro_rules! tok {
    (+$bin_op:ident) => { TokenKind::BinOp(BinOpToken::$bin_op) };
    (&&$bool_op:ident) => { TokenKind::BoolOp(BoolOpToken::$bool_op) };
    (!$unary_op:ident) => { TokenKind::UnaryOp(UnaryOpToken::$unary_op) };
    (<$unary_op:ident) => { TokenKind::OpenDelim(DelimToken::$unary_op) };
    (>$unary_op:ident) => { TokenKind::CloseDelim(DelimToken::$unary_op) };
}

macro_rules! ts {
    ($($t1:tt$t2:tt),+) => {
        TokenStream::new_with_slice(&[ $(Token{kind: tok!($t1$t2), span: Span::any()}),* ])
    };
}

fn tparser(ts: TokenStream) -> nom::IResult<TokenStream, bool> {
    let (ts, x) = tag(TokenStream::new_with_slice(&[ Token{kind: tok!(+Plus), span: Span::any()} ]))(ts)?;

    println!("ts: {:?}", ts);
    println!("x: {:?}", x);

    Err(nom::Err::Error((ts, nom::error::ErrorKind::Alpha)))
}

pub fn parser_play() {
    println!("{:?}", ts!(&&Le, <Bracket, >Brace));
    // let tokens = get_some_tokens();
    // let ts = TokenStream::new(&tokens);
    //
    // println!("tparser called with: {:?}", ts);
    // let r = tparser(ts);
    // println!("tparser result: {:?}", r);
}