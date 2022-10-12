use crate::ast::doc::Doc;
use crate::ast::file::{Definition, File};
use crate::ast::identifier::Identifier;
use crate::ast::struct_def::{StructDef, StructField};
use crate::ast::ty::{DiscreteTy, Ty, TyKind};
use parser::span::Span;

pub trait Visit {
    fn visit_file(&mut self, i: &File) {
        visit_file(self, i);
    }

    fn visit_definition(&mut self, i: &Definition) {
        visit_definition(self, i);
    }

    fn visit_struct(&mut self, i: &StructDef) {
        visit_struct(self, i);
    }

    fn visit_doc(&mut self, i: &Doc) {
        visit_doc(self, i);
    }

    fn visit_identifier(&mut self, i: &Identifier) {
        visit_identifier(self, i);
    }

    fn visit_struct_field(&mut self, i: &StructField) {
        visit_struct_field(self, i);
    }

    fn visit_span(&mut self, i: &Span) {
        visit_span(self, i);
    }

    fn visit_ty(&mut self, i: &Ty) {
        visit_ty(self, i);
    }

    fn visit_bool_ty(&mut self, span: &Span) {
        visit_bool_ty(self, span);
    }

    fn visit_discrete_ty(&mut self, discrete: &DiscreteTy, span: &Span) {
        visit_discrete_ty(self, discrete, span);
    }
}

pub fn visit_file<V>(v: &mut V, node: &File)
where
    V: Visit + ?Sized,
{
    for def in &node.items {
        v.visit_definition(def);
    }
}

pub fn visit_definition<V>(v: &mut V, node: &Definition)
where
    V: Visit + ?Sized,
{
    match node {
        Definition::Struct(struct_def) => v.visit_struct(struct_def),
        _ => todo!(),
    }
}

pub fn visit_struct<V>(v: &mut V, node: &StructDef)
where
    V: Visit + ?Sized,
{
    v.visit_doc(&node.doc);
    v.visit_identifier(&node.typename);
    for field in &node.fields {
        v.visit_struct_field(field);
    }
    v.visit_span(&node.span);
}

pub fn visit_doc<V>(_v: &mut V, _node: &Doc)
where
    V: Visit + ?Sized,
{
}

pub fn visit_identifier<V>(v: &mut V, node: &Identifier)
where
    V: Visit + ?Sized,
{
    v.visit_span(&node.span);
}

pub fn visit_struct_field<V>(v: &mut V, node: &StructField)
where
    V: Visit + ?Sized,
{
    v.visit_doc(&node.doc);
    v.visit_identifier(&node.name);
    v.visit_ty(&node.ty);
    v.visit_span(&node.span);
}

pub fn visit_span<V>(_v: &mut V, _node: &Span)
where
    V: Visit + ?Sized,
{
}

pub fn visit_ty<V>(v: &mut V, node: &Ty)
where
    V: Visit + ?Sized,
{
    match &node.kind {
        TyKind::Boolean => v.visit_bool_ty(&node.span),
        TyKind::Discrete(discrete) => v.visit_discrete_ty(discrete, &node.span),
        _ => {}
    }
    v.visit_span(&node.span);
}

pub fn visit_bool_ty<V>(_v: &mut V, _span: &Span)
where
    V: Visit + ?Sized,
{
}

pub fn visit_discrete_ty<V>(_v: &mut V, _discrete: &DiscreteTy, _span: &Span)
where
    V: Visit + ?Sized,
{
}
