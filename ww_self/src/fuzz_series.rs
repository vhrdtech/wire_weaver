//! The dynamic `Series` decoder against the generated one (CLT-29): Rust types with `#[derive_shrink_wrap(..,
//! series)]` and their description in a bundle, as codegen would write it. Items generated from bytes are written by
//! the typed `Series`, read back dynamically, and must equal what the dynamic decoder makes of the same items as a
//! plain `Vec<T>` (which matches the typed plain decode, checked elsewhere); random bytes into both decoders give the
//! same items or both an error. Shared by the unit tests and the `series-dynamic` fuzz target (`cargo fuzz` builds
//! with `--cfg fuzzing`, which makes this module public).

use crate::{
    ApiBundleOwned, ApiLevelOwned, FieldOwned, FieldsOwned, GenericArgOwned, GenericParamKindOwned,
    GenericParamOwned, ItemEnumOwned, ItemStructOwned, Length, Repr, TypeLocationOwned, TypeOwned,
    ValueOwned, VariantOwned,
};
use shrink_wrap::prelude::*;
use shrink_wrap::series::{CodecPolicy, Coder, FloatCodec, NumericCodec, Predictor, SeriesOwned};
use shrink_wrap::{ElementSize, Nibble, UNib32};
use ww_numeric::{NumericAnyTypeOwned, NumericBaseType};
use ww_version::{FullVersionOwned, VersionOwned};

#[derive_shrink_wrap(owned, series, self_describing, ww_repr = unib32, derive(Clone, Copy, Debug, PartialEq))]
pub enum Prefix {
    Unit,
    Milli,
    Micro,
}

// the derive doesn't keep a `#[default]` on a variant
#[allow(clippy::derivable_impls)]
impl Default for Prefix {
    fn default() -> Self {
        Prefix::Unit
    }
}

#[derive_shrink_wrap(owned, series, self_describing, ww_repr = unib32, derive(Clone, Copy, Debug, PartialEq))]
pub enum Storage {
    U8(u8) = 1,
    U16(u16) = 2,
    I32(i32) = 6,
    F32(f32) = 7,
}

#[derive_shrink_wrap(owned, series, final_structure, derive(Clone, Copy, Debug, PartialEq))]
pub struct Quantity {
    pub prefix: Prefix,
    pub value: Storage,
}

/// The design's sample, evolvable.
#[derive_shrink_wrap(owned, series, derive(Clone, Copy, Debug, PartialEq))]
pub struct IvSample {
    pub v: Quantity,
    pub i: Quantity,
}

/// [IvSample] with a field appended.
#[derive_shrink_wrap(owned, series, derive(Clone, Copy, Debug, PartialEq))]
pub struct IvSampleV2 {
    pub v: Quantity,
    pub i: Quantity,
    #[default = None]
    pub range_ok: Option<bool>,
}

#[derive_shrink_wrap(owned, series, final_structure, derive(Clone, Copy, Debug, PartialEq))]
pub struct Timed<T> {
    pub t: Quantity,
    pub value: T,
}

#[derive_shrink_wrap(owned, series, derive(Clone, Debug, PartialEq))]
pub struct InnerOwned {
    pub x: u16,
    #[default = Prefix::Unit]
    pub unit: Prefix,
    #[default = String::new()]
    pub label: String,
}

/// Every kind of leaf and group.
#[derive_shrink_wrap(owned, series, derive(Clone, Debug, PartialEq))]
pub struct KitchenOwned {
    pub flag: bool,
    pub q: Option<Quantity>,
    pub name: String,
    pub pair: (u8, Prefix, i64),
    pub arr: [i16; 3],
    pub f: f32,
    pub d: f64,
    pub big: u128,
    pub nib: Nibble,
    pub bytes: Vec<u8>,
    pub inner: Option<InnerOwned>,
    pub prefixes: [Prefix; 2],
    #[series(opaque)]
    pub tags: Vec<Prefix>,
    pub shape: ShapeOwned,
}

#[derive_shrink_wrap(owned, series, ww_repr = unib32, derive(Clone, Debug, PartialEq))]
pub enum ShapeOwned {
    Empty,
    Circle(u8),
    Rect { w: u16, h: u16, label: String },
}

// ---- the bundle --------------------------------------------------------------------------------------------------

/// Indices of the definitions in [bundle].
pub mod idx {
    pub const PREFIX: u32 = 0;
    pub const STORAGE: u32 = 1;
    pub const QUANTITY: u32 = 2;
    pub const IV_SAMPLE: u32 = 3;
    pub const IV_SAMPLE_V2: u32 = 4;
    pub const TIMED: u32 = 5;
    pub const INNER: u32 = 6;
    pub const KITCHEN: u32 = 7;
    pub const SHAPE: u32 = 8;
}

