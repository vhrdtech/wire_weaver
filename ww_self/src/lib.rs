#![cfg_attr(not(feature = "std"), no_std)]
// The model's enums are `no_future_version` (SW-47): a bundle carries `ww_self_version`, and every reader walks the
// whole model, so a newer kind of type or item can't be skipped as `Unknown(_)`. Tolerating newer model variants is
// INT-8.

#[cfg(feature = "std")]
mod alloc;
#[cfg(feature = "std")]
pub use alloc::StreamFnRef;
mod common;
#[cfg(feature = "std")]
pub use common::COMMON_ERROR;
#[cfg(feature = "std")]
pub mod definition_cache;
#[cfg(feature = "std")]
pub mod errors;
#[cfg(feature = "std")]
pub mod inline;
#[cfg(feature = "std")]
pub mod signature;
#[cfg(feature = "std")]
mod value;
#[cfg(feature = "std")]
mod visitor;
#[cfg(feature = "std")]
pub use visitor::{visit, visit_mut};

use shrink_wrap::prelude::*;
pub use ww_numeric::{NumericAnyType, NumericBaseType};
use ww_version::{CompactVersion, FullVersion, VersionTriplet};

#[cfg(feature = "std")]
use ww_numeric::NumericAnyTypeOwned;
use ww_numeric::NumericValue;
#[cfg(feature = "std")]
use ww_version::FullVersionOwned;

pub const MAGIC: u32 = 0xA91B_14F0;
/// Version of this crate, stored in [ApiBundle::ww_self_version], so that a reader knows which format a bundle
/// (saved file or introspection data) was written in.
pub const VERSION: VersionTriplet = VersionTriplet::new(
    parse_u32(env!("CARGO_PKG_VERSION_MAJOR")),
    parse_u32(env!("CARGO_PKG_VERSION_MINOR")),
    parse_u32(env!("CARGO_PKG_VERSION_PATCH")),
);

const fn parse_u32(s: &str) -> u32 {
    let b = s.as_bytes();
    let mut i = 0;
    let mut n = 0u32;
    while i < b.len() {
        n = n * 10 + (b[i] - b'0') as u32;
        i += 1;
    }
    n
}

// TODO: add doc
// TODO: add ufs

#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug)
)]
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
    /// Ids of [optional](ApiItem::optional) items of `root` that this device leaves out (API-16), same as
    /// [ApiItemKind::Trait] `omitted` for the other levels. `None` and an empty list both mean none.
    #[default = None]
    pub root_omitted: Option<RefVec<'i, UNib32>>,
    // Hash of ApiBundle with docs present (for a bundle with no docs) and vice versa.
    // pub toggle_doc_hash: ApiHash<'i>,
}

#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug),
    ww_repr = u2,
    no_future_version
)]
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

#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug),
    ww_repr = u2,
    no_future_version
)]
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

#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug)
)]
pub struct ApiLevel<'i> {
    pub docs: RefVec<'i, &'i str>,
    pub crate_idx: UNib32,
    pub trait_name: &'i str,
    pub items: RefVec<'i, ApiItem<'i>>,
}

#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug)
)]
pub struct ApiItem<'i> {
    pub id: UNib32,
    pub kind: ApiItemKind<'i>,
    pub multiplicity: Multiplicity,
    pub since: Option<VersionTriplet>,
    pub ident: &'i str,
    pub docs: RefVec<'i, &'i str>,
    /// `Some` for an item marked `#[optional]` or `#[optional(group)]` in its trait (API-16): a device may leave it
    /// out (`ww_codegen!(.., omit = [..])`), then a call answers `Unimplemented`.
    #[default = None]
    pub optional: Option<Optional<'i>>,
}

/// One entry of a method's error list (`#[errors(Common(InUse, Refused), Nack)]`, API-32): a variant of the
/// method's error enum, and for a variant holding one enum (`Common(CommonError)`), the variants of that one.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq)
)]
pub struct ErrorVariant<'i> {
    /// Name of the variant of the error enum.
    pub ident: &'i str,
    /// Variants of the enum in the variant's only field (`Common(InUse, Refused)`), `None` for all of them
    /// (`Common`).
    pub nested: Option<RefVec<'i, &'i str>>,
}

