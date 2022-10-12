use crate::TypeAliasDef;

#[derive(Clone, Debug, PartialEq)]
pub enum Definition {
    //Const(ConstDef),
    // Enum(EnumDef),
    // Struct(StructDef),
    //Function(FunctionDef),
    TypeAlias(TypeAliasDef),
    // Xpi(XpiDef),
}

// impl Display for Definition {
//     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
//         match self {
//             Definition::Struct(s) => writeln!(f, "{}", s),
//             Definition::Enum(ed) => writeln!(f, "{:?}", ed),
//             Definition::Xpi(_x) => writeln!(f, "xPI"),
//             Definition::TypeAlias(_a) => writeln!(f, "alias"),
//         }
//     }
// }
