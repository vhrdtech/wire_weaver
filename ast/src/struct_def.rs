use crate::{Attrs, Doc, Identifier, Span, Ty};
use std::fmt::{Display, Formatter};
use util::color;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructDef {
    pub doc: Doc,
    pub attrs: Attrs,
    pub typename: Identifier,
    pub fields: Vec<StructField>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructField {
    pub doc: Doc,
    pub attrs: Attrs,
    pub name: Identifier,
    pub ty: Ty,
    pub span: Span,
}

impl StructDef {
    // pub fn is_sized(&self) -> bool {
    //     for f in &self.fields {
    //         if !f.ty.is_sized() {
    //             return false;
    //         }
    //     }
    //     true
    // }
}

impl Display for StructDef {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}{}{}struct{} {}{}",
            self.doc,
            self.attrs,
            color::BOLD,
            color::ORANGE,
            color::DEFAULT,
            self.typename,
        )?;
        itertools::intersperse(
            self.fields.iter().map(|field| format!("{}", field)),
            ", ".to_owned(),
        )
            .try_for_each(|s| write!(f, "{}", s))?;
        Ok(())
    }
}

impl Display for StructField {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}{}{}pub {}{}{}: {}",
            self.doc,
            self.attrs,
            color::ORANGE,
            color::MAGENTA,
            self.name,
            color::DEFAULT,
            self.ty
        )
    }
}
