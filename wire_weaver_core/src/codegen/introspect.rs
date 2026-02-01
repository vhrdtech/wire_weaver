use crate::ast::api;
use crate::ast::trait_macro_args::{ImplTraitLocation, ImplTraitMacroArgs};
use proc_macro2::TokenStream;
use quote::quote;
use shrink_wrap::{SerializeShrinkWrap, UNib32};

/// Collect information about API items and referenced data types.
/// Serialize into ww_self and create a byte array to be put into device firmware.
pub fn introspect(api_level: &api::ApiLevel, skip_docs: bool) -> TokenStream {
    let mut traits: Vec<(TraitKey, ww_self::ApiLevelLocationOwned, Vec<_>)> = vec![];
    collect_traits(api_level, &mut traits, skip_docs);
    fix_trait_indices(&mut traits);
    let (trait_keys, traits): (Vec<_>, Vec<_>) = traits.into_iter().map(|(k, v, _)| (k, v)).unzip();

    let api_bundle = ww_self::ApiBundleOwned {
        root: convert_level(api_level, &trait_keys, None, skip_docs),
        types: Default::default(),
        traits,
        ext_crates: Default::default(),
    };
    let mut scratch = [0u8; 16_384]; // TODO: use Vec based BufWriter here
    let bytes = api_bundle.to_ww_bytes(&mut scratch).unwrap();
    let len = bytes.len();
    quote! {
        [u8; #len] = [ #(#bytes),* ]
    }
}

#[derive(PartialEq, Eq)]
struct TraitKey {
    location: ImplTraitLocation,
    trait_name: String,
}

impl TraitKey {
    fn from_args(args: &ImplTraitMacroArgs) -> Self {
        TraitKey {
            location: args.location.clone(),
            trait_name: args.trait_name.to_string(),
        }
    }
}

type TraitItemOriginalKeys = Vec<(usize, TraitKey)>;

fn collect_traits(
    api_level: &api::ApiLevel,
    traits: &mut Vec<(
        TraitKey,
        ww_self::ApiLevelLocationOwned,
        TraitItemOriginalKeys,
    )>,
    skip_docs: bool,
) {
    for item in &api_level.items {
        let api::ApiItemKind::ImplTrait { args, level } = &item.kind else {
            continue;
        };
        let level = level.as_ref().expect("");
        collect_traits(level, traits, skip_docs);

        let key = TraitKey::from_args(args);
        if traits.iter().any(|(k, _v, _)| k == &key) {
            continue;
        }

        let mut trait_keys = vec![];
        let converted_level = convert_level(level, &[], Some(&mut trait_keys), skip_docs);
        traits.push((
            key,
            // TODO: ww_self: handle global traits
            ww_self::ApiLevelLocationOwned::InLine(converted_level),
            trait_keys,
        ));
    }
}

/// Correct each ww_self::ApiItemKind::Trait idx to the correct index in the traits array, which were unknown on the first pass.
///
/// traits contains all levels/traits flattened into one array.
/// .2 contains original TraitKey's (crate name + trait name) that level items referred to.
fn fix_trait_indices(
    traits: &mut [(
        TraitKey,
        ww_self::ApiLevelLocationOwned,
        TraitItemOriginalKeys,
    )],
) {
    let mut corrections = vec![];
    for (_, _, trait_keys) in traits.iter() {
        let mut item_corrections = vec![];
        for (item_idx, original_key) in trait_keys {
            let correct_idx = traits
                .iter()
                .position(|(k, _, _)| k == original_key)
                .expect("");
            item_corrections.push((*item_idx, UNib32(correct_idx as u32)));
        }
        corrections.push(item_corrections);
    }
    for (idx, item_corrections) in corrections.into_iter().enumerate() {
        let ww_self::ApiLevelLocationOwned::InLine(level) = &mut traits[idx].1 else {
            continue;
        };
        for (item_idx, correct_trait_idx) in item_corrections {
            let ww_self::ApiItemKindOwned::Trait { idx: placeholder } =
                &mut level.items[item_idx].kind
            else {
                continue;
            };
            *placeholder = correct_trait_idx;
        }
    }
}

fn convert_level(
    level: &api::ApiLevel,
    traits: &[TraitKey],
    mut trait_keys: Option<&mut TraitItemOriginalKeys>,
    skip_docs: bool,
) -> ww_self::ApiLevelOwned {
    let docs = if skip_docs {
        "".into()
    } else {
        level.docs.to_string()
    };
    ww_self::ApiLevelOwned {
        docs,
        ident: level.name.to_string(),
        items: level
            .items
            .iter()
            .enumerate()
            .map(|(item_idx, i)| {
                if let Some(trait_keys) = trait_keys.as_mut()
                    && let api::ApiItemKind::ImplTrait { args, level: _ } = &i.kind
                {
                    let key = TraitKey::from_args(args);
                    trait_keys.push((item_idx, key));
                }
                convert_item(i, traits, skip_docs)
            })
            .collect(),
    }
}

fn convert_item(
    item: &api::ApiItem,
    traits: &[TraitKey],
    skip_docs: bool,
) -> ww_self::ApiItemOwned {
    let (kind, ident) = match &item.kind {
        api::ApiItemKind::Method {
            ident,
            args,
            return_type,
        } => (
            ww_self::ApiItemKindOwned::Method {
                args: args.iter().map(convert_arg).collect(),
                return_ty: return_type.as_ref().map(convert_ty),
            },
            ident.to_string(),
        ),
        api::ApiItemKind::Property {
            ident,
            ty,
            access,
            user_result_ty,
        } => (
            ww_self::ApiItemKindOwned::Property {
                ty: convert_ty(ty),
                access: convert_access(*access),
                user_result_ty: user_result_ty.as_ref().map(convert_ty),
            },
            ident.to_string(),
        ),
        api::ApiItemKind::Stream { ident, ty, is_up } => (
            ww_self::ApiItemKindOwned::Stream {
                ty: convert_ty(ty),
                is_up: *is_up,
            },
            ident.to_string(),
        ),
        api::ApiItemKind::ImplTrait { args, level: _ } => {
            let key = TraitKey::from_args(args);
            let idx = traits.iter().position(|k| k == &key).unwrap_or(65_535);
            (
                ww_self::ApiItemKindOwned::Trait {
                    idx: UNib32(idx as u32),
                },
                args.resource_name.to_string(),
            )
        }
        api::ApiItemKind::Reserved => (ww_self::ApiItemKindOwned::Reserved, "".into()),
    };
    let docs = if skip_docs {
        "".into()
    } else {
        item.docs.to_string()
    };
    ww_self::ApiItemOwned {
        id: UNib32(item.id),
        multiplicity: convert_multiplicity(&item.multiplicity),
        ident,
        docs,
        kind,
    }
}

fn convert_multiplicity(m: &api::Multiplicity) -> ww_self::Multiplicity {
    match m {
        api::Multiplicity::Flat => ww_self::Multiplicity::Flat,
        // TODO: ww_self: convert multiplicity
        api::Multiplicity::Array { size_bound: _ } => ww_self::Multiplicity::Array,
    }
}

fn convert_ty(ty: &shrink_wrap_core::ast::Type) -> ww_self::TypeOwned {
    match ty {
        shrink_wrap_core::ast::Type::Bool => ww_self::TypeOwned::Bool,
        shrink_wrap_core::ast::Type::U4 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::U4)
        }
        shrink_wrap_core::ast::Type::U8 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::U8)
        }
        shrink_wrap_core::ast::Type::U16 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::U16)
        }
        shrink_wrap_core::ast::Type::U32 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::U32)
        }
        shrink_wrap_core::ast::Type::U64 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::U64)
        }
        shrink_wrap_core::ast::Type::U128 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::U128)
        }
        shrink_wrap_core::ast::Type::UNib32 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::UNib32)
        }
        shrink_wrap_core::ast::Type::ULeb32 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::ULeb32)
        }
        shrink_wrap_core::ast::Type::ULeb64 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::ULeb64)
        }
        shrink_wrap_core::ast::Type::ULeb128 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::ULeb128)
        }
        shrink_wrap_core::ast::Type::I4 => unimplemented!(),
        shrink_wrap_core::ast::Type::I8 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::I8)
        }
        shrink_wrap_core::ast::Type::I16 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::I16)
        }
        shrink_wrap_core::ast::Type::I32 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::I32)
        }
        shrink_wrap_core::ast::Type::I64 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::I64)
        }
        shrink_wrap_core::ast::Type::I128 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::I128)
        }
        shrink_wrap_core::ast::Type::ILeb32 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::ILeb32)
        }
        shrink_wrap_core::ast::Type::ILeb64 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::ILeb64)
        }
        shrink_wrap_core::ast::Type::ILeb128 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::ILeb128)
        }
        shrink_wrap_core::ast::Type::F32 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::F32)
        }
        shrink_wrap_core::ast::Type::F64 => {
            ww_self::TypeOwned::NumericBase(ww_self::NumericBaseType::F64)
        }
        shrink_wrap_core::ast::Type::String => ww_self::TypeOwned::String,
        shrink_wrap_core::ast::Type::Array(len, ty) => ww_self::TypeOwned::Array {
            len: u32::try_from(*len).unwrap(),
            ty: Box::new(convert_ty(ty)),
        },
        shrink_wrap_core::ast::Type::Tuple(types) => {
            ww_self::TypeOwned::Tuple(types.iter().map(convert_ty).collect())
        }
        shrink_wrap_core::ast::Type::Vec(ty) => ww_self::TypeOwned::Vec(Box::new(convert_ty(ty))),
        shrink_wrap_core::ast::Type::External(_path, _) => {
            ww_self::TypeOwned::OutOfLine { idx: UNib32(0) }
        } // TODO: ww_self: out of line types
        shrink_wrap_core::ast::Type::Option(_ident, ty) => ww_self::TypeOwned::Option {
            is_flag_on_stack: true,
            some_ty: Box::new(convert_ty(ty)),
        },
        shrink_wrap_core::ast::Type::IsSome(_) => ww_self::TypeOwned::Flag,
        shrink_wrap_core::ast::Type::Result(_, ok_err_ty) => ww_self::TypeOwned::Result {
            is_flag_on_stack: true,
            ok_ty: Box::new(convert_ty(&ok_err_ty.0)),
            err_ty: Box::new(convert_ty(&ok_err_ty.1)),
        },
        shrink_wrap_core::ast::Type::IsOk(_) => ww_self::TypeOwned::Flag,
        shrink_wrap_core::ast::Type::RefBox(_) => ww_self::TypeOwned::Box(Box::new(convert_ty(ty))),
    }
}

fn convert_arg(arg: &api::Argument) -> ww_self::ArgumentOwned {
    ww_self::ArgumentOwned {
        ident: arg.ident.to_string(),
        ty: convert_ty(&arg.ty),
    }
}

fn convert_access(access: api::PropertyAccess) -> ww_self::PropertyAccess {
    match access {
        api::PropertyAccess::Const => ww_self::PropertyAccess::Const,
        api::PropertyAccess::ReadWrite => ww_self::PropertyAccess::ReadWrite,
        api::PropertyAccess::ReadOnly => ww_self::PropertyAccess::ReadOnly,
        api::PropertyAccess::WriteOnly => ww_self::PropertyAccess::WriteOnly,
    }
}