fn num(base: NumericBaseType) -> TypeOwned {
    TypeOwned::NumericAny(NumericAnyTypeOwned::Base(base))
}

fn def(idx: u32) -> TypeOwned {
    TypeOwned::OutOfLine {
        type_idx: UNib32(idx),
    }
}

fn field(name: Option<&str>, ty: TypeOwned) -> FieldOwned {
    FieldOwned {
        ident: name.map(String::from),
        default: None,
        since: None,
        ty,
        docs: vec![],
        slot: None,
        series_opaque: false,
    }
}

fn named(fields: Vec<FieldOwned>) -> FieldsOwned {
    FieldsOwned::Named(fields)
}

fn unit_variant(ident: &str, d: u32) -> VariantOwned {
    VariantOwned {
        docs: vec![],
        ident: ident.into(),
        fields: FieldsOwned::Unit,
        discriminant: UNib32(d),
        since: None,
    }
}

fn variant(ident: &str, d: u32, fields: FieldsOwned) -> VariantOwned {
    VariantOwned {
        fields,
        ..unit_variant(ident, d)
    }
}

fn st(ident: &str, size: ElementSize, fields: FieldsOwned) -> TypeOwned {
    TypeOwned::Struct(ItemStructOwned {
        size,
        crate_idx: UNib32(0),
        docs: vec![],
        ident: ident.into(),
        path: None,
        generics: vec![],
        fields,
        sparse: false,
    })
}

fn en(ident: &str, size: ElementSize, variants: Vec<VariantOwned>) -> TypeOwned {
    TypeOwned::Enum(ItemEnumOwned {
        size,
        repr: Repr::UNib32,
        crate_idx: UNib32(0),
        docs: vec![],
        ident: ident.into(),
        path: None,
        generics: vec![],
        variants,
    })
}

fn prefix_value(ident: &str) -> ValueOwned {
    ValueOwned::Enum {
        variant: ident.into(),
        fields: crate::FieldsValueOwned::Unit,
    }
}

