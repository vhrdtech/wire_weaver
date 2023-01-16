use crate::attribute::{Attr, AttrKind};
use crate::generics::GenericParam;
use crate::ops::{BinaryOp, UnaryOp};
use crate::struct_def::StructField;
use crate::xpi_def::{UriSegmentSeed, XpiKind};
use crate::*;
use std::ops::Deref;

pub trait Visit {
    fn visit_file(&mut self, i: &File) {
        visit_file(self, i);
    }

    fn visit_definition(&mut self, i: &Definition) {
        visit_definition(self, i);
    }

    fn visit_struct_def(&mut self, i: &StructDef) {
        visit_struct_def(self, i);
    }

    fn visit_enum_def(&mut self, i: &EnumDef) {
        visit_enum_def(self, i);
    }

    fn visit_xpi_def(&mut self, i: &XpiDef) {
        visit_xpi_def(self, i);
    }

    fn visit_xpi_uri_segment(&mut self, i: &UriSegmentSeed) {
        visit_xpi_uri_segment(self, i);
    }

    fn visit_xpi_kind(&mut self, i: &XpiKind) {
        visit_xpi_kind(self, i);
    }

    fn visit_fn_def(&mut self, i: &FnDef) {
        visit_fn_def(self, i);
    }

    fn visit_fn_args(&mut self, i: &FnArguments) {
        visit_fn_args(self, i);
    }

    fn visit_type_alias(&mut self, i: &TypeAliasDef) {
        visit_type_alias(self, i);
    }

    fn visit_doc(&mut self, i: &Doc) {
        visit_doc(self, i);
    }

    fn visit_attrs(&mut self, i: &Attrs) {
        visit_attrs(self, i);
    }

    fn visit_attr(&mut self, i: &Attr) {
        visit_attr(self, i);
    }

    fn visit_generics(&mut self, i: &Generics) {
        visit_generics(self, i);
    }

    fn visit_identifier(&mut self, i: &Identifier) {
        visit_identifier(self, i);
    }

    fn visit_struct_def_field(&mut self, i: &StructField) {
        visit_struct_def_field(self, i);
    }

    fn visit_enum_def_item(&mut self, i: &EnumItem) {
        visit_enum_def_item(self, i);
    }

    fn visit_span(&mut self, i: &Span) {
        visit_span(self, i);
    }

    fn visit_ty(&mut self, i: &Ty) {
        visit_ty(self, i);
    }

    fn visit_expr(&mut self, i: &Expr) {
        visit_expr(self, i);
    }

    fn visit_statement(&mut self, i: &Stmt) {
        visit_statement(self, i);
    }

    fn visit_num_bound(&mut self, i: &NumBound) {
        visit_num_bound(self, i);
    }

    fn visit_path(&mut self, i: &Path) {
        visit_path(self, i);
    }

    fn visit_unary_expr(&mut self, op: &UnaryOp, cons: &Expr) {
        visit_unary_expr(self, op, cons);
    }

    fn visit_binary_expr(&mut self, op: &BinaryOp, cons0: &Expr, cons1: &Expr) {
        visit_binary_expr(self, op, cons0, cons1);
    }

    fn visit_bool_ty(&mut self, span: &Span) {
        visit_bool_ty(self, span);
    }

    fn visit_discrete_ty(&mut self, discrete: &DiscreteTy, span: &Span) {
        visit_discrete_ty(self, discrete, span);
    }

    fn visit_autonum_ty(&mut self, autonum: &AutoNumber) {
        visit_autonum_ty(self, autonum);
    }

    fn visit_lit(&mut self, lit: &Lit) {
        visit_lit(self, lit);
    }
}

pub fn visit_file<V: Visit + ?Sized>(v: &mut V, node: &File) {
    for def in node.defs.values() {
        v.visit_definition(def);
    }
}

pub fn visit_definition<V: Visit + ?Sized>(v: &mut V, node: &Definition) {
    match node {
        Definition::TypeAlias(type_alias_def) => v.visit_type_alias(type_alias_def),
        Definition::Enum(enum_def) => v.visit_enum_def(enum_def),
        Definition::Struct(struct_def) => v.visit_struct_def(struct_def),
        Definition::Function(fn_def) => v.visit_fn_def(fn_def),
        Definition::Xpi(xpi_def) => v.visit_xpi_def(xpi_def),
    }
}

