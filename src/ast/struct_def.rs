use std::fmt::{Display, Formatter};
use termion::{color, style};
use crate::ast::doc::Doc;
use crate::ast::identifier::Identifier;
use crate::ast::ty::Ty;
use parser::ast::def_struct::{
    DefStruct as StructDefParser, StructField as StructFieldParser,
};
use parser::span::Span;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructDef {
    pub doc: Doc,
    pub typename: Identifier,
    pub fields: Vec<StructField>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructField {
    pub doc: Doc,
    pub name: Identifier,
    pub ty: Ty,
    pub span: Span,
}

impl<'i> From<StructDefParser<'i>> for StructDef {
    fn from(sd: StructDefParser<'i>) -> Self {
        StructDef {
            doc: sd.doc.into(),
            typename: sd.typename.into(),
            fields: sd.fields.fields.iter().map(|sf| sf.clone().into()).collect(),
            span: sd.span.into(),
        }
    }
}

impl<'i> From<StructFieldParser<'i>> for StructField {
    fn from(sf: StructFieldParser<'i>) -> Self {
        StructField {
            doc: sf.doc.into(),
            name: sf.name.into(),
            ty: sf.ty.into(),
            span: sf.span.into(),
        }
    }
}

impl StructDef {
    pub fn is_sized(&self) -> bool {
        for f in &self.fields {
            if !f.ty.is_sized() {
                return false;
            }
        }
        true
    }
}

impl Display for StructDef {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "{}{}{}struct{} {:-} {}-->{} {:#}",
            self.doc,
            style::Bold,
            color::Fg(color::Rgb(203, 120, 50)),
            style::Reset,
            self.typename,
            color::Fg(color::Blue),
            style::Reset,
            self.span
        )?;
        self.fields.iter().try_for_each(|sf| write!(f, "{}", sf))?;
        Ok(())
    }
}

impl Display for StructField {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "  {}pub {}{:-}{}: {}",
            // self.doc,
            color::Fg(color::Rgb(203, 120, 50)),
            color::Fg(color::Magenta),
            self.name,
            style::Reset,
            self.ty
        )
    }
}