/// Marks an [ApiItem] as optional (`#[optional]`, `#[optional(group)]`), see `docs/api/optional_items.md`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq)
)]
pub struct Optional<'i> {
    /// Items of one group are left out together, `omit = ["<trait>.<group>"]`.
    pub group: Option<&'i str>,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq),
    self_describing,
    ww_repr = u2,
    no_future_version
)]
pub enum Multiplicity {
    Flat,
    Array { index_type_idx: Option<UNib32> },
}

#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug),
    ww_repr = nib,
    no_future_version
)]
pub enum ApiItemKind<'i> {
    Method {
        args: RefVec<'i, Argument<'i>>,
        return_ty: Option<Type<'i>>,
        /// Variants of the `Err` type of `return_ty` this method returns, `#[errors(..)]` in its trait (API-32), in
        /// the order written. `None` without `#[errors]`: any variant. A device may still return others (only the
        /// client checks), see `docs/api/errors.md#which-errors-a-method-returns`.
        #[default = None]
        errors: Option<RefVec<'i, ErrorVariant<'i>>>,
    },
    Property {
        ty: Type<'i>,
        access: PropertyAccess,
        write_err_ty: Option<Type<'i>>,
        /// Variants of `write_err_ty` a write (set) returns, `#[errors(..)]` above `property!` in its trait (API-32),
        /// as for [Method](ApiItemKind::Method).
        #[default = None]
        errors: Option<RefVec<'i, ErrorVariant<'i>>>,
    },
    Stream {
        ty: Type<'i>,
        is_up: bool,
    },
    Trait {
        trait_idx: UNib32,
        /// Ids of [optional](ApiItem::optional) items of this trait that the device leaves out (API-16): a call to
        /// one answers `Unimplemented`. `None` and an empty list both mean none; a trait referenced in a crate's own
        /// model (not a device's) always has `None`.
        #[default = None]
        omitted: Option<RefVec<'i, UNib32>>,
    },
    /// Sink (host to device) with flow control, e.g. `#[credit] sink!(..)`. A plain sink is
    /// `Stream { is_up: false }`.
    FlowSink {
        ty: Type<'i>,
        flow: FlowControl,
    },
    /// Stream function (device to host, API-31): `#[stream(T, delivery)] fn name(args) -> R`, or `#[counted]` /
    /// `#[reliable] stream!(name: T)`; opened by calling it, see `docs/api/stream_reliability.md`. A plain
    /// `stream!(name: T)` without arguments and result is `Stream { is_up: true }`.
    StreamFn {
        ty: Type<'i>,
        delivery: Delivery,
        args: RefVec<'i, Argument<'i>>,
        return_ty: Option<Type<'i>>,
        /// Variants of the `Err` type of `return_ty` opening the stream returns, `#[errors(..)]` in its trait
        /// (API-32), as for [Method](ApiItemKind::Method).
        #[default = None]
        errors: Option<RefVec<'i, ErrorVariant<'i>>>,
    },
    // Reserved, no useful info keeping reserved items after IDs are calculated
}

/// Delivery kind of a [ApiItemKind::StreamFn].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Copy, Debug, PartialEq, Eq),
    ww_repr = nib,
    no_future_version
)]
pub enum Delivery {
    /// Latest value wins: no positions, nothing counted (events are `StreamData`).
    Plain,
    /// Every event has a position and the units the device dropped before it: losses are seen exactly, nothing is
    /// resent.
    Counted,
    /// Counted, acknowledged and resent; kept in the device's store while the host is slow or gone.
    Reliable,
}

/// Flow control of a [ApiItemKind::FlowSink].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Copy, Debug, PartialEq, Eq),
    ww_repr = nib,
    no_future_version
)]
pub enum FlowControl {
    /// The device grants credit in bytes, each write carries its byte position, lost writes are sent again from the
    /// last accepted position (`docs/api/streams.md`).
    Credit,
}

#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug)
)]
pub struct Argument<'i> {
    pub ident: &'i str,
    pub ty: Type<'i>,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Copy, Debug),
    self_describing,
    ww_repr = u3,
    no_future_version
)]
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