pub fn visit_struct_def<V: Visit + ?Sized>(v: &mut V, node: &StructDef) {
    v.visit_doc(&node.doc);
    v.visit_attrs(&node.attrs);
    v.visit_identifier(&node.typename);
    for field in &node.fields {
        v.visit_struct_def_field(field);
    }
    v.visit_span(&node.span);
}

pub fn visit_enum_def<V: Visit + ?Sized>(v: &mut V, node: &EnumDef) {
    v.visit_doc(&node.doc);
    v.visit_attrs(&node.attrs);
    v.visit_identifier(&node.typename);
    for item in &node.items {
        v.visit_enum_def_item(item);
    }
    v.visit_span(&node.span);
}

pub fn visit_xpi_def<V: Visit + ?Sized>(v: &mut V, node: &XpiDef) {
    v.visit_doc(&node.doc);
    v.visit_attrs(&node.attrs);
    v.visit_xpi_uri_segment(&node.uri_segment);
    v.visit_xpi_kind(&node.kind);
    // for (id, try_eval) in &node.kv {
    //     v.visit_identifier(id);
    // }
    for implement in &node.implements {
        v.visit_expr(implement);
    }
    for child in &node.children {
        v.visit_xpi_def(child);
    }
    v.visit_span(&node.span);
}

