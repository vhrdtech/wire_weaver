use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use crate::ast::doc::Doc;
use crate::ast::expr::{Expr, TryEvaluateInto};
use crate::ast::identifier::Identifier;
use parser::ast::def_xpi_block::{AccessMode, XpiResourceTransform, XpiUri as XpiUriParser, DefXpiBlock as XpiDefParser, XpiCellTy, XpiPlainTy, XpiResourceModifier};
use crate::ast::fn_def::FnArguments;
use crate::ast::lit::Lit;
use crate::ast::ty::Ty;
use crate::error::{Error, ErrorKind};
use either::Either;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XpiDef {
    pub doc: Doc,
    // pub attrs: Attrs,
    pub uri: XpiUri,
    pub serial: u32,
    pub kind: XpiKind,
    pub kv: HashMap<String, TryEvaluateInto<Lit>>,
    // pub implements: Vec<>,
    pub children: Vec<XpiDef>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XpiKind {
    /// Resource without a type is a group, like `/main {}`, used to group things in a logical manner.
    /// Any other resource is also implicitly a group.
    Group,
    /// Similar resources can be put into an array and accessed by index.
    /// In contrast with interpolated resources, only one array of resources is created.
    /// Resource with a type `[_; numbound]`, like `/channels<[_; 4]> {}`.
    /// Note that regular arrays are XpiKind::Property, for example `/arr<[u8; 4]>`.
    Array,

    // Constant with a value defined when a node is starting, must not change afterwards.
    // `/channel_count<const u8>`
    //Const,
    /// Any type can be a property, read only by default.
    /// `/ro_property<u8>` or `/ro_explicit_property<ro u8>` or `/write_only<wo u8>` or `/read_write<rw u8>`
    /// `+observe` modifier can be added to add support for notifications on value changes (ro or rw).
    Property {
        access: AccessMode,
        observable: bool,
        ty: Ty,
    },
    /// Streams can be opened or closed, have a start and possibly an end.
    /// Auto wrapped in Cell? <-> mismatch with a property or a method, can also lead to race conditions
    /// `/file_contents<ro+stream [u8; ?]>` or `/firmware<wo+stream [u8; max 128]>`.
    /// `/bidirectional<rw+stream>` - might be usable in some contexts?
    /// `u8` or `[u8; ?]` for buffers or both make sense - ?
    Stream,
    /// `/borrowable_group<Cell<_>> { /child<rw u8> }`
    /// `/borrowable_property<Cell<u8>>` - implicitly rw, otherwise no reason for a Cell
    /// `/write_only_cell<Cell< wo u8> >>`
    /// `/borrowable_stream<Cell< ro+stream u8 >>`
    Cell,
    /// Callable method. `/method<fn ()>`, `/with_args_and_ret<fn (x: u8) -> u8>`
    Method {
        args: FnArguments,
        ret_ty: Ty,
    },

    /// Not yet known kind (type alias or generic type used), can be Property, Cell or Method
    Generic {
        transform: XpiResourceTransform,
        ty: Ty,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XpiUri {
    /// Ready to use resource identifier.
    /// OneNamedPart is already Resolved, other variants need expression resolving pass.
    /// `/main`, `a_ctrl`, `velocity_x`, `register_0_b`
    Resolved(Identifier),
    /// `\`get_names()\``
    ExprOnly(Expr),
    /// /\`'a'..'c'\`_ctrl
    ExprThenNamedPart(Expr, Identifier),
    /// /velocity_\`'x'..'z'\`
    NamedPartThenExpr(Identifier, Expr),
    /// /register_\`'0'..'9'\`_b
    Full(Identifier, Expr, Identifier),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XpiResourceTy {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XpiBody {}

impl<'i> From<XpiUriParser<'i>> for XpiUri {
    fn from(uri: XpiUriParser<'i>) -> Self {
        match uri {
            XpiUriParser::OneNamedPart(id) => XpiUri::Resolved(id.into()),
            XpiUriParser::ExprOnly(expr) => XpiUri::ExprOnly(expr.into()),
            XpiUriParser::ExprThenNamedPart(expr, id) => {
                XpiUri::ExprThenNamedPart(expr.into(), id.into())
            }
            XpiUriParser::NamedPartThenExpr(id, expr) => {
                XpiUri::NamedPartThenExpr(id.into(), expr.into())
            }
            XpiUriParser::Full(id1, expr, id2) => XpiUri::Full(id1.into(), expr.into(), id2.into()),
        }
    }
}

impl<'i> TryFrom<XpiDefParser<'i>> for XpiDef {
    type Error = Error;

    fn try_from(xd: XpiDefParser<'i>) -> Result<Self, Self::Error> {
        let (serial, ty, span) = match xd.resource_ty {
            Some(xty) => {
                (xty.serial.map(|s| s.0).ok_or(Error {
                    kind: ErrorKind::NoSerial,
                    span: xd.span.into(),
                })?, xty.ty, xty.span)
            }
            None => {
                return Err(Error {
                    kind: ErrorKind::NoSerial,
                    span: xd.span.into(),
                });
            }
        };
        let kind = (ty, span).try_into()?;
        let mut children = vec![];
        for c in xd.body.children {
            children.push(c.try_into()?);
        }
        // let children: Result<Vec<XpiDef>, Error> = xd.body.children.iter().map(|c| XpiDef::try_from(c.clone())).collect();
        // let children = children?;
        Ok(XpiDef {
            doc: xd.docs.into(),
            uri: xd.uri.into(),
            serial,
            kind,
            kv: xd.body.kv_list
                .iter()
                .map(|kv|
                    (
                        kv.key.name.to_string(),
                        TryEvaluateInto::NotResolved(kv.value.clone().into())
                    )
                ).collect(),
            children
        })
    }
}

impl<'i> TryFrom<( Option<Either<XpiCellTy<'i>, XpiPlainTy<'i>>>, parser::pest::Span<'i> )> for XpiKind {
    type Error = Error;

    fn try_from(ty: ( Option<Either<XpiCellTy<'i>, XpiPlainTy<'i>>>, parser::pest::Span<'i> )) -> Result<Self, Self::Error> {
        match ty.0 {
            Some(Either::Right(plain_ty)) => {
                let access = plain_ty.0.map(|t| t.access).unwrap_or(AccessMode::Ro);
                let modifier = plain_ty.0.map(|t| t.modifier).flatten();
                match modifier {
                    Some(m) => {
                        match m {
                            XpiResourceModifier::Observe => {
                                Ok(XpiKind::Property {
                                    access,
                                    observable: true,
                                    ty: plain_ty.1.into()
                                })
                            },
                            XpiResourceModifier::Stream => {
                                Ok(XpiKind::Stream {
                                    // access,
                                    // observable: false,
                                    // ty: plain_ty.1.into()
                                })
                            }
                        }
                    },
                    None => {
                        Ok(XpiKind::Property {
                            access,
                            observable: false,
                            ty: plain_ty.1.into()
                        })
                    }
                }
            }
            Some(Either::Left(_cell_ty)) => {
                todo!()
            }
            None => {
                Ok(XpiKind::Group)
            }
        }
    }
}