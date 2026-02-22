#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
pub mod visitor;

use shrink_wrap::prelude::*;
pub use ww_numeric::{NumericAnyType, NumericBaseType};
use ww_version::{CompactVersion, FullVersion};

#[cfg(feature = "std")]
use ww_numeric::NumericAnyTypeOwned;
#[cfg(feature = "std")]
use ww_version::FullVersionOwned;

#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[owned = "std"]
#[serde = "serde"]
pub struct ApiBundle<'i> {
    /// API entry point.
    pub root: ApiLevel<'i>,
    /// Deduplicated array of types collected from all API levels, referred to by [Type::OutOfLine].
    pub types: RefVec<'i, Type<'i>>,
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
    InLine(ApiLevel<'i>),
    SkippedFullVersion {
        version: FullVersion<'i>,
        trait_name: &'i str,
    },
    SkippedCompactVersion {
        version: CompactVersion,
        trait_id: UNib32,
    },
}

#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[owned = "std"]
#[serde = "serde"]
pub struct ApiLevel<'i> {
    pub docs: &'i str,
    pub ident: &'i str,
    // pub source_location?
    pub items: RefVec<'i, ApiItem<'i>>,
}

#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[owned = "std"]
#[serde = "serde"]
pub struct ApiItem<'i> {
    pub id: UNib32,
    pub multiplicity: Multiplicity,
    pub ident: &'i str,
    pub docs: &'i str,
    pub kind: ApiItemKind<'i>,
}

#[derive_shrink_wrap]
#[ww_repr(u2)]
#[derive(Clone, Debug)]
#[serde = "serde"]
pub enum Multiplicity {
    Flat,
    Array, // size bound?
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
        user_result_ty: Option<Type<'i>>,
    },
    Stream {
        ty: Type<'i>,
        is_up: bool,
    },
    Trait {
        idx: UNib32,
    },
    Reserved,
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
#[derive(Clone, Debug)]
#[serde = "serde"]
pub enum PropertyAccess {
    /// Property is not going to change, observe not available
    Const,
    /// Property can only be read, but can change and be observed for changes
    ReadOnly,
    /// Property can be read, written and observed for changes
    ReadWrite,
    /// Property can only be written
    WriteOnly,
}

#[derive_shrink_wrap]
#[ww_repr(unib32)]
#[derive(Clone, Debug)]
#[owned = "std"]
#[serde = "serde"]
pub enum Type<'i> {
    /// 1-bit, alignment of one-bit, same as `UB(UBits(1))` but serialized with only 1 nibble because bool is used very often.
    Bool,

    NumericBase(NumericBaseType),
    NumericAny(NumericAnyType<'i>),

    // Reserved Type with discriminant length of 1 nibble
    // Reserved6,
    // Reserved Type with discriminant length of 1 nibble
    // Reserved7,
    /// Variable length Unicode string
    String,
    /// Variable length `Vec<T>`
    Vec(RefBox<'i, Type<'i>>),
    /// Fixed size array `[T; len]`
    Array {
        len: u32,
        ty: RefBox<'i, Type<'i>>,
    },
    /// Variable length tuple `(T1, T2, ...)`
    Tuple(RefVec<'i, Type<'i>>),
    /// User defined struct
    Struct(ItemStruct<'i>),
    /// User defined enum
    Enum(ItemEnum<'i>),
    /// Flag followed by Optional `T` if true and nothing otherwise.
    Option {
        /// If true then flag is popped from the stack; otherwise it is read from the buffer
        is_flag_on_stack: bool,
        some_ty: RefBox<'i, Type<'i>>,
    },
    /// Flag followed by `T` if flag is true and `E` otherwise.
    Result {
        /// If true then flag is popped from the stack; otherwise it is read from the buffer
        is_flag_on_stack: bool,
        ok_ty: RefBox<'i, Type<'i>>,
        err_ty: RefBox<'i, Type<'i>>,
    },
    /// Read bool and put it onto "flag stack".
    /// When serializing: must do the reverse operation for all Options and Results that have is_flag_on_stack set to true.
    Flag,
    /// Self-referential types using `Box<T>` and `RefBox<'i, T>`
    Box(RefBox<'i, Type<'i>>),
    /// Open range `start..end`
    Range(RefBox<'i, Type<'i>>),
    /// Closed range `start..=end`
    RangeInclusive(RefBox<'i, Type<'i>>),

    /// Type definition from ApiBundle types array.
    OutOfLine {
        idx: UNib32,
    },
}

#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[serde = "serde"]
pub struct TypeMeta<'i> {
    pub def: Type<'i>,
    pub source: TypeDefinitionSource,
}

#[derive_shrink_wrap]
#[ww_repr(u2)]
#[derive(Clone, Debug)]
#[serde = "serde"]
pub enum TypeDefinitionSource {
    /// Type was defined in the same crate as ApiLevel that refers to itl.
    Local,
    /// Type was defined in an external crate that have a global ID assigned to it.
    GlobalCompact(CompactVersion),
    /// Type was defined in an external crate without global ID. One deduplicated array of names is kept in [ApiBundle].
    GlobalFull {
        /// Index into [ApiBundle] ext_crates array.
        idx: u32,
    },
}

#[derive_shrink_wrap]
#[ww_repr(unib32)]
#[derive(Clone, Debug)]
#[owned = "std"]
#[serde = "serde"]
pub enum Value<'i> {
    Bool(bool),
    Numeric(NumericBaseType),
    String(&'i str),
    // TODO: the rest
}

#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[owned = "std"]
#[serde = "serde"]
pub struct ItemStruct<'i> {
    pub size: ElementSize,
    pub docs: &'i str,
    pub ident: &'i str,
    pub fields: RefVec<'i, Field<'i>>,
}

#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[owned = "std"]
#[serde = "serde"]
pub struct Field<'i> {
    pub docs: &'i str,
    pub ident: &'i str,
    pub ty: RefBox<'i, Type<'i>>,
    pub default: Option<Value<'i>>,
}

#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[owned = "std"]
#[serde = "serde"]
pub struct ItemEnum<'i> {
    pub size: ElementSize,
    pub docs: &'i str,
    pub ident: &'i str,
    pub repr: Repr,
    pub variants: RefVec<'i, Variant<'i>>,
}

#[derive_shrink_wrap]
#[ww_repr(u4)]
#[derive(Clone, Debug)]
#[serde = "serde"]
pub enum Repr {
    U(u8),
    UNib32,
}

#[derive_shrink_wrap]
#[derive(Clone, Debug)]
#[owned = "std"]
#[serde = "serde"]
pub struct Variant<'i> {
    pub docs: &'i str,
    pub ident: &'i str,
    pub fields: Fields<'i>,
    pub discriminant: UNib32,
    // pub since: Option<Version>,
}

#[derive_shrink_wrap]
#[ww_repr(u2)]
#[derive(Clone, Debug)]
#[owned = "std"]
#[serde = "serde"]
pub enum Fields<'i> {
    Named(RefVec<'i, Field<'i>>),
    Unnamed(RefVec<'i, Type<'i>>),
    Unit,
}