pub fn visit_xpi_uri_segment<V: Visit + ?Sized>(v: &mut V, node: &UriSegmentSeed) {
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

pub fn visit_xpi_kind<V: Visit + ?Sized>(v: &mut V, node: &XpiKind) {
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

pub fn visit_fn_def<V: Visit + ?Sized>(v: &mut V, node: &FnDef) {
    v.visit_doc(&node.doc);
    v.visit_attrs(&node.attrs);
    v.visit_identifier(&node.name);
    if let Some(generics) = &node.generics {
        v.visit_generics(generics);
    }
    v.visit_fn_args(&node.arguments);
    if let Some(ty) = &node.ret_ty {
        v.visit_ty(ty);
    }
    for stmt in &node.statements {
        v.visit_statement(stmt);
    }
}

pub fn visit_fn_args<V: Visit + ?Sized>(v: &mut V, node: &FnArguments) {
    for arg in &node.args {
        v.visit_identifier(&arg.name);
        v.visit_ty(&arg.ty);
    }
}

pub fn visit_type_alias<V: Visit + ?Sized>(v: &mut V, node: &TypeAliasDef) {
    v.visit_doc(&node.doc);
    v.visit_attrs(&node.attrs);
    v.visit_identifier(&node.typename);
    v.visit_ty(&node.ty);
}

pub fn visit_doc<V: Visit + ?Sized>(v: &mut V, node: &Doc) {
    for (_, span) in &node.lines {
        v.visit_span(span);
    }
}

pub fn visit_attrs<V: Visit + ?Sized>(v: &mut V, node: &Attrs) {
    for attr in &node.attrs {
        v.visit_attr(attr);
    }
    v.visit_span(&node.span);
}

pub fn visit_attr<V: Visit + ?Sized>(v: &mut V, node: &Attr) {
    match &node.kind {
        AttrKind::Expr(expr) => v.visit_expr(expr),
        AttrKind::TT(_) => {}
    }
    v.visit_path(&node.path);
    v.visit_span(&node.span);
}

pub fn visit_generics<V: Visit + ?Sized>(v: &mut V, node: &Generics) {
    for p in &node.params {
        match p {
            GenericParam::Ty(ty) => v.visit_ty(ty),
            GenericParam::Expr(expr) => v.visit_expr(expr),
        }
    }
}

pub fn visit_identifier<V: Visit + ?Sized>(v: &mut V, node: &Identifier) {
    v.visit_span(&node.span);
}

pub fn visit_struct_def_field<V: Visit + ?Sized>(v: &mut V, node: &StructField) {
    v.visit_doc(&node.doc);
    v.visit_attrs(&node.attrs);
    v.visit_identifier(&node.name);
    v.visit_ty(&node.ty);
    v.visit_span(&node.span);
}

pub fn visit_enum_def_item<V: Visit + ?Sized>(v: &mut V, node: &EnumItem) {
    v.visit_doc(&node.doc);
    v.visit_attrs(&node.attrs);
    v.visit_identifier(&node.name);
    match &node.kind {
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

pub fn visit_span<V: Visit + ?Sized>(_v: &mut V, _node: &Span) {}

pub fn visit_ty<V: Visit + ?Sized>(v: &mut V, node: &Ty) {
    match &node.kind {
        TyKind::Unit => {}
        TyKind::Boolean => v.visit_bool_ty(&node.span),
        TyKind::Discrete(discrete) => v.visit_discrete_ty(discrete, &node.span),
        TyKind::AutoNumber(autonum) => v.visit_autonum_ty(autonum),
        _ => {}
    }
    v.visit_span(&node.span);
}

pub fn visit_expr<V: Visit + ?Sized>(v: &mut V, node: &Expr) {
    match node {
        Expr::Call { method, args } => {
            v.visit_path(method);
            for expr in &args.0 {
                v.visit_expr(expr);
            }
        }
        Expr::Index { object, by } => {
            v.visit_path(object);
            for expr in &by.0 {
                v.visit_expr(expr);
            }
        }
        Expr::Lit(lit) => v.visit_lit(lit),
        Expr::Tuple(tuple) => {
            for expr in &tuple.0 {
                v.visit_expr(expr);
            }
        }
        Expr::Ty(ty) => v.visit_ty(ty),
        Expr::Ref(path) => v.visit_path(path),
        Expr::ConsU(op, cons) => v.visit_unary_expr(op, cons),
        Expr::ConsB(op, cons) => {
            let (cons0, cons1) = cons.deref();
            v.visit_binary_expr(op, cons0, cons1);
        }
    }
}

pub fn visit_unary_expr<V: Visit + ?Sized>(v: &mut V, _op: &UnaryOp, cons: &Expr) {
    v.visit_expr(cons);
}

pub fn visit_binary_expr<V: Visit + ?Sized>(v: &mut V, _op: &BinaryOp, cons0: &Expr, cons1: &Expr) {
    v.visit_expr(cons0);
    v.visit_expr(cons1);
}

pub fn visit_statement<V: Visit + ?Sized>(v: &mut V, node: &Stmt) {
    match node {
        Stmt::Let(let_stmt) => {
            v.visit_identifier(&let_stmt.ident);
            if let Some(ty) = &let_stmt.type_ascription {
                v.visit_ty(ty);
            }
            v.visit_expr(&let_stmt.expr);
        }
        Stmt::Expr(expr, _) => v.visit_expr(expr),
        Stmt::Def(def) => v.visit_definition(def),
    }
}

pub fn visit_num_bound<V: Visit + ?Sized>(_v: &mut V, node: &NumBound) {
    match node {
        NumBound::Unbound => {}
        NumBound::MinBound(_num) => {}
        NumBound::MaxBound(_num) => {}
        NumBound::Set(_try_eval_into_set) => {}
    }
}

pub fn visit_path<V: Visit + ?Sized>(v: &mut V, node: &Path) {
    for segment in &node.segments {
        v.visit_identifier(&segment.ident);
    }
}

pub fn visit_bool_ty<V: Visit + ?Sized>(_v: &mut V, _span: &Span) {}

pub fn visit_discrete_ty<V: Visit + ?Sized>(v: &mut V, node: &DiscreteTy, _span: &Span) {
    v.visit_num_bound(&node.num_bound);
}

pub fn visit_autonum_ty<V: Visit + ?Sized>(v: &mut V, autonum: &AutoNumber) {
    v.visit_lit(&autonum.start);
    v.visit_lit(&autonum.step);
    v.visit_lit(&autonum.end);
}

pub fn visit_lit<V: Visit + ?Sized>(v: &mut V, node: &Lit) {
    v.visit_span(&node.span);
}
