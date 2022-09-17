mod lexer;

use quote::{format_ident, quote, TokenStreamExt};

use proc_macro2::{Ident, Literal, Span};

extern crate pest;
#[macro_use]
extern crate pest_derive;

extern crate proc_macro;
use proc_macro::{TokenStream, TokenTree};
use quote::__private::ext::RepToTokensExt;

use crate::pest::Parser;
use lexer::{MQuoteLexer, Rule};

#[derive(Copy, Clone, Eq, PartialEq)]
enum Language {
    Rust,
    Dart,
}

#[proc_macro]
pub fn mquote(ts: TokenStream) -> TokenStream {
    let mut ts = ts.into_iter();
    let language = match ts.next().unwrap() {
        TokenTree::Ident(ident) => match ident.to_string().as_str() {
            "rust" => Language::Rust,
            "dart" => Language::Dart,
            _ => panic!("Unsupported language: {}", ident),
        },
        _ => panic!("Expected language name"),
    };
    let mquote_ts = match ts.next().unwrap() {
        TokenTree::Literal(lit) => lit.to_string(),
        _ => panic!("Expected raw string literal with mtoken's"),
    };
    let debug = ts.next().is_some();

    // eprintln!("\nParsing mquote str: {}", mquote_ts);
    let mquote_ts = MQuoteLexer::parse(Rule::token_stream, &mquote_ts).unwrap();
    if debug {
        eprintln!("Parsed: {:?}", mquote_ts);
    }

    let mut ts_builder = new_ts_builder();
    ts_builder.append_all(quote! {
        let mut recreate_trees_afterwards = false;
    });
    for token in mquote_ts {
        tt_append(token, &mut ts_builder, language);
    }
    let print_ts_if_debug = if debug {
        quote! {
            println!("{:?}", ts);
        }
    } else {
        quote!()
    };
    let ts_builder = quote! {
        {
            #ts_builder
            if recreate_trees_afterwards {
                ts.recreate_trees();
            }
            #print_ts_if_debug
            ts
        }
    };
    if debug {
        eprintln!("TS builder: {}", ts_builder);
    }
    ts_builder.into()
}

fn new_ts_builder() -> proc_macro2::TokenStream {
    quote! {
        use std::rc::Rc;
        use mtoken::ext::TokenStreamExt;
        let mut ts = mtoken::TokenStream::new();
    }
}

fn interpolate_path(token: pest::iterators::Pair<Rule>) -> proc_macro2::TokenStream {
    let segments = token.into_inner().map(|path_segment| {
        assert!(
            path_segment.as_rule() == Rule::interpolate_path_segment,
            "Internal error: {:?} found instead of interpolate_path_segment",
            path_segment
        );
        format_ident!("{}", path_segment.as_str())
    });
    quote! {
        #(#segments).*
    }
}

fn ident_flavor(language: Language) -> Ident {
    match language {
        Language::Rust => Ident::new("RustAutoRaw", Span::call_site()),
        Language::Dart => Ident::new("DartAutoRaw", Span::call_site()),
    }
}

