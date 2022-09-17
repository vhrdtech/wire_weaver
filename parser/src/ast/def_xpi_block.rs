use super::prelude::*;
use crate::ast::expr::Expr;
use crate::ast::naming::{XpiKeyName, XpiUriSegmentName};
use crate::ast::ty::Ty;
use crate::error::{ParseError, ParseErrorKind};
use either::Either;
use std::fmt::{Debug, Formatter};
use pest::Span;

// macro_rules! function {
//     () => {{
//         fn f() {}
//         fn type_name_of<T>(_: T) -> &'static str {
//             std::any::type_name::<T>()
//         }
//         let name = type_name_of(f);
//         &name[..name.len() - 3]
//     }}
// }

#[derive(Clone)]
pub struct DefXpiBlock<'i> {
    pub docs: Doc<'i>,
    pub attrs: Attrs<'i>,
    pub uri: XpiUri<'i>,
    pub resource_ty: Option<XpiResourceTy<'i>>,
    pub body: XpiBody<'i>,
    pub span: Span<'i>,
}

impl<'i> Parse<'i> for DefXpiBlock<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        // dbg!(function!());
        // dbg!("xpi_block parse");
        // crate::util::pest_print_tree(input.pairs.clone());
        let mut input = ParseInput::fork(input.expect1(Rule::xpi_block)?, input);
        Ok(DefXpiBlock {
            docs: input.parse()?,
            attrs: input.parse()?,
            uri: input.parse()?,
            resource_ty: input.parse_or_skip()?,
            body: input.parse()?,
            span: input.span,
        })
    }
}

impl<'i> Debug for DefXpiBlock<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "\n\x1b[32m{}\x1b[33m{:?}\n\x1b[36mDefXpiBlock(uri: {:?} ty: {:?})\x1b[0m",
            self.docs, self.attrs, self.uri, self.resource_ty
        )?;
        writeln!(f, "{:?}", self.body)
    }
}

#[derive(Debug, Clone)]
pub struct XpiResourceTy<'i> {
    pub ty: Option<Either<XpiCellTy<'i>, XpiPlainTy<'i>>>,
    pub serial: Option<XpiSerial>,
    pub span: Span<'i>,
}

#[derive(Debug, Clone)]
pub struct XpiCellTy<'i>(pub Option<XpiResourceTransform>, pub Ty<'i>);

#[derive(Debug, Clone)]
pub struct XpiPlainTy<'i>(pub Option<XpiResourceTransform>, pub Ty<'i>);

impl<'i> Parse<'i> for XpiResourceTy<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::xpi_resource_ty)?, input);
        let ty = match input.pairs.peek() {
            Some(p) => match p.as_rule() {
                Rule::resource_cell_ty => {
                    let mut input =
                        ParseInput::fork(input.expect1(Rule::resource_cell_ty)?, &mut input);
                    let transform = input.parse_or_skip()?;
                    let ty = input.parse()?;
                    Some(Either::Left(XpiCellTy(transform, ty)))
                }
                Rule::xpi_resource_transform => {
                    let transform = input.parse()?;
                    if input.pairs.peek().expect("wrong grammar").as_rule() == Rule::resource_cell_ty {
                        input.errors.push(ParseError {
                            kind: ParseErrorKind::CellWithAccessModifier,
                            rule: p.as_rule(),
                            span: (p.as_span().start(), p.as_span().end())
                        });
                        return Err(ParseErrorSource::UserError);
                    }
                    let ty = input.parse()?;
                    Some(Either::Right(XpiPlainTy(Some(transform), ty)))
                }
                Rule::any_ty => Some(Either::Right(XpiPlainTy(None, input.parse()?))),
                _ => None,
            },
            None => None,
        };

        Ok(XpiResourceTy {
            ty,
            serial: input.parse_or_skip()?,
            span: input.span
        })
    }
}

#[derive(Debug, Default, Clone)]
pub struct XpiBody<'i> {
    pub kv_list: Vec<XpiBlockKeyValue<'i>>,
    pub implements: Vec<Expr<'i>>,
    pub children: Vec<DefXpiBlock<'i>>,
}

impl<'i> Parse<'i> for XpiBody<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        if input.pairs.peek().is_none() {
            return Ok(XpiBody::default());
        }
        let mut input = ParseInput::fork(input.expect1(Rule::xpi_body)?, input);
        let mut kv_list = Vec::new();
        let mut implements = Vec::new();
        let mut children = Vec::new();

        while let Some(p) = input.pairs.peek() {
            match p.as_rule() {
                Rule::xpi_field => {
                    kv_list.push(input.parse()?);
                }
                Rule::xpi_impl => {
                    let mut input = ParseInput::fork(input.expect1(Rule::xpi_impl)?, &mut input);
                    implements.push(input.parse()?);
                }
                Rule::xpi_block => {
                    children.push(input.parse()?);
                }
                _ => {
                    return Err(ParseErrorSource::internal("unexpected xpi_body element"));
                }
            }
        }

        Ok(XpiBody {
            kv_list,
            implements,
            children,
        })
    }
}

