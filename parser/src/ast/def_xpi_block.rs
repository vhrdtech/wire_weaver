use super::prelude::*;
use crate::ast::expr::ExprParse;
use crate::ast::ty::TyParse;
use crate::error::{ParseError, ParseErrorKind};
use crate::warning::{ParseWarning, ParseWarningKind};
use ast::xpi_def::{AccessMode, XpiKind};
use ast::{TryEvaluateInto, TyKind, XpiDef};
use std::collections::HashMap;
use std::ops::Deref;

pub struct XpiDefParse(pub XpiDef);

pub struct UriSegmentSeedParse(pub ast::xpi_def::UriSegmentSeed);

struct XpiResourceTyParse {
    pub serial: Option<u32>,
    pub kind: XpiKind,
}

struct XpiBlockKeyValueParse {
    pub key: IdentifierParse<identifier::XpiKeyName>,
    pub value: ExprParse,
}

impl<'i> Parse<'i> for XpiDefParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::xpi_block)?, input);
        let doc: DocParse = input.parse()?;
        let attrs: AttrsParse = input.parse()?;
        let uri_segment: UriSegmentSeedParse = input.parse()?;
        let resource_ty: Option<XpiResourceTyParse> = input.parse_or_skip()?;
        let mut kv = HashMap::new();
        let mut implements = Vec::new();
        let mut children = Vec::new();
        if input.pairs.peek().is_some() {
            let mut input = ParseInput::fork(input.expect1(Rule::xpi_body)?, &mut input);

            while let Some(p) = input.pairs.peek() {
                match p.as_rule() {
                    Rule::xpi_field => {
                        let pair: XpiBlockKeyValueParse = input.parse()?;
                        kv.insert(pair.key.0, TryEvaluateInto::NotResolved(pair.value.0));
                    }
                    Rule::xpi_impl => {
                        let mut input =
                            ParseInput::fork(input.expect1(Rule::xpi_impl)?, &mut input);
                        let expr: ExprParse = input.parse()?;
                        implements.push(expr.0);
                    }
                    Rule::xpi_block => {
                        let def_xpi: XpiDefParse = input.parse()?;
                        children.push(def_xpi.0);
                    }
                    _ => {
                        return Err(ParseErrorSource::internal("unexpected xpi_body element"));
                    }
                }
            }
        }

        let (serial, kind) = resource_ty
            .map(|rt| (rt.serial, rt.kind))
            .unwrap_or((None, XpiKind::Group));
        Ok(XpiDefParse(XpiDef {
            doc: doc.0,
            attrs: attrs.0,
            uri_segment: uri_segment.0,
            serial,
            kind,
            kv,
            implements,
            children,
            span: input.span.clone(),
        }))
    }
}

impl<'i> Parse<'i> for UriSegmentSeedParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::xpi_uri_segment)?, input);

        let mut input_peek = input.pairs.clone();
        let (p1, p2, p3) = (input_peek.next(), input_peek.next(), input_peek.next());
        let ast_uri_seed = if p1.is_some() && p2.is_some() && p3.is_some() {
            let ident1: IdentifierParse<identifier::XpiUriSegmentName> = input.parse()?;
            let expr: ExprParse = input.parse()?;
            let ident2: IdentifierParse<identifier::XpiUriSegmentName> = input.parse()?;
            ast::xpi_def::UriSegmentSeed::Full(ident1.0, expr.0, ident2.0)
        } else if p1.is_some() && p2.is_some() {
            if p1.unwrap().as_rule() == Rule::identifier {
                let ident: IdentifierParse<identifier::XpiUriSegmentName> = input.parse()?;
                let expr: ExprParse = input.parse()?;
                ast::xpi_def::UriSegmentSeed::NamedPartThenExpr(ident.0, expr.0)
            } else {
                let expr: ExprParse = input.parse()?;
                let ident: IdentifierParse<identifier::XpiUriSegmentName> = input.parse()?;
                ast::xpi_def::UriSegmentSeed::ExprThenNamedPart(expr.0, ident.0)
            }
        } else if p1.is_some() {
            if p1.unwrap().as_rule() == Rule::identifier {
                let ident: IdentifierParse<identifier::XpiUriSegmentName> = input.parse()?;
                ast::xpi_def::UriSegmentSeed::Resolved(ident.0)
            } else {
                let expr: ExprParse = input.parse()?;
                ast::xpi_def::UriSegmentSeed::ExprOnly(expr.0)
            }
        } else {
            return Err(ParseErrorSource::internal("wrong xpi_uri_segment rule"));
        };
        Ok(UriSegmentSeedParse(ast_uri_seed))
    }
}

enum XpiResourceTyInner {
    Cell {
        transform: Option<XpiResourceTransform>,
        ty: TyParse,
    },
    Plain {
        transform: Option<XpiResourceTransform>,
        ty: TyParse,
    },
}