fn tt_append(
    token: pest::iterators::Pair<Rule>,
    ts_builder: &mut proc_macro2::TokenStream,
    language: Language,
) {
    // eprintln!("tt_append: {:?}", token);
    match token.as_rule() {
        Rule::delim_token_tree => {
            let delimiter = match token.as_str().chars().next().unwrap() {
                '(' => proc_macro2::Ident::new("Parenthesis", Span::call_site()),
                '{' => proc_macro2::Ident::new("Brace", Span::call_site()),
                '[' => proc_macro2::Ident::new("Bracket", Span::call_site()),
                _ => panic!("Internal error, expected delimiter"),
            };
            let mut delim_stream = new_ts_builder();
            for token in token.into_inner() {
                tt_append(token, &mut delim_stream, language);
            }
            ts_builder.append_all(quote! {
                let delim_stream = { #delim_stream ts };
                ts.append(mtoken::Group::new(mtoken::Delimiter::#delimiter, delim_stream));
            })
        }
        Rule::token => {
            tt_append(token.into_inner().next().unwrap(), ts_builder, language);
        }
        Rule::interpolate => {
            let path = interpolate_path(token);
            ts_builder.append_all(quote! {
                #path.to_tokens(&mut ts);
            });
        }
        Rule::interpolate_repetition => {
            let mut prefix_ts = new_ts_builder();
            let mut interpolate_or_key = None;
            let mut infix_ts = new_ts_builder();
            let mut interpolate_or_value = None;
            let mut postfix_ts = new_ts_builder();
            let mut separator = new_ts_builder();

            for interpolate_token in token.into_inner() {
                match interpolate_token.as_rule() {
                    Rule::token | Rule::COMMENT => {
                        let token = if interpolate_token.as_rule() == Rule::token {
                            interpolate_token.into_inner().next().unwrap()
                        } else {
                            interpolate_token
                        };
                        if interpolate_or_key.is_none() {
                            tt_append(token, &mut prefix_ts, language);
                        } else {
                            if interpolate_or_value.is_none() {
                                tt_append(token, &mut infix_ts, language);
                            } else {
                                tt_append(token, &mut postfix_ts, language);
                            }
                        }
                    }
                    Rule::interpolate => {
                        let interpolate_expr = interpolate_path(interpolate_token);
                        if interpolate_or_key.is_none() {
                            interpolate_or_key = Some(interpolate_expr);
                        } else {
                            interpolate_or_value = Some(interpolate_expr);
                        }
                    }
                    Rule::repetition_separator => {
                        tt_append(
                            interpolate_token.into_inner().next().unwrap(),
                            &mut separator,
                            language
                        );
                    }
                    _ => panic!(
                        "Internal error: unexpected token in interpolate repetition: {:?}",
                        interpolate_token
                    ),
                }
            }
            // eprintln!("{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}", prefix_ts, interpolate_or_key, infix_ts, interpolate_or_value, postfix_ts, separator);
            // #( #fields ),*
            if interpolate_or_value.is_none() {
                // interpolate over iterator
                let interpolate_iter1 = interpolate_or_key.unwrap();
                ts_builder.append_all(quote! {
                    let prefix = { #prefix_ts ts };
                    let postfix = { #infix_ts ts };
                    let interpolate = #interpolate_iter1.into_iter().map(|token_or_stream| {
                        let mut its = mtoken::TokenStream::new();
                        its.append_all(prefix.clone());
                        token_or_stream.to_tokens(&mut its);
                        its.append_all(postfix.clone());
                        its
                    });
                    let separator = { #separator ts };
                    ts.append_separated(interpolate, separator);
                });
            } else {
                // #( #field_ser_methods( self.#field_names )?; )*
                let interpolate_iter1 = interpolate_or_key.unwrap();
                let interpolate_iter2 = interpolate_or_value.unwrap();
                ts_builder.append_all(quote! {
                    let prefix = { #prefix_ts ts };
                    let infix = { #infix_ts ts };
                    let postfix = { #postfix_ts ts };
                    let interpolate = #interpolate_iter1.into_iter()
                        .zip(#interpolate_iter2.into_iter())
                        .map(|(token_or_stream_1, token_or_stream_2)| {
                            let mut its = mtoken::TokenStream::new();
                            its.append_all(prefix.clone());
                            token_or_stream_1.to_tokens(&mut its);
                            its.append_all(infix.clone());
                            token_or_stream_2.to_tokens(&mut its);
                            its.append_all(postfix.clone());
                            its
                        });
                    let separator = { #separator ts };
                    ts.append_separated(interpolate, separator);
                });
            }
        }
        Rule::ident => {
            let cancel_auto_raw = token.as_str().chars().next().unwrap() == 'ȸ';
            let (ident_lit, flavor) = if cancel_auto_raw {
                let ident_skip: String = token.as_str().chars().skip(1).collect();
                (Literal::string(ident_skip.as_str()), Ident::new("Plain", Span::call_site()))
            } else {
                (Literal::string(token.as_str()), ident_flavor(language))
            };
            ts_builder.append_all(quote! {
                ts.append(
                    mtoken::Ident::new(
                        Rc::new(#ident_lit.to_owned()),
                        mtoken::token::IdentFlavor::#flavor,
                        //Span::call_site()
                    )
                );
            });
        }
        // Rule::ds_comment => {
        //
        // },
        // Rule::ts_comment => {
        //
        // },
        Rule::punctuation => {
            let punct: Vec<char> = token.as_str().chars().collect();
            match punct.len() {
                1 => {
                    let ch = punct[0];
                    // handle 'lifetime for Rust
                    let spacing = if language == Language::Rust && ch == '\'' {
                        Ident::new("Joint", Span::call_site())
                    } else {
                        Ident::new("Alone", Span::call_site())
                    };
                    let punct_lit = Literal::character(ch);
                    ts_builder.append_all(quote! {
                        ts.append(mtoken::Punct::new(#punct_lit, mtoken::Spacing::#spacing));
                    })
                }
                2 => {
                    let punct_lit = Literal::character(punct[0]);
                    let punct_lit2 = Literal::character(punct[1]);
                    ts_builder.append_all(quote! {
                        ts.append(mtoken::Punct::new(#punct_lit, mtoken::Spacing::Joint));
                        ts.append(mtoken::Punct::new(#punct_lit2, mtoken::Spacing::Alone));
                    })
                }
                3 => {
                    let punct_lit = Literal::character(punct[0]);
                    let punct_lit2 = Literal::character(punct[1]);
                    let punct_lit3 = Literal::character(punct[2]);
                    ts_builder.append_all(quote! {
                        ts.append(mtoken::Punct::new(#punct_lit, mtoken::Spacing::Joint));
                        ts.append(mtoken::Punct::new(#punct_lit2, mtoken::Spacing::Joint));
                        ts.append(mtoken::Punct::new(#punct_lit3, mtoken::Spacing::Alone));
                    })
                }
                _ => panic!("internal: Up to 3 chars in a punct is supported")
            }
        }
        Rule::delimiter => {
            let delim = match token.as_str() {
                "\\\\(" => "ParenOpen",
                "\\\\)" => "ParenClose",
                "\\\\{" => "BraceOpen",
                "\\\\}" => "BraceClose",
                "\\\\[" => "BracketOpen",
                "\\\\]" => "BracketClose",
                _ => panic!("Unexpected delimiter"),
            };
            let delim = Ident::new(delim, Span::call_site());
            ts_builder.append_all(quote! {
                ts.append(mtoken::token::DelimiterRaw::#delim);
                recreate_trees_afterwards = true;
            })
        }
        Rule::literal => {
            let repr = Literal::string(token.as_str());
            ts_builder.append_all(quote! {
                ts.append(
                    mtoken::Literal::new(#repr.to_string())
                );
            });
        }
        Rule::spacing_joint => {
            ts_builder.append_all(quote! {
                ts.modify_last_spacing(mtoken::Spacing::Joint);
            })
        }
        Rule::COMMENT => {
            let comment_style = token.into_inner().next().expect("internal: Wrong comment grammar");
            let flavor = match comment_style.as_rule() {
                Rule::doc_comment => {
                    match language {
                        Language::Rust | Language::Dart => "TripleSlash",
                    }
                }
                Rule::single_line_comment => {
                    match language {
                        Language::Rust | Language::Dart => "DoubleSlash",
                    }
                }
                Rule::multi_line_comment => {
                    match language {
                        Language::Rust | Language::Dart => "SlashStarMultiline",
                    }
                }
                _ => panic!("internal: Wrong comment grammar kinds")
            };
            let contents = comment_style
                .into_inner()
                .next()
                .expect("internal: Wrong comment grammar (no _inner)")
                .as_str();
            let contents = Literal::string(contents);
            let flavor = Ident::new(flavor, Span::call_site());
            ts_builder.append_all(quote! {
                ts.append(
                    mtoken::Comment::new(#contents, mtoken::CommentFlavor::#flavor)
                );
            });
        }
        Rule::EOI => {}
        _ => panic!("Internal error: expected a token tree, got: {:?}", token),
    }
}
