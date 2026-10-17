//! Introspection data is untrusted input (a device sends it): [fuzz_one] feeds bytes through everything a host does
//! with a bundle, as the `introspection` fuzz target and the unit tests do. Each step returns an error or a result,
//! never a panic, an endless loop, a stack overflow or an allocation driven by the data.
//!
//! The first byte picks how the rest is read: as a serialized bundle (what a device sends), or as a program that
//! builds a bundle with generic definitions, instances and parameters, valid or not (indices out of range, wrong
//! kinds and counts of arguments, cycles, deep nesting, growing instances), which random bytes rarely produce.

use crate::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, FieldOwned, FieldsOwned,
    GenericArgOwned, GenericParamKindOwned, GenericParamOwned, ItemEnumOwned, ItemStructOwned,
    Length, Multiplicity, PropertyAccess, Repr, TypeLocationOwned, TypeOwned, ValueOwned,
    VariantOwned,
};
use shrink_wrap::{DeserializeShrinkWrapOwned, ElementSize, UNib32};
use ww_numeric::{NumericAnyTypeOwned, NumericBaseType, NumericValue};
use ww_version::{FullVersionOwned, VersionOwned};

/// Runs `data` through decoding, [prepare](crate::generics::prepare) and the dynamic decoder.
pub fn fuzz_one(data: &[u8]) {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let (bundle_part, value) = split(rest);
    let bundle = if mode & 1 == 0 {
        match ApiBundleOwned::from_ww_bytes_owned(bundle_part) {
            Ok(bundle) => bundle,
            Err(_) => return,
        }
    } else {
        Gen { data: bundle_part }.bundle()
    };
    use_bundle(bundle, value);
}

/// Two bytes of length, then the bundle part, the rest is value data.
fn split(data: &[u8]) -> (&[u8], &[u8]) {
    let Some((len, rest)) = data.split_first_chunk::<2>() else {
        return (data, &[]);
    };
    let len = (u16::from_le_bytes(*len) as usize).min(rest.len());
    rest.split_at(len)
}

fn use_bundle(mut bundle: ApiBundleOwned, value: &[u8]) {
    if crate::generics::prepare(&mut bundle).is_err() {
        return;
    }
    let mut types = vec![];
    for item in &bundle.root.items {
        if let ApiItemKindOwned::Property { ty, .. } = &item.kind {
            types.push(ty.clone());
        }
    }
    for location in &bundle.types {
        if let TypeLocationOwned::InLine { ty, .. } = location {
            types.push(ty.clone());
        }
    }
    for ty in &types {
        let _ = ty.human_name(true, &bundle);
        let _ = ty.human_definition(&bundle, true);
        let _ = ty.is_lifetime(&bundle);
        let _ = ty.is_unsized(&bundle);
        let _ = ValueOwned::default(ty, &bundle);
        // what was read and can be written (not a variant from a newer version, `<unknown N>`) is read back
        if let Ok(decoded) = ValueOwned::des_shrink_wrap_dyn(value, ty, &bundle)
            && let Ok(bytes) = decoded.ser_shrink_wrap_dyn(ty, &bundle)
        {
            let again = ValueOwned::des_shrink_wrap_dyn(&bytes, ty, &bundle)
                .expect("a written value is read back");
            assert!(
                values_equal(&again, &decoded),
                "{again:?} != {decoded:?}"
            );
        }
    }
}

/// Equal, NaN equal to itself.
fn values_equal(a: &ValueOwned, b: &ValueOwned) -> bool {
    format!("{a:?}") == format!("{b:?}")
}

/// Builds a bundle from bytes, see the module docs. Runs out of bytes as zeros.
struct Gen<'a> {
    data: &'a [u8],
}

