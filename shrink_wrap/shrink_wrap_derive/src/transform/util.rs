use crate::ast::docs::Docs;
use crate::ast::item_struct::Field;
use crate::ast::object_size::ObjectSize;
use crate::ast::ty::Type;
use crate::transform::syn_util::{
    collect_docs_attrs, take_default_attr, take_flag_attr, take_id_attr, take_series_attr,
    take_since_attr, take_wire_attr,
};
use crate::transform::transform_ty::transform_type;
use proc_macro2::{Ident, Span};

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) enum FieldPathRoot {
    NamedField(Ident),
    EnumVariant(Ident),
    Argument(Ident),
    Output,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) enum FieldSelector {
    NamedField(Ident),
    Tuple(u32),
    Array(usize),
    // ResultIfOk,
    // ResultIfErr,
    // OptionIsSome,
}

#[derive(Debug)]
pub(crate) struct FieldPath {
    root: FieldPathRoot,
    selectors: Vec<FieldSelector>,
}

impl FieldPath {
    pub(crate) fn new(root: FieldPathRoot) -> Self {
        FieldPath {
            root,
            selectors: vec![],
        }
    }

    #[allow(dead_code)]
    pub(crate) fn push(&mut self, selector: FieldSelector) {
        self.selectors.push(selector);
    }

    pub(crate) fn clone_and_push(&self, selector: FieldSelector) -> Self {
        FieldPath {
            root: self.root.clone(),
            selectors: self.selectors.iter().cloned().chain([selector]).collect(),
        }
    }

    pub(crate) fn flag_ident(&self) -> Ident {
        match &self.root {
            FieldPathRoot::NamedField(ident) | FieldPathRoot::Argument(ident) => {
                let ident = ident.to_string();
                Ident::new(format!("_{ident}_flag").as_str(), Span::call_site())
            }
            FieldPathRoot::EnumVariant(_enum_variant_name) => match self.selectors.first() {
                Some(FieldSelector::NamedField(ident)) => {
                    let ident = ident.to_string();
                    Ident::new(format!("_{ident}_flag").as_str(), Span::call_site())
                }
                Some(FieldSelector::Tuple(idx)) => {
                    Ident::new(format!("_{}", idx).as_str(), Span::call_site())
                }
                _ => Ident::new("_todo_flag", Span::call_site()),
            },
            FieldPathRoot::Output => Ident::new("_output_flag", Span::call_site()),
        }
    }
}

/// Create flags for Result or Option fields without explicitly defined ones.
pub(crate) fn create_flags(fields: &mut Vec<Field>, explicit_flags: &[Ident]) {
    let mut fields_without_flags = vec![];
    for (idx, f) in fields.iter().enumerate() {
        let is_flag_ty = matches!(f.ty, Type::Result(_, _) | Type::Option(_, _));
        if is_flag_ty && !explicit_flags.iter().any(|i| i == &f.ident) {
            fields_without_flags.push((idx, matches!(f.ty, Type::Result(_, _)), f.ident.clone()));
        }
    }
    for (shift, (pos, is_result, ident)) in fields_without_flags.into_iter().enumerate() {
        let flag_ident = Ident::new(format!("_{}_flag", ident).as_str(), ident.span());
        let flag = Field {
            docs: Docs::empty(),
            _id: 0, // TODO: Adjust auto created flag IDs
            ident: flag_ident,
            ty: if is_result {
                Type::IsOk(ident)
            } else {
                Type::IsSome(ident)
            },
            _since: None,
            default: None,
            series_opaque: false,
            attrs: vec![],
        };
        fields.insert(pos + shift, flag);
    }
}

pub(crate) fn create_tuple_flags(fields: &[Type]) -> Vec<Type> {
    let mut fields_plus_flags = vec![];
    for (idx, ty) in fields.iter().enumerate() {
        let is_flag_ty = matches!(ty, Type::Result(_, _) | Type::Option(_, _));
        if is_flag_ty {
            let flag_ident = Ident::new(format!("_{idx}").as_str(), Span::call_site());
            let is_result_ty = matches!(ty, Type::Result(_, _));
            fields_plus_flags.push(if is_result_ty {
                Type::IsOk(flag_ident)
            } else {
                Type::IsSome(flag_ident)
            });
            fields_plus_flags.push(ty.clone());
        } else {
            fields_plus_flags.push(ty.clone());
        }
    }
    fields_plus_flags
}

