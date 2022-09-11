use parser::span::Span;
use std::fmt::{Debug, Display, Formatter};
use std::rc::Rc;

#[derive(Clone, Eq, PartialEq)]
pub struct Identifier {
    pub symbols: Rc<String>,
    pub context: IdentifierContext,
    pub span: Span,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum IdentifierContext {
    /// type **MyType** = u8;
    UserTyName,

    /// **autonum**, **indexof**
    BuiltinTyName,

    /// use **abc** :: **def**;
    PathSegment,

    /// /**resource**, /**ch**`1..3`
    XpiUriSegmentName,

    /// /x { **key_name**: value; }
    XpiKeyName,

    /// fn **fun**() {}
    FnName,

    /// fn fun(**arg_name**: u8) {}
    FnArgName,

    /// let **val** = 1;
    VariableDefName,

    /// **force**.**filtered** + 5
    VariableRefName,

    /// struct **MyStruct** {}
    StructTyName,

    /// struct MyStruct { **field**: u8 }
    StructFieldName,

    /// enum **MyEnum** {}
    EnumTyName,

    /// enum MyEnum { **Field1**, **Field2** }
    EnumFieldName,

    /// fn fun<**GN**>() {}
    GenericName,
}

macro_rules! impl_from_parser_struct {
    ($kind: ident) => {
        impl<'i> From<parser::ast::naming::Identifier<'i, $kind>> for Identifier {
            fn from(t: parser::ast::naming::Identifier<'i, $kind>) -> Self {
                Identifier {
                    symbols: Rc::new(t.name.to_string()),
                    context: IdentifierContext::$kind,
                    span: t.span.into(),
                }
            }
        }
    };
}
use parser::ast::naming::*;
impl_from_parser_struct!(UserTyName);
impl_from_parser_struct!(BuiltinTyName);
impl_from_parser_struct!(PathSegment);
impl_from_parser_struct!(XpiUriSegmentName);
impl_from_parser_struct!(XpiKeyName);
impl_from_parser_struct!(FnName);
impl_from_parser_struct!(FnArgName);
impl_from_parser_struct!(VariableDefName);
impl_from_parser_struct!(VariableRefName);
impl_from_parser_struct!(StructTyName);
impl_from_parser_struct!(StructFieldName);
impl_from_parser_struct!(EnumTyName);
impl_from_parser_struct!(EnumFieldName);
impl_from_parser_struct!(GenericName);

impl Display for Identifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            write!(f, "Id(\x1b[35m{}\x1b[0m @{:#})", self.symbols, self.span)
        } else {
            write!(
                f,
                "Id<{:?}>(\x1b[35m{}\x1b[0m @{})",
                self.context, self.symbols, self.span
            )
        }
    }
}

impl Debug for Identifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            write!(f, "{:#}", self)
        } else {
            write!(f, "{}", self)
        }
    }
}
