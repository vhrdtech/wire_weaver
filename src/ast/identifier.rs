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
    VariableName,

    /// struct **MyStruct** {}
    StructTyName,

    /// struct MyStruct { **field**: u8 }
    StructFieldName,

    /// enum **MyEnum** {}
    EnumTyName,

    /// enum MyEnum { **Field1**, **Field2** }
    EnumFieldName,
}