/// The types above, as codegen describes them.
pub fn bundle() -> ApiBundleOwned {
    use NumericBaseType as B;
    let quantity_fields = || {
        named(vec![
            field(Some("prefix"), def(idx::PREFIX)),
            field(Some("value"), def(idx::STORAGE)),
        ])
    };
    let iv_fields = |v2: bool| {
        let mut fields = vec![
            field(Some("v"), def(idx::QUANTITY)),
            field(Some("i"), def(idx::QUANTITY)),
        ];
        if v2 {
            fields.push(FieldOwned {
                default: Some(ValueOwned::Option(None)),
                ..field(
                    Some("range_ok"),
                    TypeOwned::Option {
                        some_ty: Box::new(TypeOwned::Bool),
                    },
                )
            });
        }
        named(fields)
    };
    let unnamed = |ty| FieldsOwned::Unnamed(vec![field(None, ty)]);
    let timed = ItemStructOwned {
        size: ElementSize::UnsizedFinalStructure,
        crate_idx: UNib32(0),
        docs: vec![],
        ident: "Timed".into(),
        path: None,
        generics: vec![GenericParamOwned {
            ident: "T".into(),
            kind: GenericParamKindOwned::Type,
        }],
        fields: named(vec![
            field(Some("t"), def(idx::QUANTITY)),
            field(Some("value"), TypeOwned::Param(UNib32(0))),
        ]),
        sparse: false,
    };
    let types = vec![
        en(
            "Prefix",
            ElementSize::SelfDescribing,
            vec![
                unit_variant("Unit", 0),
                unit_variant("Milli", 1),
                unit_variant("Micro", 2),
            ],
        ),
        en(
            "Storage",
            ElementSize::SelfDescribing,
            vec![
                variant("U8", 1, unnamed(num(B::U8))),
                variant("U16", 2, unnamed(num(B::U16))),
                variant("I32", 6, unnamed(num(B::I32))),
                variant("F32", 7, unnamed(num(B::F32))),
            ],
        ),
        st(
            "Quantity",
            ElementSize::UnsizedFinalStructure,
            quantity_fields(),
        ),
        st("IvSample", ElementSize::Unsized, iv_fields(false)),
        st("IvSampleV2", ElementSize::Unsized, iv_fields(true)),
        TypeOwned::Struct(timed),
        st(
            "Inner",
            ElementSize::Unsized,
            named(vec![
                field(Some("x"), num(B::U16)),
                FieldOwned {
                    default: Some(prefix_value("Unit")),
                    ..field(Some("unit"), def(idx::PREFIX))
                },
                FieldOwned {
                    default: Some(ValueOwned::String(String::new())),
                    ..field(Some("label"), TypeOwned::String)
                },
            ]),
        ),
        st(
            "Kitchen",
            ElementSize::Unsized,
            named(vec![
                field(Some("flag"), TypeOwned::Bool),
                field(
                    Some("q"),
                    TypeOwned::Option {
                        some_ty: Box::new(def(idx::QUANTITY)),
                    },
                ),
                field(Some("name"), TypeOwned::String),
                field(
                    Some("pair"),
                    TypeOwned::Tuple(vec![num(B::U8), def(idx::PREFIX), num(B::I64)]),
                ),
                field(
                    Some("arr"),
                    TypeOwned::Array {
                        len: Length::Fixed(UNib32(3)),
                        ty: Box::new(num(B::I16)),
                    },
                ),
                field(Some("f"), num(B::F32)),
                field(Some("d"), num(B::F64)),
                field(Some("big"), num(B::U128)),
                field(Some("nib"), num(B::Nibble)),
                field(Some("bytes"), TypeOwned::Vec(Box::new(num(B::U8)))),
                field(
                    Some("inner"),
                    TypeOwned::Option {
                        some_ty: Box::new(def(idx::INNER)),
                    },
                ),
                field(
                    Some("prefixes"),
                    TypeOwned::Array {
                        len: Length::Fixed(UNib32(2)),
                        ty: Box::new(def(idx::PREFIX)),
                    },
                ),
                FieldOwned {
                    series_opaque: true,
                    ..field(Some("tags"), TypeOwned::Vec(Box::new(def(idx::PREFIX))))
                },
                field(Some("shape"), def(idx::SHAPE)),
            ]),
        ),
        en(
            "Shape",
            ElementSize::Unsized,
            vec![
                unit_variant("Empty", 0),
                variant("Circle", 1, unnamed(num(B::U8))),
                variant(
                    "Rect",
                    2,
                    named(vec![
                        field(Some("w"), num(B::U16)),
                        field(Some("h"), num(B::U16)),
                        field(Some("label"), TypeOwned::String),
                    ]),
                ),
            ],
        ),
    ];
    let timed = TypeOwned::Instance {
        type_idx: UNib32(idx::TIMED),
        args: vec![GenericArgOwned::Type(def(idx::IV_SAMPLE))],
    };
    let items = vec![
        property(
            0,
            "timed_series",
            TypeOwned::Series(Box::new(timed.clone())),
        ),
        property(1, "timed_vec", TypeOwned::Vec(Box::new(timed))),
    ];
    bundle_of(types, items)
}

/// A bundle of the definitions `types` and the API `items`, prepared.
pub fn bundle_of(types: Vec<TypeOwned>, items: Vec<crate::ApiItemOwned>) -> ApiBundleOwned {
    let mut bundle = ApiBundleOwned {
        magic: crate::MAGIC,
        ww_self_version: crate::VERSION,
        root: ApiLevelOwned {
            docs: vec![],
            crate_idx: UNib32(0),
            trait_name: "SeriesCheck".into(),
            items,
        },
        types: types
            .into_iter()
            .map(|ty| TypeLocationOwned::InLine {
                ty,
                crate_idx: UNib32(0),
            })
            .collect(),
        traits: vec![],
        ext_crates: vec![FullVersionOwned::new(
            "series_check".into(),
            VersionOwned::new(0, 1, 0),
        )],
        root_omitted: None,
    };
    crate::generics::prepare(&mut bundle).expect("a valid bundle");
    bundle
}

/// `Series<Timed<IvSample>>` and `Vec<Timed<IvSample>>` in a bundle from [bundle]: the types of its two
/// properties, after `prepare` replaced the instances with their own definitions.
pub fn timed_iv(bundle: &ApiBundleOwned) -> (TypeOwned, TypeOwned) {
    let ty = |i: usize| match &bundle.root.items[i].kind {
        crate::ApiItemKindOwned::Property { ty, .. } => ty.clone(),
        _ => unreachable!(),
    };
    (ty(0), ty(1))
}

