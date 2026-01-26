use crate::ast::api::{ApiItem, ApiItemKind, ApiLevel, Argument, Multiplicity, PropertyAccess};
use crate::codegen::api_client::{ClientModel, ClientPathMode};
use proc_macro2::Ident;
use shrink_wrap_core::ast::Type;

pub trait Visit {
    fn visit_level(&mut self, cx: &Context, level: &ApiLevel) {
        self.visit_items(cx, &level.items);
        self.visit_child_levels(cx, &level.items);
    }

    fn visit_items(&mut self, cx: &Context, items: &[ApiItem]) {
        for item in items {
            match &item.kind {
                ApiItemKind::Method {
                    ident,
                    args,
                    return_type,
                } => {
                    self.visit_method(cx, ident, args, return_type);
                }
                ApiItemKind::Property { ident, ty, access } => {
                    self.visit_property(cx, ident, ty, *access);
                }
                ApiItemKind::Stream { ident, ty, is_up } => {
                    self.visit_stream(cx, ident, ty, *is_up);
                }
                ApiItemKind::ImplTrait { args, level } => {
                    let level = level.as_ref().expect("empty level");
                    self.visit_trait(cx, level);
                }
                ApiItemKind::Reserved => {}
            }
        }
    }

    fn visit_child_levels(&mut self, cx: &Context, items: &[ApiItem]) {
        for item in items {
            let ApiItemKind::ImplTrait { args: _, level } = &item.kind else {
                continue;
            };
            let level = level.as_ref().expect("empty level");
            let mut index_chain_len = cx.index_chain_len + 1;
            if matches!(item.multiplicity, Multiplicity::Array { .. }) {
                index_chain_len += 1;
            }
            let cx = Context {
                index_chain_len,
                model: cx.model,
                path_mode: cx.path_mode,
            };
            self.visit_level(&cx, level);
        }
    }

    fn visit_method(
        &mut self,
        cx: &Context,
        ident: &Ident,
        args: &[Argument],
        return_ty: &Option<Type>,
    ) {
    }

    fn visit_property(&mut self, cx: &Context, ident: &Ident, ty: &Type, access: PropertyAccess) {}

    fn visit_stream(&mut self, cx: &Context, ident: &Ident, ty: &Type, is_up: bool) {}

    fn visit_trait(&mut self, cx: &Context, level: &ApiLevel) {}
}

pub struct Context {
    // resource_id: u32,
    pub index_chain_len: usize,
    // crate_name: Ident,
    pub model: ClientModel,
    pub path_mode: ClientPathMode,
}