/// Check that using stack for flags will work
pub(crate) fn check_flag_order(fields: &[Field]) -> Result<(), String> {
    let mut flags_stack = vec![];
    for field in fields.iter() {
        match &field.ty {
            Type::Result(_, _) | Type::Option(_, _) => {
                let Some(flag_ident) = flags_stack.pop() else {
                    return Err(format!(
                        "incorrect flag order (no flag), expected LIFO: {}",
                        field.ident
                    ));
                };
                if flag_ident != field.ident {
                    return Err(format!(
                        "incorrect flag order, expected LIFO: {}",
                        field.ident
                    ));
                }
            }
            Type::IsOk(ident) | Type::IsSome(ident) => {
                flags_stack.push(ident.clone());
            }
            _ => {}
        }
    }
    Ok(())
}

/// `UVlq32Backfill::backfill()` patches the first 5 bytes of a buffer, so the field must be first on the wire,
/// directly and not inside Option, Vec, tuple, etc. Must run after flags are created, as they are inserted
/// in front of their Option or Result. Enum fields come after the discriminant, so `first_allowed` is false for them.
pub(crate) fn check_backfill_position(fields: &[Field], first_allowed: bool) -> Result<(), String> {
    for (idx, field) in fields.iter().enumerate() {
        if !field.ty.contains_backfill() {
            continue;
        }
        let is_plain = matches!(field.ty, Type::External(_, _));
        if !(first_allowed && idx == 0 && is_plain) {
            return Err(format!(
                "UVlq32Backfill must be the first field of a struct and not nested in another type: {}",
                field.ident
            ));
        }
    }
    Ok(())
}

/// A `TailSize<N>` slot bounds the rest of the value for a reader, so there is at most one per struct or enum
/// variant, as a plain field, and every field before it must be `Sized` or `SelfDescribing`: a `Vec`, `String` or
/// `Unsized` field keeps its length at the back of the buffer, which a reader bounded by nothing but the slot can't
/// find when the buffer is longer than the value. Known types are checked here; user types before the slot get a
/// const assert in the generated code. Returns the index of the slot field, if any.
pub(crate) fn check_tail_size_position(
    types: &[&Type],
    names: &[String],
) -> Result<Option<usize>, String> {
    let mut slot = None;
    for (idx, ty) in types.iter().enumerate() {
        if !ty.contains_tail_size() {
            continue;
        }
        if slot.is_some() {
            return Err(format!(
                "at most one TailSize per struct or enum variant: {}",
                names[idx]
            ));
        }
        if !matches!(ty, Type::TailSize(_)) {
            return Err(format!(
                "TailSize must be a plain field, not inside Option, Vec, Result, tuples or arrays: {}",
                names[idx]
            ));
        }
        slot = Some(idx);
    }
    let Some(slot) = slot else {
        return Ok(None);
    };
    for (ty, name) in types.iter().zip(names).take(slot) {
        if let Some(ObjectSize::Unsized | ObjectSize::UnsizedFinalStructure) = ty.element_size() {
            return Err(format!(
                "field `{name}` before the TailSize slot must be Sized or SelfDescribing (no Vec, String or Unsized type): \
its length would be at the back of the buffer, where a reader bounded by the slot alone can't find it"
            ));
        }
    }
    Ok(Some(slot))
}

/// `TailBytes` takes all the bytes left in a buffer, so the field must be last on the wire, directly and not inside
/// Option, Vec, tuple, etc. Flags are inserted in front of their Option or Result, so the last field stays last.
/// Same for struct fields and enum variant fields: the variant is the last thing written for an enum.
pub(crate) fn check_tail_bytes_position(types: &[&Type]) -> Result<(), String> {
    for (idx, ty) in types.iter().enumerate() {
        if !ty.contains_tail_bytes() {
            continue;
        }
        let is_plain = matches!(ty, Type::External(_, _));
        if !(idx == types.len() - 1 && is_plain) {
            return Err(
                "TailBytes must be the last field of a struct or enum variant and not nested in another type".into(),
            );
        }
    }
    Ok(())
}

// fn transform_const(item_const: &syn::ItemConst) -> Result<ItemConst, String> {
//     let ty = transform_type(
//         item_const.ty.deref().clone(),
//         None,
//         &FieldPath::new(FieldPathRoot::Argument(item_const.ident.clone())),
//     )?;
//     let mut attrs = item_const.attrs.clone();
//     let docs = collect_docs_attrs(&mut attrs);
//     collect_unknown_attributes(&mut attrs);
//     Ok(ItemConst {
//         docs,
//         ident: item_const.ident.clone().into(),
//         ty,
//         value: item_const.expr.deref().clone(),
//     })
// }

