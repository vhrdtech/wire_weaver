use crate::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned,
    ArgumentOwned, FieldsOwned, ItemEnumOwned, ItemStructOwned, PropertyAccess, TypeLocationOwned,
    TypeOwned,
};

pub trait VisitMut {
    fn visit_level(&mut self, level: &mut ApiLevelOwned) {
        let _ = level;
    }

    fn visit_doc(&mut self, doc: &mut Vec<String>) {
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
        access: &mut PropertyAccess,
        ty: &mut TypeOwned,
        write_result_ty: &mut Option<TypeOwned>,
    ) {
        let _ = access;
        let _ = ty;
        let _ = write_result_ty;
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
    for ty_location in &mut api_bundle.types {
        match ty_location {
            TypeLocationOwned::InLine { ty, .. } => {
                visit_type(ty, v);
            }
            TypeLocationOwned::SkippedFullVersion { .. } => {
                todo!()
            } // TypeLocationOwned::SkippedCompactVersion { .. } => {
              //     todo!()
              // }
        }
    }
    for level_location in &mut api_bundle.traits {
        match level_location {
            ApiLevelLocationOwned::InLine { level, .. } => {
                visit_level(level, v);
            }
            ApiLevelLocationOwned::SkippedFullVersion { .. } => {
                todo!()
            }
            ApiLevelLocationOwned::SkippedCompactVersion { .. } => {
                todo!()
            }
        }
    }
}

fn visit_level(level: &mut ApiLevelOwned, v: &mut impl VisitMut) {
    v.visit_level(level);
    v.visit_ident(&mut level.trait_name);
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
            access,
            ty,
            write_err_ty,
        } => visit_property(access, ty, write_err_ty, v),
        ApiItemKindOwned::Stream { ty, is_up } => visit_stream(ty, is_up, v),
        ApiItemKindOwned::Trait { .. } => {}
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
    access: &mut PropertyAccess,
    ty: &mut TypeOwned,
    write_err_ty: &mut Option<TypeOwned>,
    v: &mut impl VisitMut,
) {
    v.visit_property(access, ty, write_err_ty);
    visit_type(ty, v);
    if let Some(ty) = write_err_ty {
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
    visit_fields(&mut item_struct.fields, v);
}

fn visit_item_enum(item_enum: &mut ItemEnumOwned, v: &mut impl VisitMut) {
    v.visit_item_enum(item_enum);
    v.visit_ident(&mut item_enum.ident);
    v.visit_doc(&mut item_enum.docs);
    for variant in &mut item_enum.variants {
        v.visit_ident(&mut variant.ident);
        v.visit_doc(&mut variant.docs);
        visit_fields(&mut variant.fields, v);
    }
}

fn visit_fields(fields: &mut FieldsOwned, v: &mut impl VisitMut) {
    match fields {
        FieldsOwned::Named(fields) | FieldsOwned::Unnamed(fields) => {
            for field in fields {
                if let Some(ident) = &mut field.ident {
                    v.visit_ident(ident);
                }
                v.visit_doc(&mut field.docs);
                visit_type(&mut field.ty, v);
            }
        }
        FieldsOwned::Unit => {}
    }
}
