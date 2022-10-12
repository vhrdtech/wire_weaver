use crate::{Doc, Identifier};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeAliasDef {
    pub doc: Doc,
    // pub attrs: Attrs,
    pub typename: Identifier,
    // pub ty: Ty,
}

// impl<'i> TryFrom<TypeAliasDefParser<'i>> for TypeAliasDef {
//     type Error = Error;
//
//     fn try_from(a: TypeAliasDefParser<'i>) -> Result<Self, Self::Error> {
//         Ok(TypeAliasDef {
//             doc: a.doc.into(),
//             attrs: a.attrs.try_into()?,
//             typename: a.typename.into(),
//             ty: a.r#type.into(),
//         })
//     }
// }