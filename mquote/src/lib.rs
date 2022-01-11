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
use quote::__private::ext::RepToTokensExt;

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
        // eprintln!("{:?}", tt);
        match tt.as_rule() {
            Rule::delim_token_tree => {

            },
            Rule::token_except_delimiters => {
                tt_append(tt.into_inner().next().unwrap(), &mut ts_builder);
            },
            Rule::EOI => {},
            _ => panic!("Internal error: expected a token tree, got: {:?}", tt)
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

fn tt_append(token: pest::iterators::Pair<Rule>, ts_builder: &mut proc_macro2::TokenStream) {
    eprintln!("tt_append: {:?}", token);
    match token.as_rule() {
        Rule::interpolate => {
            let path = token.into_inner().map(|path_segment| {
                format_ident!("{}", path_segment.as_str())
            });
            ts_builder.append_all(quote! {
                ts.append_all(#(#path).*);
            });
        },
        Rule::interpolate_repetition => {

        },
        Rule::ident => {
            let ident_lit = Literal::string(token.as_str());
            ts_builder.append_all(quote! {
                ts.append(mtoken::Ident::new(#ident_lit, mtoken::Span::call_site()));
            })
        },
        Rule::ds_comment => {

        },
        Rule::ts_comment => {

        },
        Rule::punctuation => {
            let punct = token.as_str().to_string();
            if punct.len() == 1 {
                let punct_lit = Literal::character(punct.chars().next().unwrap());
                ts_builder.append_all(quote! {
                    ts.append(mtoken::Punct::new(#punct_lit, mtoken::Spacing::Alone));
                })
            } else if punct.len() == 2 {
                let punct_lit = Literal::character(punct.chars().next().unwrap());
                let punct_lit2 = Literal::character(punct.chars().skip(1).next().unwrap());
                ts_builder.append_all(quote! {
                    ts.append(mtoken::Punct::new(#punct_lit, mtoken::Spacing::Joint));
                    ts.append(mtoken::Punct::new(#punct_lit2, mtoken::Spacing::Alone));
                })
            }
        },
        Rule::literal => {

        },
        _ => panic!("Internal error: expected token or interpolation")
    }
}