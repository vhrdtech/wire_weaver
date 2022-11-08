use crate::attribute::{Attr, AttrKind};
use crate::generics::GenericParam;
use crate::ops::{BinaryOp, UnaryOp};
use crate::struct_def::StructField;
use crate::xpi_def::{UriSegmentSeed, XpiKind};
use crate::*;
use std::ops::DerefMut;

pub trait VisitMut {
    fn visit_file(&mut self, i: &mut File) {
        visit_file(self, i);
    }

    fn visit_definition(&mut self, i: &mut Definition) {
        visit_definition(self, i);
    }

    fn visit_struct_def(&mut self, i: &mut StructDef) {
        visit_struct_def(self, i);
    }

    fn visit_enum_def(&mut self, i: &mut EnumDef) {
        visit_enum_def(self, i);
    }

    fn visit_xpi_def(&mut self, i: &mut XpiDef) {
        visit_xpi_def(self, i);
    }

    fn visit_xpi_uri_segment(&mut self, i: &mut UriSegmentSeed) {
        visit_xpi_uri_segment(self, i);
    }

    fn visit_xpi_kind(&mut self, i: &mut XpiKind) {
        visit_xpi_kind(self, i);
    }

    fn visit_fn_def(&mut self, i: &mut FnDef) {
        visit_fn_def(self, i);
    }

    fn visit_fn_args(&mut self, i: &mut FnArguments) {
        visit_fn_args(self, i);
    }

    fn visit_type_alias(&mut self, i: &mut TypeAliasDef) {
        visit_type_alias(self, i);
    }

    fn visit_doc(&mut self, i: &mut Doc) {
        visit_doc(self, i);
    }

    fn visit_attrs(&mut self, i: &mut Attrs) {
        visit_attrs(self, i);
    }

    fn visit_attr(&mut self, i: &mut Attr) {
        visit_attr(self, i);
    }

    fn visit_generics(&mut self, i: &mut Generics) {
        visit_generics(self, i);
    }

    fn visit_identifier(&mut self, i: &mut Identifier) {
        visit_identifier(self, i);
    }

    fn visit_struct_def_field(&mut self, i: &mut StructField) {
        visit_struct_def_field(self, i);
    }

    fn visit_enum_def_item(&mut self, i: &mut EnumItem) {
        visit_enum_def_item(self, i);
    }

    fn visit_span(&mut self, i: &mut Span) {
        visit_span(self, i);
    }

    fn visit_ty(&mut self, i: &mut Ty) {
        visit_ty(self, i);
    }

    fn visit_expr(&mut self, i: &mut Expr) {
        visit_expr(self, i);
    }

    fn visit_statement(&mut self, i: &mut Stmt) {
        visit_statement(self, i);
    }

    fn visit_num_bound(&mut self, i: &mut NumBound) {
        visit_num_bound(self, i);
    }

    fn visit_path(&mut self, i: &mut Path) {
        visit_path(self, i);
    }

    fn visit_unary_expr(&mut self, op: &mut UnaryOp, cons: &mut Expr) {
        visit_unary_expr(self, op, cons);
    }

    fn visit_binary_expr(&mut self, op: &mut BinaryOp, cons0: &mut Expr, cons1: &mut Expr) {
        visit_binary_expr(self, op, cons0, cons1);
    }

    fn visit_bool_ty(&mut self, span: &mut Span) {
        visit_bool_ty(self, span);
    }

    fn visit_discrete_ty(&mut self, discrete: &mut DiscreteTy, span: &mut Span) {
        visit_discrete_ty(self, discrete, span);
    }

    fn visit_autonum_ty(&mut self, autonum: &mut AutoNumber) {
        visit_autonum_ty(self, autonum);
    }

    fn visit_lit(&mut self, lit: &mut Lit) {
        visit_lit(self, lit);
    }
}

pub fn visit_file<V: VisitMut + ?Sized>(v: &mut V, node: &mut File) {
    for (_, def) in &mut node.defs {
        v.visit_definition(def);
    }
}

pub fn visit_definition<V: VisitMut + ?Sized>(v: &mut V, node: &mut Definition) {
    match node {
        Definition::TypeAlias(type_alias_def) => v.visit_type_alias(type_alias_def),
        Definition::Enum(enum_def) => v.visit_enum_def(enum_def),
        Definition::Struct(struct_def) => v.visit_struct_def(struct_def),
        Definition::Function(fn_def) => v.visit_fn_def(fn_def),
        Definition::Xpi(xpi_def) => v.visit_xpi_def(xpi_def),
    }
}

pub fn visit_struct_def<V: VisitMut + ?Sized>(v: &mut V, node: &mut StructDef) {
    v.visit_doc(&mut node.doc);
    v.visit_attrs(&mut node.attrs);
    v.visit_identifier(&mut node.typename);
    for field in &mut node.fields {
        v.visit_struct_def_field(field);
    }
    v.visit_span(&mut node.span);
}