#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq),
    ww_repr = unib32,
    no_future_version
)]
pub enum Type<'i> {
    /// 1-bit, alignment of one-bit, same as `UB(UBits(1))` but serialized with only 1 nibble because bool is used very often.
    Bool,
    /// Any numeric type (u8, i16, i2, f32, shift-scale, etc.)
    NumericAny(NumericAnyType<'i>),
    /// Type definition from ApiBundle types array.
    OutOfLine { type_idx: UNib32 },
    /// Relocated presence flag (`#[flag] name: bool`) of the `Option` or `Result` field with the same name that
    /// comes later in the same named fields. A bool on the wire, that field is then written without its own flag.
    /// Only a named field can have this type, it's not a part of the value (and is not in [FieldsValue]).
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
    /// The flag is elsewhere if there is a [Type::Flag] field with the same name before this one.
    Option { some_ty: RefBox<'i, Type<'i>> },
    /// Flag followed by `T` if the flag is true and `E` otherwise.
    /// The flag is elsewhere if there is a [Type::Flag] field with the same name before this one.
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
    /// [String](Type::String) or [Vec](Type::Vec) `ty` with at most `max_len` elements (bytes for a string),
    /// `MaxLen<T, N>`. Same bytes on the wire as `ty`, a longer value is an error.
    MaxLen {
        max_len: UNib32,
        ty: RefBox<'i, Type<'i>>,
    },
    /// List of integers (`U8`..`U64`, `I8`..`I64`) written as differences to the previous one in a bit-level prefix
    /// code, `Delta<'i, T>` / `DeltaOwned<T>` (SW-31). Same layout around the elements as [Vec](Type::Vec).
    Delta(NumericBaseType),
    /// Same as [Delta](Type::Delta), but differences of differences (the Gorilla timestamp code),
    /// `DeltaOfDelta<'i, T>` / `DeltaOfDeltaOwned<T>`.
    DeltaOfDelta(NumericBaseType),
    /// List of `F32` or `F64` written as XOR with the previous value (Gorilla), `XorFloat<'i, T>` /
    /// `XorFloatOwned<T>` (SW-32).
    XorFloat(NumericBaseType),
}

#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq),
    ww_repr = unib32,
    no_future_version
)]
pub enum Value<'i> {
    Bool(bool),
    Numeric(NumericValue),
    String(&'i str),
    Vec(RefVec<'i, Value<'i>>),
    Array(RefVec<'i, Value<'i>>),
    Tuple(RefVec<'i, Value<'i>>),
    Struct {
        fields: FieldsValue<'i>,
    },
    Enum {
        variant: &str,
        fields: FieldsValue<'i>,
    },
    Option(Option<RefBox<'i, Value<'i>>>),
    Result(Result<RefBox<'i, Value<'i>>, RefBox<'i, Value<'i>>>),
    Range(Range<NumericValue>),
    RangeInclusive(RangeInclusive<NumericValue>),
}

#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq),
    ww_repr = u2,
    no_future_version
)]
pub enum FieldsValue<'i> {
    Named(RefVec<'i, (&'i str, Value<'i>)>),
    Unnamed(RefVec<'i, Value<'i>>),
    Unit,
}

#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq)
)]
pub struct ItemStruct<'i> {
    pub size: ElementSize,
    pub crate_idx: UNib32,
    pub docs: RefVec<'i, &'i str>,
    pub ident: &'i str,
    pub fields: Fields<'i>,
    /// `#[derive_shrink_wrap(sparse, ..)]` (DER-13): every field is an `Option<T>`, written as one slot of an
    /// `OptionAnyVec` (its [Field::slot]) instead of the plain, sequential field encoding. A dynamic decoder
    /// (TOOL-11) must know this to read the struct's bytes correctly.
    #[default = false]
    pub sparse: bool,
}

#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq)
)]
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
    /// The field's slot in its owning [ItemStruct], when it is [sparse](ItemStruct::sparse): its `#[slot = N]`, or
    /// the previous field's slot + 1 (0 for the first), as `shrink_wrap_derive` assigns it. `None` for a field of a
    /// non-`sparse` struct.
    #[default = None]
    pub slot: Option<UNib32>,
}

#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq)
)]
pub struct ItemEnum<'i> {
    pub size: ElementSize,
    pub repr: Repr,
    pub crate_idx: UNib32,
    pub docs: RefVec<'i, &'i str>,
    pub ident: &'i str,
    pub variants: RefVec<'i, Variant<'i>>,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq),
    self_describing,
    ww_repr = u3,
    no_future_version
)]
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

#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq)
)]

pub struct Variant<'i> {
    pub docs: RefVec<'i, &'i str>,
    pub ident: &'i str,
    pub fields: Fields<'i>,
    pub discriminant: UNib32,
    pub since: Option<VersionTriplet>,
}

#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq),
    ww_repr = u2,
    no_future_version
)]
pub enum Fields<'i> {
    Named(RefVec<'i, Field<'i>>),
    Unnamed(RefVec<'i, Field<'i>>),
    Unit,
}
