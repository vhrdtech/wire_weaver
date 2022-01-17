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
    let _language = match ts.next().unwrap() {
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
    eprintln!("\nParsing mquote str: {}", mquote_ts);
    let mquote_ts = MQuoteLexer::parse(Rule::token_stream, &mquote_ts).unwrap();
    eprintln!("Parsed: {:?}", mquote_ts);

    let mut ts_builder = new_ts_builder();
    for token in mquote_ts {
        tt_append(token, &mut ts_builder);
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

fn new_ts_builder() -> proc_macro2::TokenStream {
    quote! {
          let mut ts = mtoken::TokenStream::new();
    }
}

fn interpolate_path(token: pest::iterators::Pair<Rule>) -> proc_macro2::TokenStream {
    let segments = token.into_inner().map(|path_segment| {
        assert!(path_segment.as_rule() == Rule::interpolate_path_segment, "Internal error: {:?} found instead of interpolate_path_segment", path_segment);
        format_ident!("{}", path_segment.as_str())
    });
    quote! {
        #(#segments).*
    }
}

fn tt_append(token: pest::iterators::Pair<Rule>, ts_builder: &mut proc_macro2::TokenStream) {
    eprintln!("tt_append: {:?}", token);
    match token.as_rule() {
        Rule::delim_token_tree => {
            let delimiter = match token.as_str().chars().next().unwrap() {
                '(' => proc_macro2::Ident::new("Parenthesis", proc_macro2::Span::call_site()),
                '{' => proc_macro2::Ident::new("Brace", proc_macro2::Span::call_site()),
                '[' => proc_macro2::Ident::new("Bracket", proc_macro2::Span::call_site()),
                _ => panic!("Internal error, expected delimiter")
            };
            let mut delim_stream = new_ts_builder();
            for token in token.into_inner() {
                tt_append(token, &mut delim_stream);
            }
            ts_builder.append_all(quote! {
                let delim_stream = { #delim_stream ts };
                ts.append(mtoken::Group::new(mtoken::Delimiter::#delimiter, delim_stream));
            })
        },
        Rule::token => {
            tt_append(token.into_inner().next().unwrap(), ts_builder);
        },
        Rule::interpolate => {
            let path = interpolate_path(token);
            ts_builder.append_all(quote! {
                #path.to_tokens(&mut ts);
            });
        },
        Rule::interpolate_repetition => {
            let mut prefix_ts = new_ts_builder();
            let mut interpolate_or_key = None;
            let mut infix_ts = new_ts_builder();
            let mut interpolate_or_value = None;
            let mut postfix_ts = new_ts_builder();
            let mut separator = new_ts_builder();

            for interpolate_token in token.into_inner() {
                match interpolate_token.as_rule() {
                    Rule::token => {
                        let token = interpolate_token.into_inner().next().unwrap();
                        if interpolate_or_key.is_none() {
                            tt_append(token, &mut prefix_ts);
                        } else {
                            if interpolate_or_value.is_none() {
                                tt_append(token, &mut infix_ts);
                            } else {
                                tt_append(token, &mut postfix_ts);
                            }
                        }
                    },
                    Rule::interpolate => {
                        let mut interpolate_expr = interpolate_path(interpolate_token);
                        if interpolate_or_key.is_none() {
                            interpolate_or_key = Some(interpolate_expr);
                        } else {
                            interpolate_or_value = Some(interpolate_expr);
                        }
                    },
                    Rule::repetition_separator => {
                        tt_append(interpolate_token.into_inner().next().unwrap(), &mut separator);
                    },
                    _ => panic!("Internal error: unexpected token in interpolate repetition: {:?}", interpolate_token),
                }
            }
            eprintln!("{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}", prefix_ts, interpolate_or_key, infix_ts, interpolate_or_value, postfix_ts, separator);
            if interpolate_or_value.is_none() { // interpolate over iterator
                let interpolate_path_expr = interpolate_or_key.unwrap();
                ts_builder.append_all(quote! {
                    let prefix = { #prefix_ts ts };
                    let postfix = { #infix_ts ts };
                    let interpolate = #interpolate_path_expr.into_iter().map(|token_or_stream| {
                        let mut its = mtoken::TokenStream::new();
                        its.append_all(prefix.clone());
                        token_or_stream.to_tokens(&mut its);
                        its.append_all(postfix.clone());
                        its
                    });
                    let separator = { #separator ts };
                    ts.append_separated(interpolate, separator);
                });
            } else { // interpolate over key-value iterator
                todo!()
            }
        },
        Rule::ident => {
            let ident_lit = Literal::string(token.as_str());
            ts_builder.append_all(quote! {
                ts.append(mtoken::Ident::new(#ident_lit, mtoken::Span::call_site()));
            });
        },
        // Rule::ds_comment => {
        //
        // },
        // Rule::ts_comment => {
        //
        // },
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
        Rule::EOI => {},
        _ => panic!("Internal error: expected a token tree, got: {:?}", token),
    }
}

