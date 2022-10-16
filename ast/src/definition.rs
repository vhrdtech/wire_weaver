use std::fmt::{Display, Formatter};
use crate::{EnumDef, FnDef, StructDef, TypeAliasDef, XpiDef};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Definition {
    //Const(ConstDef),
    Enum(EnumDef),
    Struct(StructDef),
    Function(FnDef),
    TypeAlias(TypeAliasDef),
    Xpi(XpiDef),
}

impl Display for Definition {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Definition::Enum(ed) => write!(f, "{:?}", ed),
            Definition::Struct(s) => write!(f, "{}", s),
            Definition::Xpi(x) => write!(f, "{}", x),
            Definition::Function(fd) => write!(f, "{:?}", fd),
            Definition::TypeAlias(a) => write!(f, "{:?}", a),
        }
    }
}
