use std::convert::{TryFrom, TryInto};
use std::rc::Rc;
use crate::ast::expr::Expr;
use mtoken::{TokenTree, TokenStream, Delimiter, Group, token::IdentFlavor};
use mtoken::ext::TokenStreamExt;
use crate::ast::identifier::Identifier;
use parser::ast::attrs::{Attr as AttrParser, Attrs as AttrsParser, AttrKind as AttrKindParser};
use parser::pest::iterators::{Pair, Pairs};
use crate::error::Error;
use parser::lexer::Rule;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Attrs(pub Vec<Attr>);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Attr {
    pub path: Vec<Identifier>,
    pub kind: AttrKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttrKind {
    TT(TokenTree),
    Expr(Expr),
}

impl<'i> TryFrom<AttrsParser<'i>> for Attrs {
    type Error = Error;

    fn try_from(attrs_parser: AttrsParser) -> Result<Self, Self::Error> {
        let mut attrs = vec![];
        for a in attrs_parser.attributes {
            attrs.push(a.try_into()?);
        }
        Ok(Attrs(attrs))
    }
}

impl<'i> TryFrom<AttrParser<'i>> for Attr {
    type Error = Error;

    fn try_from(attr: AttrParser<'i>) -> Result<Self, Self::Error> {
        Ok(Attr {
            path: attr.path.iter().map(|p| p.clone().into()).collect(),
            kind: attr.kind.try_into()?,
        })
    }
}

impl<'i> TryFrom<AttrKindParser<'i>> for AttrKind {
    type Error = Error;

    fn try_from(attr_kind: AttrKindParser<'i>) -> Result<Self, Self::Error> {
        match attr_kind {
            AttrKindParser::TokenTree(p) => Ok(AttrKind::TT(parse_into_token_tree(p)?)),
            AttrKindParser::Expression(e) => Ok(AttrKind::Expr(e.into()))
        }
    }
}

fn parse_into_token_tree(p: Pair<Rule>) -> Result<TokenTree, Error> {
    let delim = match p.as_str().chars().next().unwrap() {
        '(' => Delimiter::Parenthesis,
        '{' => Delimiter::Brace,
        '[' => Delimiter::Bracket,
        _ => panic!("Wrong attribute grammar")
    };
    let ts = parse_delim_tt(p.into_inner())?;
    Ok(TokenTree::Group(Group::new(delim, ts)))
}

fn parse_delim_tt(pairs: Pairs<Rule>) -> Result<TokenStream, Error> {
    let mut ts = TokenStream::new();
    for p in pairs {
        match p.as_rule() {
            Rule::token => {
                let token = p.into_inner().next().unwrap();
                match token.as_rule() {
                    Rule::identifier => {
                        let ident_lit = token.as_str().to_string();
                        ts.append(mtoken::Ident::new(Rc::new(ident_lit), IdentFlavor::Plain));
                    }
                    Rule::any_lit => {
                        todo!()
                    }
                    Rule::punctuation => {
                        let punct: Vec<char> = token.as_str().chars().collect();
                        for (i, ch) in punct.iter().enumerate() {
                            let spacing = if i != punct.len() - 1 {
                                mtoken::Spacing::Joint
                            } else {
                                mtoken::Spacing::Alone
                            };
                            ts.append(mtoken::Punct::new(*ch, spacing));
                        }
                    }
                    _ => panic!("Wrong attribute grammar")
                }
            }
            Rule::delim_token_tree => {
                ts.append(parse_into_token_tree(p)?);
            }
            _ => panic!("Wrong attribute grammar")
        }
    }
    Ok(ts)
}