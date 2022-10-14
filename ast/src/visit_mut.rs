use crate::*;
use crate::struct_def::StructField;


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

    fn visit_type_alias(&mut self, i: &mut TypeAliasDef) {
        visit_type_alias(self, i);
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

    fn visit_autonum_ty(&mut self, autonum: &mut AutoNumber, span: &mut Span) {
        visit_autonum_ty(self, autonum, span);
    }

    fn visit_lit(&mut self, lit: &mut Lit, span: &mut Span) {
        visit_lit(self, lit, span);
    }
}

pub fn visit_file<V>(v: &mut V, node: &mut File)
where
    V: VisitMut + ?Sized,
{
    for def in &mut node.defs {
        v.visit_definition(def);
    }
}

pub fn visit_definition<V>(v: &mut V, node: &mut Definition)
where
    V: VisitMut + ?Sized,
{
    match node {
        // Definition::Struct(struct_def) => v.visit_struct(struct_def),
        Definition::TypeAlias(type_alias_def) => v.visit_type_alias(type_alias_def),
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

pub fn visit_type_alias<V>(v: &mut V, node: &mut TypeAliasDef)
    where
        V: VisitMut + ?Sized,
{
    v.visit_doc(&mut node.doc);
    v.visit_identifier(&mut node.typename);
    v.visit_ty(&mut node.ty);
}

pub fn visit_doc<V>(_v: &mut V, _node: &mut Doc)
    where
        V: VisitMut + ?Sized,
{}

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
        TyKind::AutoNumber(autonum) => v.visit_autonum_ty(autonum, &mut node.span),
        _ => {}
    }
    v.visit_span(&mut node.span);
}

pub fn visit_bool_ty<V>(_v: &mut V, _span: &Span)
where
    V: VisitMut + ?Sized,
{}

pub fn visit_discrete_ty<V>(_v: &mut V, _discrete: &DiscreteTy, _span: &Span)
    where
        V: VisitMut + ?Sized,
{}

pub fn visit_autonum_ty<V>(v: &mut V, autonum: &mut AutoNumber, span: &mut Span)
    where
        V: VisitMut + ?Sized,
{
    v.visit_lit(&mut autonum.start, span);
    v.visit_lit(&mut autonum.step, span);
    v.visit_lit(&mut autonum.end, span);
}

pub fn visit_lit<V>(_v: &mut V, _lit: &Lit, _span: &Span)
    where
        V: VisitMut + ?Sized,
{}