/// Signedness of an integer type that can be a `#[wire(W)]` field or its wire type (DER-19), `None` for any other
/// type. `wire` picks the wire side: no 128-bit types there (`WireAs` goes through 64 bits), but sub-byte, nibble and
/// variable-length ones; the field side is the Rust integers.
fn wire_int_signed(ty: &Type, wire: bool) -> Option<bool> {
    match ty {
        Type::U8 | Type::U16 | Type::U32 | Type::U64 => Some(false),
        Type::I8 | Type::I16 | Type::I32 | Type::I64 => Some(true),
        Type::U128 if !wire => Some(false),
        Type::I128 if !wire => Some(true),
        Type::Nibble | Type::UNib32 | Type::UVlq32 | Type::UVlq64 if wire => Some(false),
        Type::External(path, false) if path.args.is_empty() => {
            let name = path.segments.last()?.to_string();
            if !wire {
                return match name.as_str() {
                    "usize" if path.segments.len() == 1 => Some(false),
                    "isize" if path.segments.len() == 1 => Some(true),
                    _ => None,
                };
            }
            let (signed, bits) = if let Some(bits) = name.strip_prefix('U') {
                (false, bits)
            } else {
                (true, name.strip_prefix('I')?)
            };
            let bits: u8 = bits.parse().ok()?;
            let min = if signed { 2 } else { 1 };
            (min..=64).contains(&bits).then_some(signed)
        }
        _ => None,
    }
}

/// The type of a `#[wire(W)] field: T` (DER-19), checked: both integers of the same signedness.
fn wire_field_type(plain: Type, wire: Type, cfg: Option<syn::Meta>) -> Result<Type, String> {
    let Some(plain_signed) = wire_int_signed(&plain, false) else {
        return Err(
            "#[wire(W)] goes on a plain integer field (u8..u128, i8..i128, usize, isize)".into(),
        );
    };
    let Some(wire_signed) = wire_int_signed(&wire, true) else {
        return Err(
            "#[wire(W)]: W is an integer type of 64 bits or less: u8..u64, i8..i64, Nibble, \
                    U1..U64, I2..I64, UNib32, UVlq32 or UVlq64"
                .into(),
        );
    };
    if plain_signed != wire_signed {
        return Err(format!(
            "#[wire(W)]: the field is {} and W is {}, both must be signed or both unsigned",
            if plain_signed { "signed" } else { "unsigned" },
            if wire_signed { "signed" } else { "unsigned" },
        ));
    }
    Ok(Type::Wire {
        plain: Box::new(plain),
        wire: Box::new(wire),
        cfg,
    })
}

pub(crate) fn transform_field(
    def_order_idx: u32,
    field: &syn::Field,
    path: &FieldPath,
) -> Result<(Field, bool), String> {
    let mut field = field.clone();
    let ident = field.ident.clone().unwrap_or(Ident::new(
        format!("_{def_order_idx}").as_str(),
        Span::call_site(),
    ));

    let path = if let Some(ident) = field.ident {
        path.clone_and_push(FieldSelector::NamedField(ident))
    } else {
        path.clone_and_push(FieldSelector::Tuple(def_order_idx))
    };

    let ty = transform_type(field.ty, Some(&mut field.attrs), &path)?;
    let ty = match take_wire_attr(&mut field.attrs).map_err(|e| format!("field `{ident}`: {e}"))? {
        Some((wire, cfg)) => {
            let wire = transform_type(wire, None, &path)?;
            wire_field_type(ty, wire, cfg).map_err(|e| format!("field `{ident}`: {e}"))?
        }
        None => ty,
    };
    let default = take_default_attr(&mut field.attrs)?;
    let series_opaque =
        take_series_attr(&mut field.attrs).map_err(|e| format!("field `{ident}`: {e}"))?;
    let flag = take_flag_attr(&mut field.attrs);
    let id = take_id_attr(&mut field.attrs).unwrap_or(def_order_idx);
    let since = take_since_attr(&mut field.attrs)?;
    let docs = collect_docs_attrs(&mut field.attrs);

    if flag.is_some() {
        if !matches!(ty, Type::Bool) {
            return Err(format!("{ident:?} flag type is not bool"));
        }
        if series_opaque {
            return Err(format!(
                "field `{ident}`: a #[flag] has no column of its own in a Series, #[series(opaque)] goes on the \
                 Option or Result"
            ));
        }
        let result_ident = ident;
        let ident = if flag.is_some() {
            Ident::new(
                format!("_{}_flag", result_ident).as_str(),
                result_ident.span(),
            )
        } else {
            result_ident.clone()
        };

        Ok((
            Field {
                docs,
                _id: id,
                ident,
                ty: Type::IsOk(result_ident),
                _since: since,
                default,
                series_opaque,
                attrs: field.attrs,
            },
            true,
        ))
    } else {
        Ok((
            Field {
                docs,
                _id: id,
                ident,
                ty,
                _since: since,
                default,
                series_opaque,
                attrs: field.attrs,
            },
            false,
        ))
    }
}
