use crate::{Attrs, Doc, Expr, FnArguments, Identifier, Lit, NumBound, Span, TryEvaluateInto, Ty};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use util::color;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XpiDef {
    pub doc: Doc,
    pub attrs: Attrs,
    pub uri_segment: UriSegmentSeed,
    pub serial: Option<u32>,
    pub kind: XpiKind,
    pub kv: HashMap<Identifier, TryEvaluateInto<Expr, Lit>>,
    pub implements: Vec<Expr>,
    // TODO: change to ?
    pub children: Vec<XpiDef>,
    pub span: Span,
}

impl XpiDef {
    /// Returns true if self is a method or at least one child no matter how deep in the hierarchy is a method
    pub fn contains_methods(&self) -> bool {
        if let XpiKind::Method { .. } = self.kind {
            return true;
        }
        for c in &self.children {
            if let XpiKind::Method { .. } = c.kind {
                return true;
            }
            if c.contains_methods() {
                return true;
            }
        }
        false
    }
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
    Array {
        num_bound: NumBound,
        is_celled: bool,
    },

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
    Stream {
        /// Ro is read from node, Wo is write to node, Rw is both
        dir: AccessMode,
        ty: Ty,
    },
    /// `/borrowable_group<Cell<_>> { /child<rw u8> }`
    /// `/borrowable_property<Cell<u8>>` - implicitly rw, otherwise no reason for a Cell
    /// `/write_only_cell<Cell< wo u8> >>`
    /// `/borrowable_stream<Cell< ro+stream u8 >>`
    Cell { inner: Box<XpiKind> },
    /// Callable method. `/method<fn ()>`, `/with_args_and_ret<fn (x: u8) -> u8>`
    Method { args: FnArguments, ret_ty: Ty },
    // /// Not yet known kind (type alias or generic type used), can be Property, Cell or Method
    // Generic {
    //     transform: XpiResourceTransform,
    //     ty: Ty,
    // }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AccessMode {
    ImpliedRo,
    Rw,
    Ro,
    Wo,
    Const,
}

impl XpiDef {
    pub fn expect_method_kind(&self) -> Option<(FnArguments, Ty)> {
        match &self.kind {
            XpiKind::Method { args, ret_ty } => Some((args.clone(), ret_ty.clone())),
            _ => None,
        }
    }

    pub fn format_kind(&self) -> String {
        match self.kind {
            XpiKind::Group => "group",
            XpiKind::Array { .. } => "array",
            XpiKind::Property { .. } => "property",
            XpiKind::Stream { .. } => "stream",
            XpiKind::Cell { .. } => "Cell<_>",
            XpiKind::Method { .. } => "method",
        }
            .to_owned()
    }
}

/// UriSegment that can be interpolated into many segments (over a range or set).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UriSegmentSeed {
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

impl UriSegmentSeed {
    pub fn expect_resolved(&self) -> Option<Identifier> {
        match self {
            UriSegmentSeed::Resolved(id) => Some(id.clone()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XpiResourceTy {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XpiBody {}

impl Display for UriSegmentSeed {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            UriSegmentSeed::Resolved(id) => write!(f, "{:-}", id),
            UriSegmentSeed::ExprOnly(expr) => write!(f, "{}", expr),
            UriSegmentSeed::ExprThenNamedPart(expr, id) => write!(f, "{}{:-}", expr, id),
            UriSegmentSeed::NamedPartThenExpr(id, expr) => write!(f, "{:-}{}", id, expr),
            UriSegmentSeed::Full(expr1, id, expr2) => write!(f, "{}{:-}{}", expr1, id, expr2),
        }
    }
}

impl Display for XpiDef {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}{}{b}{y}XpiDef<{d}{} #{:?} {:?} impl:{:?} kv:{:?} children:[ ",
            self.doc,
            self.attrs,
            self.uri_segment,
            self.serial,
            self.kind,
            self.implements,
            self.kv,
            b = color::BOLD,
            y = color::YELLOW,
            d = color::DEFAULT,
        )?;
        let is_alterante = f.alternate();
        let separator = if is_alterante {
            if !self.children.is_empty() {
                writeln!(f, "")?;
            }
            ",\n"
        } else {
            ", "
        }
            .to_owned();
        itertools::intersperse(
            self.children.iter().map(|child| {
                if is_alterante {
                    format!("{:#}", child)
                } else {
                    format!("{}", child)
                }
            }),
            separator,
        )
            .try_for_each(|s| write!(f, "{}", s))?;
        write!(
            f,
            " ]{b}{y}>{d}",
            b = color::BOLD,
            y = color::YELLOW,
            d = color::DEFAULT,
        )
    }
}