pub fn visit_enum_def<V: VisitMut + ?Sized>(v: &mut V, node: &mut EnumDef) {
    v.visit_doc(&mut node.doc);
    v.visit_attrs(&mut node.attrs);
    v.visit_identifier(&mut node.typename);
    for item in &mut node.items {
        v.visit_enum_def_item(item);
    }
    v.visit_span(&mut node.span);
}

pub fn visit_xpi_def<V: VisitMut + ?Sized>(v: &mut V, node: &mut XpiDef) {
    v.visit_doc(&mut node.doc);
    v.visit_attrs(&mut node.attrs);
    v.visit_xpi_uri_segment(&mut node.uri_segment);
    v.visit_xpi_kind(&mut node.kind);
    // for (id, try_eval) in &mut node.kv {
    //     v.visit_identifier(id);
    // }
    for implement in &mut node.implements {
        v.visit_expr(implement);
    }
    for child in &mut node.children {
        v.visit_xpi_def(child);
    }
    v.visit_span(&mut node.span);
}

pub fn visit_xpi_uri_segment<V: VisitMut + ?Sized>(v: &mut V, node: &mut UriSegmentSeed) {
    match node {
        UriSegmentSeed::Resolved(id) => v.visit_identifier(id),
        UriSegmentSeed::ExprOnly(expr) => v.visit_expr(expr),
        UriSegmentSeed::ExprThenNamedPart(expr, id) => {
            v.visit_expr(expr);
            v.visit_identifier(id);
        }
        UriSegmentSeed::NamedPartThenExpr(id, expr) => {
            v.visit_identifier(id);
            v.visit_expr(expr);
        }
        UriSegmentSeed::Full(id0, expr, id1) => {
            v.visit_identifier(id0);
            v.visit_expr(expr);
            v.visit_identifier(id1);
        }
    }
}

pub fn visit_xpi_kind<V: VisitMut + ?Sized>(v: &mut V, node: &mut XpiKind) {
    match node {
        XpiKind::Group => {}
        XpiKind::Array { num_bound, .. } => {
            v.visit_num_bound(num_bound);
        }
        XpiKind::Property { .. } => {}
        XpiKind::Stream { .. } => {}
        XpiKind::Cell { .. } => {}
        XpiKind::Method { .. } => {}
    }
}

pub fn visit_fn_def<V: VisitMut + ?Sized>(v: &mut V, node: &mut FnDef) {
    v.visit_doc(&mut node.doc);
    v.visit_attrs(&mut node.attrs);
    v.visit_identifier(&mut node.name);
    if let Some(generics) = &mut node.generics {
        v.visit_generics(generics);
    }
    v.visit_fn_args(&mut node.arguments);
    if let Some(ty) = &mut node.ret_ty {
        v.visit_ty(ty);
    }
    for stmt in &mut node.statements {
        v.visit_statement(stmt);
    }
}

pub fn visit_fn_args<V: VisitMut + ?Sized>(v: &mut V, node: &mut FnArguments) {
    for arg in &mut node.args {
        v.visit_identifier(&mut arg.name);
        v.visit_ty(&mut arg.ty);
    }
}

pub fn visit_type_alias<V: VisitMut + ?Sized>(v: &mut V, node: &mut TypeAliasDef) {
    v.visit_doc(&mut node.doc);
    v.visit_attrs(&mut node.attrs);
    v.visit_identifier(&mut node.typename);
    v.visit_ty(&mut node.ty);
}

pub fn visit_doc<V: VisitMut + ?Sized>(v: &mut V, node: &mut Doc) {
    for (_, span) in &mut node.lines {
        v.visit_span(span);
    }
}

pub fn visit_attrs<V: VisitMut + ?Sized>(v: &mut V, node: &mut Attrs) {
    for attr in &mut node.attrs {
        v.visit_attr(attr);
    }
    v.visit_span(&mut node.span);
}

pub fn visit_attr<V: VisitMut + ?Sized>(v: &mut V, node: &mut Attr) {
    match &mut node.kind {
        AttrKind::Expr(expr) => v.visit_expr(expr),
        AttrKind::TT(_) => {}
    }
    v.visit_path(&mut node.path);
    v.visit_span(&mut node.span);
}

pub fn visit_generics<V: VisitMut + ?Sized>(v: &mut V, node: &mut Generics) {
    for p in &mut node.params {
        match p {
            GenericParam::Ty(ty) => v.visit_ty(ty),
            GenericParam::Expr(expr) => v.visit_expr(expr),
        }
    }
}

pub fn visit_identifier<V: VisitMut + ?Sized>(v: &mut V, node: &mut Identifier) {
    v.visit_span(&mut node.span);
}