impl<'i> Parse<'i> for XpiResourceTyParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::xpi_resource_ty)?, input);
        let ty_inner = match input.pairs.peek() {
            Some(p) => match p.as_rule() {
                Rule::resource_cell_ty => {
                    let mut input =
                        ParseInput::fork(input.expect1(Rule::resource_cell_ty)?, &mut input);
                    Some(XpiResourceTyInner::Cell {
                        transform: input.parse_or_skip()?,
                        ty: input.parse()?,
                    })
                }
                Rule::xpi_resource_transform => {
                    let transform = input.parse()?;
                    if input.pairs.peek().expect("wrong grammar").as_rule()
                        == Rule::resource_cell_ty
                    {
                        input.errors.push(ParseError {
                            kind: ParseErrorKind::CellWithAccessModifier,
                            rule: p.as_rule(),
                            span: (p.as_span().start(), p.as_span().end()),
                        });
                        return Err(ParseErrorSource::UserError);
                    }
                    Some(XpiResourceTyInner::Plain {
                        transform: Some(transform),
                        ty: input.parse()?,
                    })
                }
                Rule::ty => Some(XpiResourceTyInner::Plain {
                    transform: None,
                    ty: input.parse()?,
                }),
                _ => None,
            },
            None => None,
        };
        let serial = match input.expect1(Rule::xpi_serial) {
            Ok(serial) => {
                let serial: u32 = serial
                    .clone()
                    .into_inner()
                    .next()
                    .ok_or_else(|| ParseErrorSource::internal("wrong xpi_serial"))?
                    .as_str()
                    .parse()
                    .map_err(|_| {
                        input.push_error(&serial, ParseErrorKind::IntParseError);
                        ParseErrorSource::UserError
                    })?;
                Some(serial)
            }
            Err(_) => None,
        };

        XpiResourceTyParse::from_ty_and_serial(
            ty_inner,
            serial,
            &mut input.warnings,
            &mut input.errors,
        )
    }
}

impl XpiResourceTyParse {
    fn from_ty_and_serial(
        ty_inner: Option<XpiResourceTyInner>,
        serial: Option<u32>,
        warnings: &mut Vec<ParseWarning>,
        errors: &mut Vec<ParseError>,
    ) -> Result<Self, ParseErrorSource> {
        match ty_inner {
            Some(XpiResourceTyInner::Plain { transform, ty }) => Ok(XpiResourceTyParse {
                serial,
                kind: Self::try_from_plain_ty(transform, ty, errors)?,
            }),
            Some(XpiResourceTyInner::Cell { transform, ty }) => Ok(XpiResourceTyParse {
                serial,
                kind: Self::try_from_cell_ty(transform, ty, warnings, errors)?,
            }),
            None => Ok(XpiResourceTyParse {
                serial,
                kind: XpiKind::Group,
            }),
        }
    }

    fn push_error(
        errors: &mut Vec<ParseError>,
        kind: ParseErrorKind,
        span: ast::Span,
    ) -> ParseErrorSource {
        errors.push(ParseError {
            kind,
            rule: Rule::xpi_resource_ty,
            span: (span.start, span.end),
        });
        ParseErrorSource::UserError
    }

    fn try_from_plain_ty(
        transform: Option<XpiResourceTransform>,
        ty: TyParse,
        errors: &mut Vec<ParseError>,
    ) -> Result<XpiKind, ParseErrorSource> {
        let access = transform.map(|t| t.access).unwrap_or(AccessMode::ImpliedRo);
        let modifier = transform.map(|t| t.modifier).flatten();
        match modifier {
            Some(m) => {
                if let TyKind::Fn { .. } = ty.0.kind {
                    return Err(Self::push_error(
                        errors,
                        ParseErrorKind::FnWithMods,
                        ty.0.span,
                    ));
                }
                match m {
                    XpiResourceModifier::Observe => {
                        if access == AccessMode::Const {
                            // const+observe
                            return Err(Self::push_error(
                                errors,
                                ParseErrorKind::ConstWithMods,
                                ty.0.span,
                            ));
                        }
                        if access == AccessMode::Wo {
                            // wo+observe
                            return Err(Self::push_error(
                                errors,
                                ParseErrorKind::WoObserve,
                                ty.0.span,
                            ));
                        }
                        Ok(XpiKind::Property {
                            access,
                            observable: true,
                            ty: ty.0,
                        })
                    }
                    XpiResourceModifier::Stream => match access {
                        AccessMode::ImpliedRo => {
                            return Err(Self::push_error(
                                errors,
                                ParseErrorKind::StreamWithoutDirection,
                                ty.0.span,
                            ));
                        }
                        AccessMode::Const => {
                            return Err(Self::push_error(
                                errors,
                                ParseErrorKind::ConstWithMods,
                                ty.0.span,
                            ));
                        }
                        _ => Ok(XpiKind::Stream {
                            dir: access,
                            ty: ty.0,
                        }),
                    },
                }
            }
            None => {
                if let TyKind::Fn { args, ret_ty } = ty.0.kind {
                    Ok(XpiKind::Method {
                        args,
                        ret_ty: ret_ty.deref().clone(),
                    })
                } else if let TyKind::Unit = ty.0.kind {
                    Ok(XpiKind::Group)
                } else if let TyKind::Derive = ty.0.kind {
                    Ok(XpiKind::Group)
                } else {
                    Ok(XpiKind::Property {
                        access,
                        observable: false,
                        ty: ty.0,
                    })
                }
            }
        }
    }

