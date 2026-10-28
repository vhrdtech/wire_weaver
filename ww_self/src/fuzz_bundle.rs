//! Introspection data is untrusted input (a device sends it): [fuzz_one] feeds bytes through everything a host does
//! with a bundle, as the `introspection` fuzz target and the unit tests do. Each step returns an error or a result,
//! never a panic, an endless loop, a stack overflow or an allocation driven by the data.
//!
//! The first byte picks how the rest is read: as a serialized bundle (what a device sends), or as a program that
//! builds a bundle with generic definitions, instances and parameters, valid or not (indices out of range, wrong
//! kinds and counts of arguments, cycles, self-referential definitions, deep nesting, growing instances, long names,
//! many fields or variants, wide tuples), which random bytes rarely produce.

use crate::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, FieldOwned, FieldsOwned,
    FutureVersionOwned, GenericArgOwned, GenericParamKindOwned, GenericParamOwned, ItemEnumOwned,
    ItemStructOwned, Length, Multiplicity, PropertyAccess, Repr, TypeLocationOwned, TypeOwned,
    ValueOwned, VariantOwned,
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
        gen_bundle(bundle_part, mode & 2 != 0)
    };
    use_bundle(bundle, value);
}

/// A bundle built from `data` as a program, see the module docs: also for fuzzing what is done with two bundles
/// (compatibility checks). `recursive`: definitions without parameters that refer to each other and themselves,
/// all indices in range, mostly valid.
pub fn gen_bundle(data: &[u8], recursive: bool) -> ApiBundleOwned {
    Gen { data, recursive }.bundle()
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
            // the budget is per byte: what was read from more bytes (trailing ones) may not fit into fewer
            match ValueOwned::des_shrink_wrap_dyn(&bytes, ty, &bundle) {
                Ok(again) => assert!(values_equal(&again, &decoded), "{again:?} != {decoded:?}"),
                Err(e) if bytes.len() < value.len() && format!("{e}").contains("too large") => {}
                Err(e) => panic!("a written value is read back: {e}"),
            }
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
    /// See [gen_bundle].
    recursive: bool,
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

    /// `base`, sometimes followed by up to 64 KB more: names are as long as a device makes them.
    fn name(&mut self, base: String) -> String {
        let b = self.byte();
        if b.is_multiple_of(16) {
            base + &"x".repeat(self.byte() as usize * 256)
        } else {
            base
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
                    batched: None,
                },
                multiplicity: Multiplicity::Flat,
                since: None,
                ident: format!("p{id}"),
                docs: vec![],
                optional: None,
                meta: None,
            });
        }
        ApiBundleOwned {
            magic: crate::MAGIC,
            ww_self_version: crate::VERSION,
            root: ApiLevelOwned {
                docs: vec![],
                crate_idx: UNib32(0),
                trait_name: "Fuzz".into(),
                compact_id: None,
                meta: None,
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
        let b = if self.recursive { b & !0x30 } else { b };
        if b % 16 == 15 && !self.recursive {
            return TypeLocationOwned::SkippedFullVersion {
                crate_idx: UNib32(0),
                type_name: format!("T{idx}"),
                signature: vec![],
            };
        }
        let mut generics = vec![];
        for p in 0..(b >> 4) % 4 {
            let kind = if self.byte().is_multiple_of(3) {
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
                ident: self.name(format!("T{idx}")),
                path: None,
                instance_of: None,
                generics,
                fields,
                sparse: false,
                extra: None,
            })
        } else {
            let mut variants = vec![VariantOwned {
                docs: vec![],
                ident: "A".into(),
                fields,
                discriminant: UNib32(0),
                since: None,
            }];
            // more unit variants, up to a few thousand
            let more = match self.byte() {
                b if b.is_multiple_of(8) => self.byte() as u32 * 16,
                b => (b % 2) as u32,
            };
            for d in 1..=more {
                variants.push(VariantOwned {
                    docs: vec![],
                    ident: format!("B{d}"),
                    fields: FieldsOwned::Unit,
                    discriminant: UNib32(d),
                    since: None,
                });
            }
            let repr = match self.byte() % 4 {
                0 => Repr::BitAligned(self.byte() % 9),
                1 => Repr::UNib32,
                _ => Repr::Nibble,
            };
            TypeOwned::Enum(ItemEnumOwned {
                size: self.size(),
                repr,
                crate_idx: UNib32(0),
                docs: vec![],
                ident: self.name(format!("T{idx}")),
                path: None,
                instance_of: None,
                future_version: match self.byte() % 3 {
                    0 => None,
                    1 => Some(FutureVersionOwned::Off),
                    _ => Some(FutureVersionOwned::Variant {
                        ident: "Unknown".into(),
                    }),
                },
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
        // sometimes many, for a wide fan-out of nested definitions
        let n = match self.byte() {
            b if b & 0x40 != 0 => b % 17,
            b => b % 4,
        };
        for i in 0..n {
            fields.push(FieldOwned {
                ident: Some(self.name(format!("f{i}"))),
                default: None,
                since: None,
                ty: self.ty(defs, params, depth),
                docs: vec![],
                slot: None,
                series_opaque: self.byte().is_multiple_of(8),
            });
        }
        FieldsOwned::Named(fields)
    }

    /// An index in range most of the time, sometimes just past it.
    fn index(&mut self, n: u8) -> UNib32 {
        let past = if self.recursive { 0 } else { 2 };
        UNib32((self.byte() % (n + past).max(1)) as u32)
    }

    fn ty(&mut self, defs: u8, params: u8, depth: u8) -> TypeOwned {
        if self.recursive {
            return self.recursive_ty(defs, depth);
        }
        let leaf = depth > 8;
        let boxed = |s: &mut Self| Box::new(s.ty(defs, params, depth + 1));
        match self.byte() % if leaf { 4 } else { 18 } {
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
            // a wide tuple of one leaf type (a parameter, a reference): names of instances with it repeat its name
            13 => {
                let n = self.byte() % 16 + 1;
                let ty = self.ty(defs, params, 9);
                TypeOwned::Tuple(vec![ty; n as usize])
            }
            14 => TypeOwned::Result {
                ok_ty: boxed(self),
                err_ty: boxed(self),
            },
            15 => TypeOwned::Series(boxed(self)),
            // references behind a Box or a Vec: self-referential definitions that pass validation
            16 => TypeOwned::Option {
                some_ty: Box::new(TypeOwned::Box(Box::new(TypeOwned::OutOfLine {
                    type_idx: self.index(defs),
                }))),
            },
            _ => TypeOwned::Vec(Box::new(TypeOwned::OutOfLine {
                type_idx: self.index(defs),
            })),
        }
    }
}

impl Gen<'_> {
    /// Types without parameters and instances, references to definitions mostly behind a `Box` or a `Vec`.
    fn recursive_ty(&mut self, defs: u8, depth: u8) -> TypeOwned {
        let leaf = depth > 6;
        let out_of_line = |s: &mut Self| TypeOwned::OutOfLine {
            type_idx: s.index(defs),
        };
        match self.byte() % if leaf { 3 } else { 11 } {
            0 => TypeOwned::Bool,
            1 => TypeOwned::NumericAny(NumericAnyTypeOwned::Base(NumericBaseType::U16)),
            2 => TypeOwned::String,
            3 => TypeOwned::Option {
                some_ty: Box::new(TypeOwned::Box(Box::new(out_of_line(self)))),
            },
            4 => TypeOwned::Vec(Box::new(out_of_line(self))),
            5 => TypeOwned::Box(Box::new(out_of_line(self))),
            // directly: a cycle through it is an error
            6 => out_of_line(self),
            7 => TypeOwned::Tuple(vec![
                self.recursive_ty(defs, depth + 1),
                self.recursive_ty(defs, depth + 1),
            ]),
            8 => TypeOwned::Option {
                some_ty: Box::new(self.recursive_ty(defs, depth + 1)),
            },
            // the column decoder of a Series, through the same depth limit
            9 => TypeOwned::Series(Box::new(out_of_line(self))),
            _ => TypeOwned::Array {
                len: Length::Fixed(UNib32((self.byte() % 3) as u32)),
                ty: Box::new(self.recursive_ty(defs, depth + 1)),
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

    /// A few thousand inputs from a fixed generator, the same paths `cargo fuzz run introspection` takes, on the
    /// stack of a spawned thread (2 MiB) in a debug build: self-referential types are walked and decoded as deep as
    /// the bounds allow.
    #[test]
    fn random_inputs() {
        std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(random_inputs_on_thread)
            .unwrap()
            .join()
            .unwrap();
    }

    fn random_inputs_on_thread() {
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
                // serialized, generated, generated recursive
                *mode = [0, 1, 3][i % 3];
            }
            fuzz_one(&data);
        }
        // self-referential bundles with long values
        for _ in 0..300 {
            let len = (next() % 120) as usize;
            let mut data = vec![3u8];
            data.extend_from_slice(&(len as u16).to_le_bytes());
            data.extend((0..len + 4096).map(|_| next() as u8));
            fuzz_one(&data);
        }
    }
}
