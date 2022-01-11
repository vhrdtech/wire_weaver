mod lexer;

use quote::{quote, TokenStreamExt, format_ident};

use proc_macro2::Literal;

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

    let mut ts_builder = quote! {
          let mut ts = mtoken::TokenStream::new();
    };

    for tt in mquote_ts {
        eprintln!("{:?}", tt);
        match tt.as_rule() {
            Rule::delim_token_tree => {

            },
            Rule::interpolate => {

            },
            Rule::interpolate_repetition => {

            },
            Rule::ident => {
                let ident_lit = Literal::string(tt.as_str());
                ts_builder.append_all(quote! {
                    ts.append(mtoken::Ident::new(#ident_lit, mtoken::Span::call_site()));
                })
            },
            Rule::ds_comment => {

            },
            Rule::ts_comment => {

            },
            Rule::punctuation => {

            },
            Rule::literal => {

            },
            _ => {}
        }
    }

    let ts_builder = quote! {
        {
            #ts_builder
            ts
        }
    };
    eprintln!("TS builder: {}", ts_builder);
    ts_builder.into()
}
