use crate::{Attrs, Doc, Identifier, Ty};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeAliasDef {
    pub doc: Doc,
    pub attrs: Attrs,
    pub typename: Identifier,
    pub ty: Ty,
}
