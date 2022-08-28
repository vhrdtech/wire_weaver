use std::rc::Rc;
use crate::span::Span;

#[derive(Clone, Debug, Eq, PartialEq)]
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
    XpiUriSegment,

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
}

macro_rules! impl_from_parser_struct {
    ($from: ident, $context: ident) => {
        impl<'i> From<parser::ast::naming::$from<'i>> for Identifier {
            fn from(t: parser::ast::naming::$from<'i>) -> Self {
                Identifier {
                    symbols: Rc::new(t.name.to_string()),
                    context: IdentifierContext::$context,
                    span: t.span.into()
                }
            }
        }
    }
}

impl_from_parser_struct!(Typename, UserTyName);
impl_from_parser_struct!(BuiltinTypename, BuiltinTyName);
impl_from_parser_struct!(PathSegment, PathSegment);
impl_from_parser_struct!(EnumEntryName, EnumFieldName);
impl_from_parser_struct!(XpiUriNamedPart, XpiUriSegment);
impl_from_parser_struct!(XpiKeyName, XpiKeyName);
impl_from_parser_struct!(FnName, FnName);
impl_from_parser_struct!(FnArgName, FnArgName);
impl_from_parser_struct!(LetStmtName, VariableDefName);
impl_from_parser_struct!(Identifier, VariableRefName);
impl_from_parser_struct!(StructFieldName, StructFieldName);