fn property(id: u32, ident: &str, ty: TypeOwned) -> crate::ApiItemOwned {
    crate::ApiItemOwned {
        id: UNib32(id),
        kind: crate::ApiItemKindOwned::Property {
            ty,
            access: crate::PropertyAccess::Const,
            write_err_ty: None,
            errors: None,
        },
        multiplicity: crate::Multiplicity::Flat,
        since: None,
        ident: ident.into(),
        docs: vec![],
        optional: None,
    }
}

// ---- checks -------------------------------------------------------------------------------------------------------

fn series_of(idx: u32) -> TypeOwned {
    TypeOwned::Series(Box::new(def(idx)))
}

fn vec_of(idx: u32) -> TypeOwned {
    TypeOwned::Vec(Box::new(def(idx)))
}

/// What the dynamic decoder reads from `items` written as a plain `Vec<T>`: what a `Series` of them must give.
fn plain<T: SerializeShrinkWrapOwned + Clone>(
    items: &[T],
    vec_ty: &TypeOwned,
    bundle: &ApiBundleOwned,
) -> ValueOwned
where
    Vec<T>: SerializeShrinkWrapOwned,
{
    let bytes = items.to_vec().to_ww_bytes_owned().expect("plain bytes");
    ValueOwned::des_shrink_wrap_dyn(&bytes, vec_ty, bundle).expect("plain dynamic decode")
}

/// Equal, NaN equal to itself.
fn same(a: &ValueOwned, b: &ValueOwned) -> bool {
    format!("{a:?}") == format!("{b:?}")
}

/// `series` written by the typed `Series` read back by the dynamic decoder, equal to the plain form.
pub fn check_items<T>(
    items: &[T],
    policy: CodecPolicy,
    series_ty: &TypeOwned,
    vec_ty: &TypeOwned,
    bundle: &ApiBundleOwned,
) where
    T: for<'i> shrink_wrap::series::SeriesColumns<'i> + SerializeShrinkWrapOwned + Clone,
    Vec<T>: SerializeShrinkWrapOwned,
{
    let owned = SeriesOwned(items.to_vec());
    let bytes = owned
        .as_borrowed_with(policy)
        .to_ww_bytes_owned()
        .expect("series bytes");
    let dynamic =
        ValueOwned::des_shrink_wrap_dyn(&bytes, series_ty, bundle).expect("dynamic series decode");
    let expected = plain(items, vec_ty, bundle);
    assert!(same(&dynamic, &expected), "{dynamic:?}\n!=\n{expected:?}");
    let columns = ValueOwned::des_series_columns_dyn(&bytes, series_ty, bundle).expect("columns");
    assert_eq!(columns.len, items.len());
    assert!(same(&ValueOwned::Vec(columns.into_items()), &expected));
}

/// Random bytes into the typed and the dynamic decoder: the same items, or both an error (the dynamic one may also
/// refuse a valid `Series` too large for its bytes).
pub fn check_bytes<T>(
    bytes: &[u8],
    series_ty: &TypeOwned,
    vec_ty: &TypeOwned,
    bundle: &ApiBundleOwned,
) where
    T: for<'i> shrink_wrap::series::SeriesColumns<'i>
        + SerializeShrinkWrapOwned
        + DeserializeShrinkWrapOwned
        + Clone,
    Vec<T>: SerializeShrinkWrapOwned,
{
    let typed = SeriesOwned::<T>::from_ww_bytes_owned(bytes);
    let dynamic = ValueOwned::des_shrink_wrap_dyn(bytes, series_ty, bundle);
    match (typed, dynamic) {
        (Ok(items), Ok(dynamic)) => {
            let expected = plain(&items.0, vec_ty, bundle);
            assert!(same(&dynamic, &expected), "{dynamic:?}\n!=\n{expected:?}");
        }
        (Err(_), Err(_)) => {}
        (Ok(items), Err(e)) => {
            let e = format!("{e:#}");
            assert!(
                e.contains("too large") || e.contains("bytes of memory"),
                "typed read {} items, dynamic failed: {e}",
                items.len()
            );
        }
        (Err(e), Ok(v)) => panic!("typed failed with {e:?}, dynamic read {v:?}"),
    }
}