#[derive(Debug, Clone)]
pub enum XpiUri<'i> {
    /// `/main`
    OneNamedPart(Identifier<'i, XpiUriSegmentName>),
    /// `\`get_names()\``
    ExprOnly(Expr<'i>),
    /// /\`'a'..'c'\`_ctrl
    ExprThenNamedPart(Expr<'i>, Identifier<'i, XpiUriSegmentName>),
    /// /velocity_\`'x'..'z'\`
    NamedPartThenExpr(Identifier<'i, XpiUriSegmentName>, Expr<'i>),
    /// /register_\`'0'..'9'\`_b
    Full(
        Identifier<'i, XpiUriSegmentName>,
        Expr<'i>,
        Identifier<'i, XpiUriSegmentName>,
    ),
}

impl<'i> Parse<'i> for XpiUri<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        // dbg!(function!());
        let mut input = ParseInput::fork(input.expect1(Rule::xpi_uri_segment)?, input);

        let mut input_peek = input.pairs.clone();
        let (p1, p2, p3) = (input_peek.next(), input_peek.next(), input_peek.next());
        if p1.is_some() && p2.is_some() && p3.is_some() {
            Ok(XpiUri::Full(input.parse()?, input.parse()?, input.parse()?))
        } else if p1.is_some() && p2.is_some() {
            if p1.unwrap().as_rule() == Rule::identifier {
                Ok(XpiUri::NamedPartThenExpr(input.parse()?, input.parse()?))
            } else {
                Ok(XpiUri::ExprThenNamedPart(input.parse()?, input.parse()?))
            }
        } else if p1.is_some() {
            if p1.unwrap().as_rule() == Rule::identifier {
                Ok(XpiUri::OneNamedPart(input.parse()?))
            } else {
                Ok(XpiUri::ExprOnly(input.parse()?))
            }
        } else {
            Err(ParseErrorSource::internal("wrong xpi_uri rule"))
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AccessMode {
    Rw,
    Ro,
    Wo,
    Const,
}

impl<'i> Parse<'i> for AccessMode {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let access_mode = input.expect1(Rule::access_mode)?;
        match access_mode.as_str() {
            "rw" => Ok(AccessMode::Rw),
            "ro" => Ok(AccessMode::Ro),
            "wo" => Ok(AccessMode::Wo),
            "const" => Ok(AccessMode::Const),
            _ => Err(ParseErrorSource::internal("wrong access_mod rule")),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum XpiResourceModifier {
    Observe,
    Stream,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct XpiResourceTransform {
    pub access: AccessMode,
    pub modifier: Option<XpiResourceModifier>,
}

impl<'i> Parse<'i> for XpiResourceTransform {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::xpi_resource_transform)?, input);
        let access = input.parse()?;
        let modifier = input.pairs.next().map(|p| {
            if p.as_rule() == Rule::mod_stream {
                XpiResourceModifier::Stream
            } else {
                XpiResourceModifier::Observe
            }
        });
        Ok(XpiResourceTransform { access, modifier })
    }
}

// #[derive(Debug, Clone)]
// pub struct XpiBlockType<'i>(pub Option<Ty<'i>>);
//
// impl<'i> Parse<'i> for XpiBlockType<'i> {
//     fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
//         // dbg!(function!());
//         Ok(XpiBlockType(input.parse_or_skip()?))
//     }
// }

#[derive(Debug, Clone)]
pub struct XpiSerial(pub u32);

impl<'i> Parse<'i> for XpiSerial {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        // dbg!(function!());
        let xpi_serial = input.expect1(Rule::xpi_serial)?;
        Ok(XpiSerial(
            xpi_serial
                .as_str()
                .strip_prefix('#')
                .ok_or_else(|| ParseErrorSource::internal("xpi_serial: wrong rule"))?
                .parse()
                .map_err(|_| {
                    input.push_error(&xpi_serial, ParseErrorKind::IntParseError);
                    ParseErrorSource::UserError
                })?,
        ))
    }
}

#[derive(Debug, Clone)]
pub struct XpiBlockKeyValue<'i> {
    pub key: Identifier<'i, XpiKeyName>,
    pub value: Expr<'i>,
}

impl<'i> Parse<'i> for XpiBlockKeyValue<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::xpi_field)?, input);
        Ok(XpiBlockKeyValue {
            key: input.parse()?,
            value: input.parse()?,
        })
    }
}

// #[derive(Debug, Clone)]
// pub enum XpiValue<'i> {
//     Stmt(Stmt<'i>),
//     Expr(Expr<'i>),
// }
//
// impl<'i> Parse<'i> for XpiValue<'i> {
//     fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
//         let try_stmt: Option<Stmt<'i>> = input.parse_or_skip()?;
//         match try_stmt {
//             Some(stmt) => Ok(XpiValue::Stmt(stmt)),
//             None => Ok(XpiValue::Expr(input.parse()?)),
//         }
//     }
// }

#[cfg(test)]
mod test {
    use super::DefXpiBlock;
    use crate::ast::test::parse_str;
    use crate::lexer::Rule;

    #[test]
    fn impl_interface() {
        let xpi: DefXpiBlock = parse_str("/main{ impl log::#/full; }", Rule::xpi_block);
        assert_eq!(xpi.body.implements.len(), 1);
    }
}
