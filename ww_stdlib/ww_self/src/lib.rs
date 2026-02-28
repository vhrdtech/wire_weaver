#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
pub mod visitor;

use shrink_wrap::prelude::*;
pub use ww_numeric::{NumericAnyType, NumericBaseType};
use ww_version::{CompactVersion, FullVersion, VersionTriplet};

#[cfg(feature = "std")]
use ww_numeric::NumericAnyTypeOwned;
#[cfg(feature = "std")]
use ww_version::FullVersionOwned;

pub const MAGIC: u32 = 0xA91B_14F0;
pub const VERSION: VersionTriplet = VersionTriplet::new(0, 1, 1); // TODO: Fill properly

#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[owned = "std"]
#[serde = "serde"]
pub struct ApiBundle<'i> {
    /// [MAGIC] value
    pub magic: u32,
    /// ww_self version used when generating this bundle.
    pub ww_self_version: VersionTriplet,
    /// API entry point.
    pub root: ApiLevel<'i>,
    /// Deduplicated array of types collected from all API levels, referred to by [Type::OutOfLine].
    pub types: RefVec<'i, TypeLocation<'i>>,
    /// Deduplicated array of traits collected from all API levels, referred to by [ApiItemKind::Trait].
    pub traits: RefVec<'i, ApiLevelLocation<'i>>,
    /// Deduplicated array of all external dependencies, referred to by [TypeDefinitionSource::GlobalFull].
    pub ext_crates: RefVec<'i, FullVersion<'i>>,
}

#[derive_shrink_wrap]
#[ww_repr(u2)]
#[derive(Clone, Debug)]
#[owned = "std"]
#[serde = "serde"]
pub enum ApiLevelLocation<'i> {
    InLine {
        level: ApiLevel<'i>,
        crate_idx: UNib32,
    },
    SkippedFullVersion {
        crate_idx: UNib32,
        trait_name: &'i str,
        signature: RefVec<'i, u8>,
    },
    SkippedCompactVersion {
        version: CompactVersion,
        trait_id: UNib32,
        signature: RefVec<'i, u8>,
    },
}

#[derive_shrink_wrap]
#[ww_repr(u2)]
#[derive(Clone, Debug)]
#[owned = "std"]
#[serde = "serde"]
pub enum TypeLocation<'i> {
    InLine {
        ty: Type<'i>,
        crate_idx: UNib32,
    },
    SkippedFullVersion {
        crate_idx: UNib32,
        type_name: &'i str,
        signature: RefVec<'i, u8>,
    },
    // SkippedCompactVersion {
    //     version: CompactVersion,
    //     type_id: UNib32,
    //     signature: RefVec<'i, u8>,
    // },
}

#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[owned = "std"]
#[serde = "serde"]
pub struct ApiLevel<'i> {
    pub docs: RefVec<'i, &'i str>,
    pub crate_idx: UNib32, // TODO: remove and use one in ApiLevelLocation?
    pub trait_name: &'i str,
    pub items: RefVec<'i, ApiItem<'i>>,
}

#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[owned = "std"]
#[serde = "serde"]
pub struct ApiItem<'i> {
    pub id: UNib32,
    pub kind: ApiItemKind<'i>,
    pub multiplicity: Multiplicity,
    pub since: Option<VersionTriplet>,
    pub ident: &'i str,
    pub docs: RefVec<'i, &'i str>,
}

#[derive_shrink_wrap]
#[ww_repr(u2)]
#[self_describing]
#[derive(Clone, Debug)]
#[serde = "serde"]
pub enum Multiplicity {
    Flat,
    Array { index_type_idx: Option<UNib32> },
}

#[derive_shrink_wrap]
#[ww_repr(u4)]
#[derive(Clone, Debug)]
#[owned = "std"]
#[serde = "serde"]
pub enum ApiItemKind<'i> {
    Method {
        args: RefVec<'i, Argument<'i>>,
        return_ty: Option<Type<'i>>,
    },
    Property {
        ty: Type<'i>,
        access: PropertyAccess,
        write_err_ty: Option<Type<'i>>,
    },
    Stream {
        ty: Type<'i>,
        is_up: bool,
    },
    Trait {
        trait_idx: UNib32,
    },
    // Reserved, no useful info keeping reserved items after IDs are calculated
}

#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[owned = "std"]
#[serde = "serde"]
pub struct Argument<'i> {
    pub ident: &'i str,
    pub ty: Type<'i>,
}

#[derive_shrink_wrap]
#[ww_repr(u3)]
#[sized]
#[derive(Clone, Debug)]
#[serde = "serde"]
pub enum PropertyAccess {
    /// Property is not going to change, observe not available
    Const,
    /// Property can only be read but can change on the server and be observed for changes
    ReadOnly { observe: bool },
    /// Property can be read, written and observed for changes
    ReadWrite { observe: bool },
    /// Property can only be written
    WriteOnly,
}