    fn try_from_cell_ty(
        transform: Option<XpiResourceTransform>,
        ty: TyParse,
        warnings: &mut Vec<ParseWarning>,
        errors: &mut Vec<ParseError>,
    ) -> Result<XpiKind, ParseErrorSource> {
        // by default resource inside a Cell is rw
        let transform = match transform {
            Some(t) => Some(t),
            None => Some(XpiResourceTransform {
                access: AccessMode::ImpliedRo,
                modifier: None,
            }),
        };
        let span = ty.0.span.clone();
        let inner = Self::try_from_plain_ty(transform, ty, errors)?;
        match &inner {
            XpiKind::Property { access, .. } => {
                if *access == AccessMode::Const || *access == AccessMode::Ro {
                    warnings.push(ParseWarning {
                        kind: ParseWarningKind::CellWithConstRo,
                        rule: Rule::xpi_resource_ty,
                        span: (span.start, span.end),
                    });
                    return Ok(inner);
                }
            }
            XpiKind::Stream { dir, .. } => {
                if *dir == AccessMode::Ro {
                    warnings.push(ParseWarning {
                        kind: ParseWarningKind::CellWithRoStream,
                        rule: Rule::xpi_resource_ty,
                        span: (span.start, span.end),
                    });
                    return Ok(inner);
                }
            }
            XpiKind::Method { .. } => {}

            XpiKind::Group | XpiKind::Array { .. } | XpiKind::Cell { .. } => unreachable!(),
        }
        Ok(XpiKind::Cell {
            inner: Box::new(inner),
        })
    }
}

#[derive(Copy, Clone)]
enum XpiResourceModifier {
    Observe,
    Stream,
}

#[derive(Copy, Clone)]
struct XpiResourceTransform {
    pub access: AccessMode,
    pub modifier: Option<XpiResourceModifier>,
}

impl<'i> Parse<'i> for XpiResourceTransform {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::xpi_resource_transform)?, input);
        let access = input.expect1(Rule::access_mode)?;
        let access = match access.as_str() {
            "const" => AccessMode::Const,
            "rw" => AccessMode::Rw,
            "ro" => AccessMode::Ro,
            "wo" => AccessMode::Wo,
            _ => {
                return Err(ParseErrorSource::internal("wrong access_mode rule"));
            }
        };
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

impl<'i> Parse<'i> for XpiBlockKeyValueParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::xpi_field)?, input);
        Ok(XpiBlockKeyValueParse {
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
//
// pub fn convert_from_parser(xd: XpiDefParser, is_root: bool) -> Result<Self, Error> {
//     let (serial, ty, span): (_, _, Span) = if is_root {
//         if xd.resource_ty.is_some() {
//             return Err(Error {
//                 kind: ErrorKind::RootWithTyOrSerial,
//                 span: xd.span.into(),
//             });
//         }
//         (u32::MAX, None, xd.span.into())
//     } else {
//         match xd.resource_ty {
//             Some(xty) => {
//                 (xty.serial.map(|s| s.0).ok_or(Error {
//                     kind: ErrorKind::NoSerial,
//                     span: xd.span.into(),
//                 })?, xty.ty, xty.span.into())
//             }
//             None => {
//                 return Err(Error {
//                     kind: ErrorKind::NoSerial,
//                     span: xd.span.into(),
//                 });
//             }
//         }
//     };
//     let kind = (ty, span.clone()).try_into()?;
//     let mut children = vec![];
//     for c in xd.body.children {
//         children.push(Self::convert_from_parser(c, false)?);
//     }
//     // let children: Result<Vec<XpiDef>, Error> = xd.body.children.iter().map(|c| XpiDef::try_from(c.clone())).collect();
//     // let children = children?;
//     Ok(XpiDef {
//         doc: xd.docs.into(),
//         attrs: xd.attrs.try_into()?,
//         uri_segment: xd.uri.into(),
//         serial,
//         kind,
//         kv: xd.body.kv_list
//             .iter()
//             .map(|kv|
//                 (
//                     kv.key.name.to_string(),
//                     TryEvaluateInto::NotResolved(kv.value.clone().into())
//                 )
//             ).collect(),
//         children,
//         span,
//     })
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
