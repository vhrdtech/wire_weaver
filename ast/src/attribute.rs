use std::fmt::{Debug, Display, Formatter};
use util::color;
// use mtoken::{TokenTree, TokenStream, Delimiter, Group, token::IdentFlavor};
// use mtoken::ext::TokenStreamExt;
use crate::{Expr, Span, Path};

#[derive(Clone, Eq, PartialEq)]
pub struct Attrs {
    pub attrs: Vec<Attr>,
    /// Element span to which attributes apply
    pub span: Span,
}

// #[derive(Copy, Clone, Debug)]
// pub enum Token {
//     Punct(char),
//     Ident(u32)
// }
//
// use peg::parser;
// peg::parser!{
//     grammar tokenparser() for [Token] {
//         rule punct_path() = [Token::Punct(':')] *<2>
//         pub rule path() -> Vec<Token> = x:[Token::Ident(_)] ** punct_path() { x }
//     }
// }
//
impl Attrs {
//     pub fn peg_test(&self) {
//         println!("{:?}", tokenparser::path(&[Token::Ident(0), Token::Punct(':'), Token::Punct(':'), Token::Ident(1), Token::Punct(':'), Token::Punct(':'), Token::Ident(10)]));
//     }

    /// Find attribute by name that is expected to be unique and return Ok with it, otherwise
    /// return Error::AttributeExpected or Error::AttributeMustBeUnique.
    pub fn get_unique(&self, path: Path) -> Option<AttrKind> {
        let mut attr = None;
        for a in &self.attrs {
            if a.path == path {
                if attr.is_some() {
                    return None;
                }
                attr = Some(a.kind.clone());
            }
        }
        attr
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Attr {
    pub path: Path,
    pub kind: AttrKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttrKind {
    Expr(Expr),
    TT(()),
}

impl AttrKind {
    pub fn expect_expr(&self) -> Option<Expr> {
        match self {
            AttrKind::Expr(expr) => Some(expr.clone()),
            _ => None
        }
    }
}

// impl<'i> TryFrom<AttrsParser<'i>> for Attrs {
//     type Error = Error;
//
//     fn try_from(attrs_parser: AttrsParser) -> Result<Self, Self::Error> {
//         let mut attrs = vec![];
//         for a in attrs_parser.attributes {
//             attrs.push(a.try_into()?);
//         }
//         Ok(Attrs { attrs, span: attrs_parser.span.into() })
//     }
// }
//
// impl<'i> TryFrom<AttrParser<'i>> for Attr {
//     type Error = Error;
//
//     fn try_from(attr: AttrParser<'i>) -> Result<Self, Self::Error> {
//         Ok(Attr {
//             path: attr.path.iter().map(|p| p.clone().into()).collect(),
//             kind: attr.kind.try_into()?,
//         })
//     }
// }
//
// impl<'i> TryFrom<AttrKindParser<'i>> for AttrKind {
//     type Error = Error;
//
//     fn try_from(attr_kind: AttrKindParser<'i>) -> Result<Self, Self::Error> {
//         match attr_kind {
//             AttrKindParser::TokenTree(p) => Ok(AttrKind::TT(parse_into_token_tree(p)?)),
//             AttrKindParser::Expression(e) => Ok(AttrKind::Expr(e.into()))
//         }
//     }
// }

// fn parse_into_token_tree(p: Pair<Rule>) -> Result<TokenTree, Error> {
//     let delim = match p.as_str().chars().next().unwrap() {
//         '(' => Delimiter::Parenthesis,
//         '{' => Delimiter::Brace,
//         '[' => Delimiter::Bracket,
//         _ => panic!("Wrong attribute grammar")
//     };
//     let ts = parse_delim_tt(p.into_inner())?;
//     Ok(TokenTree::Group(Group::new(delim, ts)))
// }
//
// fn parse_delim_tt(pairs: Pairs<Rule>) -> Result<TokenStream, Error> {
//     let mut ts = TokenStream::new();
//     for p in pairs {
//         match p.as_rule() {
//             Rule::token => {
//                 let token = p.into_inner().next().unwrap();
//                 match token.as_rule() {
//                     Rule::identifier => {
//                         let ident_lit = token.as_str().to_string();
//                         ts.append(mtoken::Ident::new(Rc::new(ident_lit), IdentFlavor::Plain));
//                     }
//                     Rule::lit => {
//                         todo!()
//                     }
//                     Rule::punctuation => {
//                         let punct: Vec<char> = token.as_str().chars().collect();
//                         for (i, ch) in punct.iter().enumerate() {
//                             let spacing = if i != punct.len() - 1 {
//                                 mtoken::Spacing::Joint
//                             } else {
//                                 mtoken::Spacing::Alone
//                             };
//                             ts.append(mtoken::Punct::new(*ch, spacing));
//                         }
//                     }
//                     _ => panic!("Wrong attribute grammar")
//                 }
//             }
//             Rule::delim_token_tree => {
//                 ts.append(parse_into_token_tree(p)?);
//             }
//             _ => panic!("Wrong attribute grammar")
//         }
//     }
//     Ok(ts)
// }

impl Display for Attr {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}#[{}", color::YELLOW, self.path)?;
        match &self.kind {
            AttrKind::TT(_) => write!(f, "~ TS")?,
            AttrKind::Expr(expr) => write!(f, "({})", expr)?,
        }
        write!(f, "{}]{}", color::YELLOW, color::DEFAULT)
    }
}

impl Display for Attrs {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        itertools::intersperse(
            self.attrs.iter().map(|attr| format!("{}", attr)),
            " ".to_owned(),
        ).try_for_each(|s| write!(f, "{}", s))?;
        Ok(())
    }
}

impl Debug for Attrs {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}