#[derive_shrink_wrap]
#[ww_repr(unib32)]
#[derive(Clone, Debug, PartialEq)]
#[owned = "std"]
#[serde = "serde"]
pub enum Type<'i> {
    /// 1-bit, alignment of one-bit, same as `UB(UBits(1))` but serialized with only 1 nibble because bool is used very often.
    Bool,
    /// Any numeric type (u8, i16, i2, f32, shift-scale, etc.)
    NumericAny(NumericAnyType<'i>),
    /// Type definition from ApiBundle types array.
    OutOfLine { type_idx: UNib32 },
    /// Read bool and put it onto "flag stack".
    /// When serializing: must do the reverse operation for all Options and Results.
    Flag,
    /// Variable length Unicode string
    String,
    /// Variable length `Vec<T>`
    Vec(RefBox<'i, Type<'i>>),
    /// Fixed size array `[T; len]`
    Array {
        len: UNib32,
        ty: RefBox<'i, Type<'i>>,
    },
    /// Variable length tuple `(T1, T2, ...)`
    Tuple(RefVec<'i, Type<'i>>),
    /// User defined struct
    Struct(ItemStruct<'i>),
    /// User defined enum
    Enum(ItemEnum<'i>),
    /// Flag followed by Optional `T` if true and nothing otherwise.
    /// If the flag stack is empty, the flag is ready right away, otherwise taken from the stack
    Option { some_ty: RefBox<'i, Type<'i>> },
    /// Flag followed by `T` if the flag is true and `E` otherwise.
    /// If the flag stack is empty, the flag is ready right away, otherwise taken from the stack
    Result {
        ok_ty: RefBox<'i, Type<'i>>,
        err_ty: RefBox<'i, Type<'i>>,
    },
    /// Self-referential types using `Box<T>` and `RefBox<'i, T>`
    Box(RefBox<'i, Type<'i>>),
    /// Open range `start..end`
    Range(RefBox<'i, NumericBaseType>),
    /// Closed range `start..=end`
    RangeInclusive(RefBox<'i, NumericBaseType>),
}

#[derive_shrink_wrap]
#[ww_repr(unib32)]
#[derive(Clone, Debug, PartialEq)]
#[owned = "std"]
#[serde = "serde"]
pub enum Value<'i> {
    Bool(bool),
    Numeric(NumericBaseType),
    String(&'i str),
    // TODO: the rest
}

#[derive_shrink_wrap]
#[derive(Clone, Debug, PartialEq)]
#[owned = "std"]
#[serde = "serde"]
pub struct ItemStruct<'i> {
    pub size: ElementSize,
    // pub source: TypeDefinitionSource,
    pub docs: RefVec<'i, &'i str>,
    pub ident: &'i str,
    pub fields: Fields<'i>,
}

#[derive_shrink_wrap]
#[derive(Clone, Debug, PartialEq)]
#[owned = "std"]
#[serde = "serde"]
pub struct Field<'i> {
    #[flag]
    since: bool,
    #[flag]
    default: bool,

    pub ident: Option<&'i str>,
    pub default: Option<Value<'i>>,
    pub since: Option<VersionTriplet>,
    pub ty: Type<'i>,
    pub docs: RefVec<'i, &'i str>,
}

#[derive_shrink_wrap]
#[derive(Clone, Debug, PartialEq)]
#[owned = "std"]
#[serde = "serde"]
pub struct ItemEnum<'i> {
    pub size: ElementSize,
    pub repr: Repr,
    // pub source: TypeDefinitionSource,
    pub docs: RefVec<'i, &'i str>,
    pub ident: &'i str,
    pub variants: RefVec<'i, Variant<'i>>,
}

#[derive_shrink_wrap]
#[ww_repr(u4)] // TODO: actually leave u3 or u4 here
#[sized]
#[derive(Clone, Debug, PartialEq)]
#[serde = "serde"]
pub enum Repr {
    /// One nibble with one nibble alignment
    Nibble,
    /// Ux bits with 1-bit alignment
    BitAligned(u8),
    /// Variable length encoding
    UNib32,
    /// u8 with 1-byte alignment
    ByteAlignedU8,
    /// u16 with 1-byte alignment
    ByteAlignedU16,
    /// u32 with 1-byte alignment
    ByteAlignedU32,
}

#[derive_shrink_wrap]
#[derive(Clone, Debug, PartialEq)]
#[owned = "std"]
#[serde = "serde"]
pub struct Variant<'i> {
    pub docs: RefVec<'i, &'i str>,
    pub ident: &'i str,
    pub fields: Fields<'i>,
    pub discriminant: UNib32,
    pub since: Option<VersionTriplet>,
}

#[derive_shrink_wrap]
#[ww_repr(u2)]
#[derive(Clone, Debug, PartialEq)]
#[owned = "std"]
#[serde = "serde"]
pub enum Fields<'i> {
    Named(RefVec<'i, Field<'i>>),
    Unnamed(RefVec<'i, Field<'i>>),
    Unit,
}
