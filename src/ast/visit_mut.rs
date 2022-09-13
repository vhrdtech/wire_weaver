use crate::ast::doc::Doc;
use crate::ast::file::{Definition, File};
use crate::ast::identifier::Identifier;
use crate::ast::struct_def::{StructDef, StructField};
use crate::ast::ty::{DiscreteTy, Ty, TyKind};
use parser::span::Span;

pub trait VisitMut {
    fn visit_file(&mut self, i: &mut File) {
        visit_file(self, i);
    }

    fn visit_definition(&mut self, i: &mut Definition) {
        visit_definition(self, i);
    }

    fn visit_struct(&mut self, i: &mut StructDef) {
        visit_struct(self, i);
    }

    fn visit_doc(&mut self, i: &mut Doc) {
        visit_doc(self, i);
    }

    fn visit_identifier(&mut self, i: &mut Identifier) {
        visit_identifier(self, i);
    }

    fn visit_struct_field(&mut self, i: &mut StructField) {
        visit_struct_field(self, i);
    }

    fn visit_span(&mut self, i: &mut Span) {
        visit_span(self, i);
    }

    fn visit_ty(&mut self, i: &mut Ty) {
        visit_ty(self, i);
    }

    fn visit_bool_ty(&mut self, span: &mut Span) {
        visit_bool_ty(self, span);
    }

    fn visit_discrete_ty(&mut self, discrete: &mut DiscreteTy, span: &mut Span) {
        visit_discrete_ty(self, discrete, span);
    }
}

pub fn visit_file<V>(v: &mut V, node: &mut File)
where
    V: VisitMut + ?Sized,
{
    for def in &mut node.items {
        v.visit_definition(def);
    }
}

pub fn visit_definition<V>(v: &mut V, node: &mut Definition)
where
    V: VisitMut + ?Sized,
{
    match node {
        Definition::Struct(struct_def) => v.visit_struct(struct_def),
        _ => {},
    }
}

pub fn visit_struct<V>(v: &mut V, node: &mut StructDef)
where
    V: VisitMut + ?Sized,
{
    v.visit_doc(&mut node.doc);
    v.visit_identifier(&mut node.typename);
    for field in &mut node.fields {
        v.visit_struct_field(field);
    }
    v.visit_span(&mut node.span);
}

pub fn visit_doc<V>(_v: &mut V, _node: &mut Doc)
where
    V: VisitMut + ?Sized,
{
}

pub fn visit_identifier<V>(v: &mut V, node: &mut Identifier)
where
    V: VisitMut + ?Sized,
{
    v.visit_span(&mut node.span);
}

pub fn visit_struct_field<V>(v: &mut V, node: &mut StructField)
where
    V: VisitMut + ?Sized,
{
    v.visit_doc(&mut node.doc);
    v.visit_identifier(&mut node.name);
    v.visit_ty(&mut node.ty);
    v.visit_span(&mut node.span);
}

pub fn visit_span<V>(_v: &mut V, _node: &mut Span)
where
    V: VisitMut + ?Sized,
{
}

pub fn visit_ty<V>(v: &mut V, node: &mut Ty)
where
    V: VisitMut + ?Sized,
{
    match &mut node.kind {
        TyKind::Unit => todo!(),
        TyKind::Boolean => v.visit_bool_ty(&mut node.span),
        TyKind::Discrete(discrete) => v.visit_discrete_ty(discrete, &mut node.span),
        _ => {}
    }
    v.visit_span(&mut node.span);
}

pub fn visit_bool_ty<V>(_v: &mut V, _span: &Span)
where
    V: VisitMut + ?Sized,
{
}

pub fn visit_discrete_ty<V>(_v: &mut V, _discrete: &DiscreteTy, _span: &Span)
where
    V: VisitMut + ?Sized,
{
}
