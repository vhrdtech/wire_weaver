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

    fn after_visit_level(&mut self, level: &ApiLevel) {
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

    fn after_visit_api_item(&mut self, item: &ApiItem) {
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

    fn after_visit_impl_trait(&mut self, args: &ImplTraitMacroArgs, level: &ApiLevel) {
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

    fn hook(&mut self) -> Option<&mut dyn Visit> {
        None
    }
}

macro_rules! visit {
    ($v:ident.$method:ident($($args:tt)*)) => {
        $v.$method($($args)*);
        if let Some(hook) = $v.hook() {
            hook.$method($($args)*);
        }
    };
}

pub fn visit_api_level(level: &ApiLevel, v: &mut impl Visit) {
    visit!(v.visit_level(level));
    visit!(v.visit_ident(&level.name));
    visit!(v.visit_docs(&level.docs));
    visit!(v.visit_source_location(&level.source_location));
    for item in &level.items {
        visit!(v.visit_api_item(item));
        visit!(v.visit_docs(&item.docs));
        match &item.kind {
            ApiItemKind::Method {
                ident,
                args,
                return_type,
            } => {
                visit!(v.visit_method(ident, args, return_type));
                visit!(v.visit_ident(ident));
                for arg in args {
                    visit!(v.visit_argument(arg));
                    visit!(v.visit_ident(&arg.ident));
                    visit!(v.visit_type(&arg.ty));
                }
                if let Some(ty) = return_type {
                    visit!(v.visit_type(ty));
                }
            }
            ApiItemKind::Property {
                ident,
                ty,
                access,
                user_result_ty,
            } => {
                visit!(v.visit_property(ident, ty, *access, user_result_ty));
                visit!(v.visit_ident(ident));
                visit!(v.visit_type(ty));
                if let Some(ty) = user_result_ty {
                    visit!(v.visit_type(ty));
                }
            }
            ApiItemKind::Stream { ident, ty, is_up } => {
                visit!(v.visit_stream(ident, ty, *is_up));
                visit!(v.visit_ident(ident));
                visit!(v.visit_type(ty));
            }
            ApiItemKind::ImplTrait { args, level } => {
                let level = level.as_ref().expect("");
                visit!(v.visit_impl_trait(args, level));
                visit!(v.after_visit_impl_trait(args, level));
                visit_api_level(level, v);
            }
            ApiItemKind::Reserved => {
                visit!(v.visit_reserved());
            }
        }
        visit!(v.after_visit_api_item(item));
    }
    visit!(v.after_visit_level(level));
}
