use crate::ast::doc::Doc;
use crate::ast::expr::Expr;
use crate::ast::identifier::Identifier;
use parser::ast::def_xpi_block::{XpiBody as XpiBodyParser, XpiResourceAccessMode, XpiResourceTy as XpiResourceTyParser, XpiUri as XpiUriParser};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XpiDef {
    pub doc: Doc,
    pub uri: XpiUri,
    pub access: Option<XpiResourceAccessMode>,
    // pub ty:
    // pub ty: Option<XpiResourceTy>,
    // pub body: XpiBody,
}

pub enum XpiKind {
    /// Resource without a type is a group, like `/main {}`
    Group,
    /// Similar resources can be put into an array and accessed by index.
    /// In contrast with interpolated resources, only one array of resources is created.
    /// Resource with a type `[_; numbound]`, like `/channels<[_; 4]> {}`
    Array,
    /// Constant with a value defined when a node is starting, must not change afterwards.
    /// `/channel_count<const u8>`
    Const,
    /// Any type can be a property, read only by default.
    /// `/ro_property<u8>` or `/ro_explicit_property<ro u8>` or `/write_only<wo u8>` or `/read_write<rw u8>`
    /// `+observe` modifier can be added to add support for notifications on value changes (ro or rw).
    Property,
    /// Streams can be opened or closed, have a start and possibly an end.
    /// Auto wrapped in Cell? <-> mismatch with a property or a method, can also lead to race conditions
    /// `/file_contents<ro+stream [u8; ?]>` or `/firmware<wo+stream [u8; max 128]>`.
    /// `/bidirectional<rw+stream>` - might be usable in some contexts?
    /// `u8` or `[u8; ?]` for buffers or both make sense - ?
    Stream,
    /// `/borrowable_group<Cell<_>> { /child<rw u8> }`
    /// `/borrowable_property<Cell<u8>>` - implicitly rw, otherwise no reason for a Cell
    /// `/borrowable_stream<Cell< stream<ro, u8> >>` - change modifiers to types to make more consistent syntax
    Cell,
    /// Callable method. `/method<fn ()>`, `/with_args_and_ret<fn (u8) -> u8>`
    Method,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XpiUri {
    /// Ready to use resource identifier.
    /// OneNamedPart is already Resolved, other variants need expression resolving pass.
    /// `/main`, `a_ctrl`, `velocity_x`, `register_0_b`
    Resolved(Identifier),
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
            XpiUriParser::ExprThenNamedPart(expr, id) => {
                XpiUri::ExprThenNamedPart(expr.into(), id.into())
            }
            XpiUriParser::NamedPartThenExpr(id, expr) => {
                XpiUri::NamedPartThenExpr(id.into(), expr.into())
            }
            XpiUriParser::Full(id1, expr, id2) => {
                XpiUri::Full(id1.into(), expr.into(), id2.into())
            }
        }
    }
}
