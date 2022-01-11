mod lexer;

extern crate pest;
#[macro_use]
extern crate pest_derive;

extern crate proc_macro;
use proc_macro::{TokenTree, TokenStream};

use crate::pest::Parser;
use lexer::{Rule, MQuoteLexer};

#[proc_macro]
pub fn mquote(ts: TokenStream) -> TokenStream {
    let mut ts = ts.into_iter();
    let language = match ts.next().unwrap() {
        TokenTree::Ident(ident) => {
            ident.to_string()
        },
        _ => panic!("Expected language name")
    };
    let mquote_ts = match ts.next().unwrap() {
        TokenTree::Literal(lit) => {
            lit.to_string()
        },
        _ => panic!("Expected raw string literal with mtoken's")
    };
    let mquote_ts = MQuoteLexer::parse(Rule::token_stream, &mquote_ts).unwrap();
    panic!("{:?} {:?}", language, mquote_ts);
    "{ let mut ts = mtoken::TokenStream::new(); ts.append(mtoken::Ident::new(\"x\", mtoken::Span::call_site())); ts }".parse().unwrap()
}
