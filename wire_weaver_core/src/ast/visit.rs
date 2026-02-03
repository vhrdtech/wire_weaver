use crate::ast::api::{
    ApiItem, ApiItemKind, ApiLevel, ApiLevelSourceLocation, Argument, PropertyAccess,
};
use crate::ast::trait_macro_args::ImplTraitMacroArgs;
use proc_macro2::Ident;
use shrink_wrap_core::ast::{Docs, Type};

pub trait Visit {
    fn visit_level(&mut self, level: &ApiLevel) {
        let _ = level;
    }

    fn finish_level(&mut self, level: &ApiLevel) {
        let _ = level;
    }

    fn visit_docs(&mut self, docs: &Docs) {
        let _ = docs;
    }

    fn visit_ident(&mut self, ident: &Ident) {
        let _ = ident;
    }

    fn visit_source_location(&mut self, location: &ApiLevelSourceLocation) {
        let _ = location;
    }

    fn visit_api_item(&mut self, item: &ApiItem) {
        let _ = item;
    }

    fn finish_api_item(&mut self, item: &ApiItem) {
        let _ = item;
    }

    fn visit_method(&mut self, ident: &Ident, args: &[Argument], return_type: &Option<Type>) {
        let _ = ident;
        let _ = args;
        let _ = return_type;
    }

    fn visit_property(
        &mut self,
        ident: &Ident,
        ty: &Type,
        acess: PropertyAccess,
        user_result_ty: &Option<Type>,
    ) {
        let _ = ident;
        let _ = ty;
        let _ = acess;
        let _ = user_result_ty;
    }

    fn visit_stream(&mut self, ident: &Ident, ty: &Type, is_up: bool) {
        let _ = ident;
        let _ = ty;
        let _ = is_up;
    }

    fn visit_impl_trait(&mut self, args: &ImplTraitMacroArgs, level: &ApiLevel) {
        let _ = args;
        let _ = level;
    }

    fn visit_reserved(&mut self) {}

    fn visit_argument(&mut self, arg: &Argument) {
        let _ = arg;
    }

    fn visit_type(&mut self, ty: &Type) {
        let _ = ty;
    }
}

pub fn visit_api_level(level: &ApiLevel, v: &mut impl Visit) {
    v.visit_level(level);
    v.visit_ident(&level.name);
    v.visit_docs(&level.docs);
    v.visit_source_location(&level.source_location);
    for item in &level.items {
        v.visit_api_item(item);
        v.visit_docs(&item.docs);
        match &item.kind {
            ApiItemKind::Method {
                ident,
                args,
                return_type,
            } => {
                v.visit_method(ident, args, return_type);
                v.visit_ident(ident);
                for arg in args {
                    v.visit_argument(arg);
                    v.visit_ident(&arg.ident);
                    v.visit_type(&arg.ty);
                }
                if let Some(ty) = return_type {
                    v.visit_type(ty);
                }
            }
            ApiItemKind::Property {
                ident,
                ty,
                access,
                user_result_ty,
            } => {
                v.visit_property(ident, ty, *access, user_result_ty);
                v.visit_ident(ident);
                v.visit_type(ty);
                if let Some(ty) = user_result_ty {
                    v.visit_type(ty);
                }
            }
            ApiItemKind::Stream { ident, ty, is_up } => {
                v.visit_stream(ident, ty, *is_up);
                v.visit_ident(ident);
                v.visit_type(ty);
            }
            ApiItemKind::ImplTrait { args, level } => {
                let level = level.as_ref().expect("");
                v.visit_impl_trait(args, level);
                visit_api_level(level, v);
            }
            ApiItemKind::Reserved => {
                v.visit_reserved();
            }
        }
        v.finish_api_item(item);
    }
    v.finish_level(level);
}