/// Bytes as a source of choices: runs out into zeros.
pub struct Picks<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Picks<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Picks { bytes, pos: 0 }
    }

    pub fn byte(&mut self) -> u8 {
        let b = self.bytes.get(self.pos).copied().unwrap_or(0);
        self.pos += 1;
        b
    }

    fn u64(&mut self) -> u64 {
        (0..8).fold(0, |acc, _| acc << 8 | self.byte() as u64)
    }

    /// Mostly small steps from `prev`, sometimes a jump.
    fn walk(&mut self, prev: i64) -> i64 {
        match self.byte() % 8 {
            0 => self.u64() as i64,
            1 | 2 => prev,
            _ => prev.wrapping_add((self.byte() as i8 / 16) as i64),
        }
    }

    fn prefix(&mut self, prev: Prefix) -> Prefix {
        match self.byte() % 6 {
            0 => Prefix::Unit,
            1 => Prefix::Milli,
            2 => Prefix::Micro,
            _ => prev,
        }
    }

    fn quantity(&mut self, num: i64, prev: Prefix) -> Quantity {
        let prefix = self.prefix(prev);
        let value = match self.byte() % 7 {
            0 => Storage::U8(num as u8),
            1 => Storage::U16(num as u16),
            2 => Storage::F32(num as f32 / 7.0),
            _ => Storage::I32(num as i32),
        };
        Quantity { prefix, value }
    }
}

/// A codec policy from a pick: mostly the smallest, else a fixed one.
pub fn policy(pick: u8) -> CodecPolicy {
    if pick < 128 {
        return CodecPolicy::SMALLEST;
    }
    let predictor = [Predictor::Raw, Predictor::Delta, Predictor::DeltaOfDelta][pick as usize % 3];
    let coder =
        [Coder::Prefix, Coder::Packed, Coder::Constant, Coder::Plain][(pick as usize / 3) % 4];
    let float = if pick & 1 == 0 {
        FloatCodec::Plain
    } else {
        FloatCodec::Xor
    };
    CodecPolicy::fixed(NumericCodec::new(predictor, coder), float)
}

pub fn kitchen_items(picks: &mut Picks<'_>, max: usize) -> Vec<KitchenOwned> {
    let n = picks.byte() as usize % (max + 1);
    let mut items: Vec<KitchenOwned> = Vec::new();
    let mut num = 0i64;
    let mut prev = KitchenOwned {
        flag: false,
        q: None,
        name: String::new(),
        pair: (0, Prefix::Unit, 0),
        arr: [0; 3],
        f: 0.0,
        d: 0.0,
        big: 0,
        nib: Nibble::new_masked(0),
        bytes: vec![],
        inner: None,
        prefixes: [Prefix::Unit; 2],
        tags: vec![],
        shape: ShapeOwned::Empty,
    };
    for _ in 0..n {
        num = picks.walk(num);
        let mut k = prev.clone();
        if picks.byte().is_multiple_of(4) {
            k.flag = !k.flag;
        }
        k.q = match picks.byte() % 4 {
            0 => None,
            _ => Some(picks.quantity(num, k.q.map_or(Prefix::Milli, |q| q.prefix))),
        };
        if picks.byte().is_multiple_of(8) {
            k.name = ["", "a", "volt", "ampere ✓"][picks.byte() as usize % 4].into();
        }
        k.pair = (num as u8, picks.prefix(k.pair.1), num.wrapping_mul(3));
        k.arr = [num as i16, (num >> 3) as i16, picks.byte() as i16 - 128];
        k.f = num as f32 / 3.0;
        k.d = num as f64 * 0.25;
        k.big = (num as u128) << 64 | picks.byte() as u128;
        k.nib = Nibble::new_masked(picks.byte());
        if picks.byte().is_multiple_of(8) {
            k.bytes = (0..picks.byte() % 5).map(|_| picks.byte()).collect();
        }
        k.inner = if picks.byte().is_multiple_of(3) {
            None
        } else {
            Some(InnerOwned {
                x: num as u16,
                unit: picks.prefix(Prefix::Micro),
                label: if picks.byte().is_multiple_of(2) {
                    "x".into()
                } else {
                    String::new()
                },
            })
        };
        k.prefixes = [picks.prefix(k.prefixes[0]), picks.prefix(k.prefixes[1])];
        if picks.byte().is_multiple_of(6) {
            k.tags = (0..picks.byte() % 3)
                .map(|_| picks.prefix(Prefix::Unit))
                .collect();
        }
        k.shape = match picks.byte() % 6 {
            0 => ShapeOwned::Empty,
            1 => ShapeOwned::Circle(num as u8),
            2 => ShapeOwned::Rect {
                w: num as u16,
                h: picks.byte() as u16,
                label: "r".into(),
            },
            _ => k.shape,
        };
        items.push(k.clone());
        prev = k;
    }
    items
}

