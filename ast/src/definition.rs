use std::fmt::{Display, Formatter};
use std::rc::Rc;
use crate::{EnumDef, FnDef, Identifier, IdentifierContext, Span, StructDef, TypeAliasDef, XpiDef};
use crate::xpi_def::UriSegmentSeed;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Definition {
    //Const(ConstDef),
    Enum(EnumDef),
    Struct(StructDef),
    Function(FnDef),
    TypeAlias(TypeAliasDef),
    Xpi(XpiDef),
}

impl Definition {
    pub fn name(&self) -> Identifier {
        match self {
            Definition::Enum(d) => d.typename.clone(),
            Definition::Struct(d) => d.typename.clone(),
            Definition::Function(d) => d.name.clone(),
            Definition::TypeAlias(d) => d.typename.clone(),
            Definition::Xpi(d) => {
                match &d.uri_segment {
                    UriSegmentSeed::Resolved(id) => id.clone(),
                    _ => Identifier {
                        symbols: Rc::new("_not_resolved_xpi_name".to_string()),
                        context: IdentifierContext::XpiUriSegmentName,
                        span: Span::call_site(),
                    }
                }
            }
        }
    }
}

impl Display for Definition {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            match self {
                Definition::Enum(ed) => write!(f, "{:#?}", ed),
                Definition::Struct(s) => write!(f, "{:#}", s),
                Definition::Xpi(x) => write!(f, "{:#}", x),
                Definition::Function(fd) => write!(f, "{:#?}", fd),
                Definition::TypeAlias(a) => write!(f, "{:#?}", a),
            }
        } else {
            match self {
                Definition::Enum(ed) => write!(f, "{:?}", ed),
                Definition::Struct(s) => write!(f, "{}", s),
                Definition::Xpi(x) => write!(f, "{}", x),
                Definition::Function(fd) => write!(f, "{:?}", fd),
                Definition::TypeAlias(a) => write!(f, "{:?}", a),
            }
        }
    }
}
