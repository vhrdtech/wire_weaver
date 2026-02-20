use crate::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned,
    ArgumentOwned, FieldsOwned, ItemEnumOwned, ItemStructOwned, PropertyAccess, TypeOwned,
};

pub trait VisitMut {
    fn visit_level(&mut self, level: &mut ApiLevelOwned) {
        let _ = level;
    }

    fn visit_doc(&mut self, doc: &mut String) {
        let _ = doc;
    }

    fn visit_ident(&mut self, ident: &mut String) {
        let _ = ident;
    }

    fn visit_item(&mut self, item: &mut ApiItemOwned) {
        let _ = item;
    }

    fn visit_method(&mut self, args: &mut Vec<ArgumentOwned>, return_ty: &mut Option<TypeOwned>) {
        let _ = args;
        let _ = return_ty;
    }

    fn visit_property(
        &mut self,
        ty: &mut TypeOwned,
        access: &mut PropertyAccess,
        user_result_ty: &mut Option<TypeOwned>,
    ) {
        let _ = ty;
        let _ = access;
        let _ = user_result_ty;
    }

    fn visit_stream(&mut self, ty: &mut TypeOwned, is_up: &mut bool) {
        let _ = ty;
        let _ = is_up;
    }

    fn visit_type(&mut self, ty: &mut TypeOwned) {
        let _ = ty;
    }

    fn visit_item_struct(&mut self, item_struct: &mut ItemStructOwned) {
        let _ = item_struct;
    }

    fn visit_item_enum(&mut self, item_enum: &mut ItemEnumOwned) {
        let _ = item_enum;
    }
}

pub fn visit_api_bundle_mut(api_bundle: &mut ApiBundleOwned, v: &mut impl VisitMut) {
    visit_level(&mut api_bundle.root, v);
    for ty in &mut api_bundle.types {
        visit_type(ty, v);
    }
    for level_location in &mut api_bundle.traits {
        match level_location {
            ApiLevelLocationOwned::InLine(level) => {
                visit_level(level, v);
            }
            ApiLevelLocationOwned::SkippedFullVersion { .. } => {}
            ApiLevelLocationOwned::SkippedCompactVersion { .. } => {}
        }
    }
}

fn visit_level(level: &mut ApiLevelOwned, v: &mut impl VisitMut) {
    v.visit_level(level);
    v.visit_ident(&mut level.ident);
    v.visit_doc(&mut level.docs);
    for item in &mut level.items {
        visit_item(item, v);
    }
}

fn visit_item(item: &mut ApiItemOwned, v: &mut impl VisitMut) {
    v.visit_item(item);
    v.visit_ident(&mut item.ident);
    v.visit_doc(&mut item.docs);
    match &mut item.kind {
        ApiItemKindOwned::Method { args, return_ty } => visit_method(args, return_ty, v),
        ApiItemKindOwned::Property {
            ty,
            access,
            user_result_ty,
        } => visit_property(ty, access, user_result_ty, v),
        ApiItemKindOwned::Stream { ty, is_up } => visit_stream(ty, is_up, v),
        ApiItemKindOwned::Trait { .. } => {}
        ApiItemKindOwned::Reserved => {}
    }
}

fn visit_method(
    args: &mut Vec<ArgumentOwned>,
    return_ty: &mut Option<TypeOwned>,
    v: &mut impl VisitMut,
) {
    v.visit_method(args, return_ty);
    for arg in args {
        v.visit_ident(&mut arg.ident);
        visit_type(&mut arg.ty, v);
    }
    if let Some(return_ty) = return_ty {
        visit_type(return_ty, v)
    }
}

fn visit_property(
    ty: &mut TypeOwned,
    access: &mut PropertyAccess,
    user_result_ty: &mut Option<TypeOwned>,
    v: &mut impl VisitMut,
) {
    v.visit_property(ty, access, user_result_ty);
    visit_type(ty, v);
    if let Some(ty) = user_result_ty {
        visit_type(ty, v);
    }
}

fn visit_stream(ty: &mut TypeOwned, is_up: &mut bool, v: &mut impl VisitMut) {
    v.visit_stream(ty, is_up);
    visit_type(ty, v);
}

fn visit_type(ty: &mut TypeOwned, v: &mut impl VisitMut) {
    v.visit_type(ty);
    match ty {
        TypeOwned::Vec(ty) => {
            visit_type(ty, v);
        }
        TypeOwned::Array { ty, .. } => {
            visit_type(ty, v);
        }
        TypeOwned::Tuple(types) => {
            for ty in types {
                visit_type(ty, v);
            }
        }
        TypeOwned::Struct(item_struct) => {
            visit_item_struct(item_struct, v);
        }
        TypeOwned::Enum(item_enum) => {
            visit_item_enum(item_enum, v);
        }
        TypeOwned::Option { some_ty, .. } => visit_type(some_ty, v),
        TypeOwned::Result { ok_ty, err_ty, .. } => {
            visit_type(ok_ty, v);
            visit_type(err_ty, v)
        }
        TypeOwned::Box(ty) => {
            visit_type(ty, v);
        }
        _ => {}
    }
}

fn visit_item_struct(item_struct: &mut ItemStructOwned, v: &mut impl VisitMut) {
    v.visit_item_struct(item_struct);
    v.visit_ident(&mut item_struct.ident);
    v.visit_doc(&mut item_struct.docs);
    for field in &mut item_struct.fields {
        v.visit_ident(&mut field.ident);
        v.visit_doc(&mut field.docs);
        visit_type(&mut field.ty, v);
    }
}

fn visit_item_enum(item_enum: &mut ItemEnumOwned, v: &mut impl VisitMut) {
    v.visit_item_enum(item_enum);
    v.visit_ident(&mut item_enum.ident);
    v.visit_doc(&mut item_enum.docs);
    for variant in &mut item_enum.variants {
        v.visit_ident(&mut variant.ident);
        v.visit_doc(&mut variant.docs);
        match &mut variant.fields {
            FieldsOwned::Named(fields) => {
                for field in fields {
                    v.visit_ident(&mut field.ident);
                    v.visit_doc(&mut field.docs);
                    visit_type(&mut field.ty, v);
                }
            }
            FieldsOwned::Unnamed(types) => {
                for ty in types {
                    visit_type(ty, v);
                }
            }
            FieldsOwned::Unit => {}
        }
    }
}