pub fn iv_items(picks: &mut Picks<'_>, max: usize) -> Vec<IvSampleV2> {
    let n = picks.byte() as usize % (max + 1);
    let (mut v, mut i) = (3300i64, 152_000i64);
    let mut prev = (Prefix::Milli, Prefix::Micro);
    let mut items = Vec::new();
    for _ in 0..n {
        v = picks.walk(v);
        i = picks.walk(i);
        let s = IvSampleV2 {
            v: picks.quantity(v, prev.0),
            i: picks.quantity(i, prev.1),
            range_ok: match picks.byte() % 3 {
                0 => None,
                b => Some(b == 1),
            },
        };
        prev = (s.v.prefix, s.i.prefix);
        items.push(s);
    }
    items
}

/// The fuzz entry point: generated items through the typed writer and the dynamic reader, and the bytes themselves
/// into both readers of several types.
pub fn fuzz_one(bytes: &[u8]) {
    let bundle = bundle();
    let (series_timed, vec_timed) = timed_iv(&bundle);

    check_bytes::<KitchenOwned>(
        bytes,
        &series_of(idx::KITCHEN),
        &vec_of(idx::KITCHEN),
        &bundle,
    );
    check_bytes::<IvSample>(
        bytes,
        &series_of(idx::IV_SAMPLE),
        &vec_of(idx::IV_SAMPLE),
        &bundle,
    );
    check_bytes::<IvSampleV2>(
        bytes,
        &series_of(idx::IV_SAMPLE_V2),
        &vec_of(idx::IV_SAMPLE_V2),
        &bundle,
    );
    check_bytes::<Timed<IvSample>>(bytes, &series_timed, &vec_timed, &bundle);
    check_bytes::<ShapeOwned>(bytes, &series_of(idx::SHAPE), &vec_of(idx::SHAPE), &bundle);

    let mut picks = Picks::new(bytes);
    let policy = policy(picks.byte());
    let kitchen = kitchen_items(&mut picks, 40);
    check_items(
        &kitchen,
        policy,
        &series_of(idx::KITCHEN),
        &vec_of(idx::KITCHEN),
        &bundle,
    );
    let iv = iv_items(&mut picks, 64);
    check_items(
        &iv,
        policy,
        &series_of(idx::IV_SAMPLE_V2),
        &vec_of(idx::IV_SAMPLE_V2),
        &bundle,
    );
    // an older writer's items read as the newer type: the appended field's default
    let old: Vec<IvSample> = iv.iter().map(|s| IvSample { v: s.v, i: s.i }).collect();
    let old_bytes = SeriesOwned(old.clone())
        .to_ww_bytes_owned()
        .expect("series bytes");
    let read_new =
        ValueOwned::des_shrink_wrap_dyn(&old_bytes, &series_of(idx::IV_SAMPLE_V2), &bundle)
            .expect("old bytes as the new type");
    let as_new: Vec<IvSampleV2> = old
        .iter()
        .map(|s| IvSampleV2 {
            v: s.v,
            i: s.i,
            range_ok: None,
        })
        .collect();
    assert!(same(
        &read_new,
        &plain(&as_new, &vec_of(idx::IV_SAMPLE_V2), &bundle)
    ));
    // and the newer writer's as the older type: its columns skipped
    let new_bytes = SeriesOwned(iv.clone())
        .to_ww_bytes_owned()
        .expect("series bytes");
    let read_old = ValueOwned::des_shrink_wrap_dyn(&new_bytes, &series_of(idx::IV_SAMPLE), &bundle)
        .expect("new bytes as the old type");
    assert!(same(
        &read_old,
        &plain(&old, &vec_of(idx::IV_SAMPLE), &bundle)
    ));
    let timed: Vec<Timed<IvSample>> = old
        .iter()
        .enumerate()
        .map(|(k, s)| Timed {
            t: Quantity {
                prefix: Prefix::Micro,
                value: Storage::I32(k as i32 * 1000),
            },
            value: *s,
        })
        .collect();
    check_items(&timed, policy, &series_timed, &vec_timed, &bundle);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SeriesColumn;
    use shrink_wrap::BufWriterOwned;

    fn hex(bytes: &str) -> Vec<u8> {
        (0..bytes.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&bytes[i..i + 2], 16).unwrap())
            .collect()
    }

    fn q(prefix: Prefix, value: Storage) -> Quantity {
        Quantity { prefix, value }
    }

    /// The design's two 4-item examples (`series.md`, "Example: 4 items"), byte for byte.
    #[test]
    fn design_examples() {
        let bundle = bundle();
        let steady: Vec<IvSample> = [
            (3300, 152_000),
            (3301, 152_002),
            (3301, 151_999),
            (3299, 152_001),
        ]
        .iter()
        .map(|&(v, i)| IvSample {
            v: q(Prefix::Milli, Storage::U16(v)),
            i: q(Prefix::Micro, Storage::I32(i)),
        })
        .collect();
        let bytes = hex("42111433905191 8a0004a3801cb0".replace(' ', "").as_str());
        let dynamic =
            ValueOwned::des_shrink_wrap_dyn(&bytes, &series_of(idx::IV_SAMPLE), &bundle).unwrap();
        assert!(same(
            &dynamic,
            &plain(&steady, &vec_of(idx::IV_SAMPLE), &bundle)
        ));

        let switch = vec![
            IvSample {
                v: q(Prefix::Milli, Storage::U16(3300)),
                i: q(Prefix::Micro, Storage::I32(480)),
            },
            IvSample {
                v: q(Prefix::Milli, Storage::U16(3300)),
                i: q(Prefix::Micro, Storage::I32(482)),
            },
            IvSample {
                v: q(Prefix::Milli, Storage::U16(3301)),
                i: q(Prefix::Milli, Storage::U16(120)),
            },
            IvSample {
                v: q(Prefix::Milli, Storage::U16(3300)),
                i: q(Prefix::Milli, Storage::U16(121)),
            },
        ];
        let bytes = hex("4211143390449444d1205e2f225781e200");
        let dynamic =
            ValueOwned::des_shrink_wrap_dyn(&bytes, &series_of(idx::IV_SAMPLE), &bundle).unwrap();
        assert!(same(
            &dynamic,
            &plain(&switch, &vec_of(idx::IV_SAMPLE), &bundle)
        ));
        check_items(
            &switch,
            CodecPolicy::SMALLEST,
            &series_of(idx::IV_SAMPLE),
            &vec_of(idx::IV_SAMPLE),
            &bundle,
        );
    }

    #[test]
    fn generated_items_and_random_bytes() {
        let mut seed = 0x1234_5678_9abc_def0u64;
        for len in [0usize, 1, 2, 7, 64, 300, 1500] {
            for _ in 0..20 {
                let bytes: Vec<u8> = (0..len)
                    .map(|_| {
                        seed ^= seed << 13;
                        seed ^= seed >> 7;
                        seed ^= seed << 17;
                        seed as u8
                    })
                    .collect();
                fuzz_one(&bytes);
            }
        }
    }

    /// A `Series` of `Many` (`FIELDS` fields of type 1) with `items` items, every field in variant 2 of its enum:
    /// per field a tag run of the first tag and the end bit (a unit variant has no group).
    fn all_variant_2(fields: usize, items: u32) -> Vec<u8> {
        let mut wr = BufWriterOwned::new();
        wr.write_unib32(items).unwrap();
        for _ in 0..fields {
            wr.write_un8(4, 2).unwrap();
            wr.write_bool(false).unwrap();
        }
        wr.finish_and_take().unwrap()
    }

    /// An enum's columns list the variants with items, not the type's: a wide enum (a device describes its
    /// types) in many fields is the work and the memory of the items, not of fields x variants.
    #[test]
    fn enum_columns_hold_the_variants_with_items() {
        const VARIANTS: u32 = 100_000;
        const FIELDS: usize = 64;
        let wide = en(
            "Wide",
            ElementSize::SelfDescribing,
            (0..VARIANTS)
                .map(|d| unit_variant(&format!("V{d}"), d))
                .collect(),
        );
        let fields = (0..FIELDS)
            .map(|j| field(Some(&format!("f{j}")), def(1)))
            .collect();
        let many = st("Many", ElementSize::UnsizedFinalStructure, named(fields));
        let bundle = bundle_of(vec![many, wide], vec![]);
        let bytes = all_variant_2(FIELDS, 3);
        let columns = ValueOwned::des_series_columns_dyn(&bytes, &series_of(0), &bundle).unwrap();
        assert_eq!(columns.len, 3);
        let SeriesColumn::Fields { fields, .. } = &columns.root else {
            panic!("{columns:?}");
        };
        assert_eq!(fields.len(), FIELDS);
        for (_, column) in fields {
            let SeriesColumn::Enum {
                variant_of,
                variants,
            } = column
            else {
                panic!("{column:?}");
            };
            assert_eq!(variant_of, &[0, 0, 0]);
            assert_eq!(variants.len(), 1);
            assert_eq!(variants[0].variant_idx, 2);
            assert_eq!(variants[0].ident, "V2");
        }
        let items = ValueOwned::des_shrink_wrap_dyn(&bytes, &series_of(0), &bundle).unwrap();
        let ValueOwned::Vec(items) = items else {
            panic!("{items:?}");
        };
        assert_eq!(items.len(), 3);
        let ValueOwned::Struct {
            fields: crate::FieldsValueOwned::Named(fields),
        } = &items[1]
        else {
            panic!("{items:?}");
        };
        assert_eq!(fields.len(), FIELDS);
        assert_eq!(fields[FIELDS - 1].0, "f63");
        assert_eq!(fields[FIELDS - 1].1, prefix_value("V2"));
    }

    /// The restarts of a field come from the enum and `Option` fields declared before it: a struct with many of
    /// them is the work of their columns, not of the fields squared (every tag column re-walked for every later
    /// field: the budget bounds fields x items, so many fields over a few items).
    #[test]
    fn sibling_restarts_are_linear_in_the_fields() {
        const FIELDS: usize = 20_000;
        const ITEMS: usize = 16;
        let fields = (0..FIELDS)
            .map(|j| {
                field(
                    Some(&format!("f{j}")),
                    TypeOwned::Option {
                        some_ty: Box::new(TypeOwned::Bool),
                    },
                )
            })
            .collect();
        let flags = st("Flags", ElementSize::UnsizedFinalStructure, named(fields));
        let bundle = bundle_of(vec![flags], vec![]);
        // every field: `Some(true)` at every fourth item from the first, `None` elsewhere (a tag change at
        // every `Some` and after it), and the bool column over the `Some` items, all true
        let is_some = |i: usize| i.is_multiple_of(4);
        let mut wr = BufWriterOwned::new();
        wr.write_unib32(ITEMS as u32).unwrap();
        for _ in 0..FIELDS {
            wr.write_un8(4, 1).unwrap();
            let mut last_change = 0;
            for i in 1..ITEMS {
                if is_some(i) != is_some(i - 1) {
                    // a change: the gap from the one before (the value is left out, it flips)
                    wr.write_bool(true).unwrap();
                    wr.write_un8(4, (i - last_change - 1) as u8).unwrap();
                    last_change = i;
                }
            }
            wr.write_bool(false).unwrap();
            wr.write_bool(true).unwrap();
            wr.write_bool(false).unwrap();
        }
        let bytes = wr.finish_and_take().unwrap();
        let start = std::time::Instant::now();
        let columns = ValueOwned::des_series_columns_dyn(&bytes, &series_of(0), &bundle).unwrap();
        let took = start.elapsed();
        assert!(took < std::time::Duration::from_secs(2), "took {took:?}");
        assert_eq!(columns.len, ITEMS);
        let SeriesColumn::Fields { fields, .. } = &columns.root else {
            panic!("{columns:?}");
        };
        assert_eq!(fields.len(), FIELDS);
        let SeriesColumn::Option { some, inner } = &fields[FIELDS - 1].1 else {
            panic!("{:?}", fields[FIELDS - 1]);
        };
        assert_eq!(some.iter().filter(|&&s| s).count(), ITEMS / 4);
        assert_eq!(inner.len(), ITEMS / 4);
    }

    /// A series of items whose columns take no bits (constant) is bounded by the budget, not by its few bytes.
    #[test]
    fn amplification_is_bounded() {
        let bundle = bundle();
        let (series_timed, _) = timed_iv(&bundle);
        let sample = IvSample {
            v: q(Prefix::Milli, Storage::U16(3300)),
            i: q(Prefix::Micro, Storage::I32(152_000)),
        };
        let item = Timed {
            t: q(Prefix::Micro, Storage::I32(0)),
            value: sample,
        };
        let items = vec![item; shrink_wrap::series::SERIES_MAX_ITEMS];
        let fixed = CodecPolicy::fixed(
            NumericCodec::new(Predictor::Raw, Coder::Constant),
            FloatCodec::Plain,
        );
        let bytes = SeriesOwned(items)
            .as_borrowed_with(fixed)
            .to_ww_bytes_owned()
            .unwrap();
        assert!(bytes.len() < 1000, "{} bytes", bytes.len());
        let e = ValueOwned::des_shrink_wrap_dyn(&bytes, &series_timed, &bundle).unwrap_err();
        assert!(format!("{e}").contains("too large"), "{e}");
        // the typed decoder takes them all (validated in one pass, items decoded lazily)
        assert_eq!(
            shrink_wrap::series::Series::<Timed<IvSample>>::from_ww_bytes(&bytes)
                .unwrap()
                .len(),
            shrink_wrap::series::SERIES_MAX_ITEMS
        );
    }
}