impl Gen<'_> {
    fn byte(&mut self) -> u8 {
        match self.data.split_first() {
            Some((&b, rest)) => {
                self.data = rest;
                b
            }
            None => 0,
        }
    }

    fn bundle(&mut self) -> ApiBundleOwned {
        let defs = self.byte() % 8 + 1;
        let mut types = Vec::with_capacity(defs as usize);
        for i in 0..defs {
            types.push(self.def(i, defs));
        }
        let mut items = vec![];
        for id in 0..self.byte() % 4 + 1 {
            let ty = self.ty(defs, 0, 0);
            items.push(ApiItemOwned {
                id: UNib32(id as u32),
                kind: ApiItemKindOwned::Property {
                    ty,
                    access: PropertyAccess::Const,
                    write_err_ty: None,
                    errors: None,
                },
                multiplicity: Multiplicity::Flat,
                since: None,
                ident: format!("p{id}"),
                docs: vec![],
                optional: None,
            });
        }
        ApiBundleOwned {
            magic: crate::MAGIC,
            ww_self_version: crate::VERSION,
            root: ApiLevelOwned {
                docs: vec![],
                crate_idx: UNib32(0),
                trait_name: "Fuzz".into(),
                items,
            },
            types,
            traits: vec![],
            ext_crates: vec![FullVersionOwned::new(
                "fuzz".into(),
                VersionOwned::new(0, 1, 0),
            )],
            root_omitted: None,
        }
    }

    fn def(&mut self, idx: u8, defs: u8) -> TypeLocationOwned {
        let b = self.byte();
        if b % 16 == 15 {
            return TypeLocationOwned::SkippedFullVersion {
                crate_idx: UNib32(0),
                type_name: format!("T{idx}"),
                signature: vec![],
            };
        }
        let mut generics = vec![];
        for p in 0..(b >> 4) % 4 {
            let kind = if self.byte() % 3 == 0 {
                GenericParamKindOwned::Const {
                    ty: match self.byte() % 4 {
                        0 => TypeOwned::Bool,
                        1 => TypeOwned::String,
                        _ => u64_ty(),
                    },
                }
            } else {
                GenericParamKindOwned::Type
            };
            generics.push(GenericParamOwned {
                ident: format!("P{p}"),
                kind,
            });
        }
        let params = generics.len() as u8;
        let fields = self.fields(defs, params, 1);
        let ty = if b & 0x8 == 0 {
            TypeOwned::Struct(ItemStructOwned {
                size: self.size(),
                crate_idx: UNib32(0),
                docs: vec![],
                ident: format!("T{idx}"),
                generics,
                fields,
                sparse: false,
            })
        } else {
            let mut variants = vec![VariantOwned {
                docs: vec![],
                ident: "A".into(),
                fields,
                discriminant: UNib32(0),
                since: None,
            }];
            if self.byte() % 2 == 0 {
                variants.push(VariantOwned {
                    docs: vec![],
                    ident: "B".into(),
                    fields: FieldsOwned::Unit,
                    discriminant: UNib32(1),
                    since: None,
                });
            }
            TypeOwned::Enum(ItemEnumOwned {
                size: self.size(),
                repr: Repr::Nibble,
                crate_idx: UNib32(0),
                docs: vec![],
                ident: format!("T{idx}"),
                generics,
                variants,
            })
        };
        TypeLocationOwned::InLine {
            ty,
            crate_idx: UNib32(0),
        }
    }

    fn size(&mut self) -> ElementSize {
        match self.byte() % 3 {
            0 => ElementSize::Unsized,
            1 => ElementSize::UnsizedFinalStructure,
            _ => ElementSize::SelfDescribing,
        }
    }

    fn fields(&mut self, defs: u8, params: u8, depth: u8) -> FieldsOwned {
        let mut fields = vec![];
        for i in 0..self.byte() % 4 {
            fields.push(FieldOwned {
                ident: Some(format!("f{i}")),
                default: None,
                since: None,
                ty: self.ty(defs, params, depth),
                docs: vec![],
                slot: None,
            });
        }
        FieldsOwned::Named(fields)
    }

    /// An index in range most of the time, sometimes just past it.
    fn index(&mut self, n: u8) -> UNib32 {
        UNib32((self.byte() % (n + 2)) as u32)
    }

    fn ty(&mut self, defs: u8, params: u8, depth: u8) -> TypeOwned {
        let leaf = depth > 8;
        let boxed = |s: &mut Self| Box::new(s.ty(defs, params, depth + 1));
        match self.byte() % if leaf { 4 } else { 14 } {
            0 => TypeOwned::Bool,
            1 => TypeOwned::NumericAny(NumericAnyTypeOwned::Base(NumericBaseType::U16)),
            2 => TypeOwned::Param(self.index(params)),
            3 => TypeOwned::OutOfLine {
                type_idx: self.index(defs),
            },
            4 | 5 => {
                let type_idx = self.index(defs);
                let mut args = vec![];
                for _ in 0..self.byte() % 4 {
                    args.push(match self.byte() % 4 {
                        0 => GenericArgOwned::Const(ValueOwned::Numeric(NumericValue::U64(
                            (self.byte() % 8) as u64,
                        ))),
                        1 => GenericArgOwned::ConstParam(self.index(params)),
                        _ => GenericArgOwned::Type(self.ty(defs, params, depth + 1)),
                    });
                }
                TypeOwned::Instance { type_idx, args }
            }
            6 => TypeOwned::Vec(boxed(self)),
            7 => {
                let len = match self.byte() % 4 {
                    0 => Length::Param(self.index(params)),
                    1 => Length::Fixed(UNib32(u32::MAX)),
                    _ => Length::Fixed(UNib32((self.byte() % 5) as u32)),
                };
                TypeOwned::Array {
                    len,
                    ty: boxed(self),
                }
            }
            8 => TypeOwned::Tuple(vec![
                self.ty(defs, params, depth + 1),
                self.ty(defs, params, depth + 1),
            ]),
            9 => TypeOwned::Option {
                some_ty: boxed(self),
            },
            10 => TypeOwned::Box(boxed(self)),
            11 => TypeOwned::MaxLen {
                max_len: Length::Param(self.index(params)),
                ty: Box::new(TypeOwned::String),
            },
            12 => TypeOwned::String,
            _ => TypeOwned::Result {
                ok_ty: boxed(self),
                err_ty: boxed(self),
            },
        }
    }
}

fn u64_ty() -> TypeOwned {
    TypeOwned::NumericAny(NumericAnyTypeOwned::Base(NumericBaseType::U64))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A few thousand inputs from a fixed generator, the same paths `cargo fuzz run introspection` takes.
    #[test]
    fn random_inputs() {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for i in 0..4000 {
            let len = (next() % 200) as usize;
            let mut data: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            if let Some(mode) = data.first_mut() {
                *mode = (i % 2) as u8 | (*mode & !1);
            }
            fuzz_one(&data);
        }
    }
}
