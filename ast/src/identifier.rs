use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::rc::Rc;
use util::color;
use crate::Span;

#[derive(Clone, Eq, PartialEq)]
pub struct Identifier {
    pub symbols: Rc<String>,
    pub context: IdentifierContext,
    pub span: Span,
}

impl Hash for Identifier {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.symbols.hash(state);
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum IdentifierContext {
    /// type **MyType** = u8;
    TyAlias,

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

impl Display for Identifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if f.sign_plus() {
            write!(f, "Id:{}{}{} @{:#}", color::MAGENTA, self.symbols, color::DEFAULT, self.span)
        } else if f.sign_minus() {
            write!(f, "{}{}{}", color::MAGENTA, self.symbols, color::DEFAULT)
        } else {
            write!(f, "Id:{}{}{}", color::MAGENTA, self.symbols, color::DEFAULT)
        }
    }
}

impl Debug for Identifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if f.sign_plus() {
            write!(f, "{:+}", self)
        } else if f.sign_minus() {
            write!(f, "{:-}", self)
        } else {
            write!(f, "{}", self)
        }
    }
}

impl PartialEq<String> for Identifier {
    fn eq(&self, other: &String) -> bool {
        self.symbols.deref() == other
    }
}

impl PartialEq<&str> for Identifier {
    fn eq(&self, other: &&str) -> bool {
        self.symbols.deref().as_str() == *other
    }
}