pub fn visit_struct_def_field<V: VisitMut + ?Sized>(v: &mut V, node: &mut StructField) {
    v.visit_doc(&mut node.doc);
    v.visit_attrs(&mut node.attrs);
    v.visit_identifier(&mut node.name);
    v.visit_ty(&mut node.ty);
    v.visit_span(&mut node.span);
}

pub fn visit_enum_def_item<V: VisitMut + ?Sized>(v: &mut V, node: &mut EnumItem) {
    v.visit_doc(&mut node.doc);
    v.visit_attrs(&mut node.attrs);
    v.visit_identifier(&mut node.name);
    match &mut node.kind {
        None => {}
        Some(kind) => match kind {
            EnumItemKind::Tuple(tuple) => {
                for ty in tuple {
                    v.visit_ty(ty);
                }
            }
            EnumItemKind::Struct(struct_fields) => {
                for field in struct_fields {
                    v.visit_struct_def_field(field)
                }
            }
            EnumItemKind::Discriminant(discriminant) => v.visit_lit(discriminant),
        },
    }
}

pub fn visit_span<V: VisitMut + ?Sized>(_v: &mut V, _node: &mut Span) {}

pub fn visit_ty<V: VisitMut + ?Sized>(v: &mut V, node: &mut Ty) {
    match &mut node.kind {
        TyKind::Unit => {}
        TyKind::Boolean => v.visit_bool_ty(&mut node.span),
        TyKind::Discrete(discrete) => v.visit_discrete_ty(discrete, &mut node.span),
        TyKind::AutoNumber(autonum) => v.visit_autonum_ty(autonum),
        _ => {}
    }
    v.visit_span(&mut node.span);
}

pub fn visit_expr<V: VisitMut + ?Sized>(v: &mut V, node: &mut Expr) {
    match node {
        Expr::Call { method, args } => {
            v.visit_path(method);
            for expr in &mut args.0 {
                v.visit_expr(expr);
            }
        }
        Expr::Index { object, by } => {
            v.visit_path(object);
            for expr in &mut by.0 {
                v.visit_expr(expr);
            }
        }
        Expr::Lit(lit) => v.visit_lit(lit),
        Expr::Tuple(tuple) => {
            for expr in &mut tuple.0 {
                v.visit_expr(expr);
            }
        }
        Expr::Ty(ty) => v.visit_ty(ty),
        Expr::Ref(path) => v.visit_path(path),
        Expr::ConsU(op, cons) => v.visit_unary_expr(op, cons),
        Expr::ConsB(op, cons) => {
            let (cons0, cons1) = cons.deref_mut();
            v.visit_binary_expr(op, cons0, cons1);
        }
    }
}

pub fn visit_unary_expr<V: VisitMut + ?Sized>(v: &mut V, _op: &mut UnaryOp, cons: &mut Expr) {
    v.visit_expr(cons);
}

pub fn visit_binary_expr<V: VisitMut + ?Sized>(
    v: &mut V,
    _op: &mut BinaryOp,
    cons0: &mut Expr,
    cons1: &mut Expr,
) {
    v.visit_expr(cons0);
    v.visit_expr(cons1);
}

pub fn visit_statement<V: VisitMut + ?Sized>(v: &mut V, node: &mut Stmt) {
    match node {
        Stmt::Let(let_stmt) => {
            v.visit_identifier(&mut let_stmt.ident);
            if let Some(ty) = &mut let_stmt.type_ascription {
                v.visit_ty(ty);
            }
            v.visit_expr(&mut let_stmt.expr);
        }
        Stmt::Expr(expr, _) => v.visit_expr(expr),
        Stmt::Def(def) => v.visit_definition(def),
    }
}

pub fn visit_num_bound<V: VisitMut + ?Sized>(_v: &mut V, node: &mut NumBound) {
    match node {
        NumBound::Unbound => {}
        NumBound::MinBound(_num) => {}
        NumBound::MaxBound(_num) => {}
        NumBound::Set(_try_eval_into_set) => {}
    }
}

pub fn visit_path<V: VisitMut + ?Sized>(v: &mut V, node: &mut Path) {
    for segment in &mut node.segments {
        v.visit_identifier(segment);
    }
}

pub fn visit_bool_ty<V: VisitMut + ?Sized>(_v: &mut V, _span: &Span) {}

pub fn visit_discrete_ty<V: VisitMut + ?Sized>(v: &mut V, node: &mut DiscreteTy, _span: &Span) {
    v.visit_num_bound(&mut node.num_bound);
}

pub fn visit_autonum_ty<V: VisitMut + ?Sized>(v: &mut V, autonum: &mut AutoNumber) {
    v.visit_lit(&mut autonum.start);
    v.visit_lit(&mut autonum.step);
    v.visit_lit(&mut autonum.end);
}

pub fn visit_lit<V: VisitMut + ?Sized>(v: &mut V, node: &mut Lit) {
    v.visit_span(&mut node.span);
}
