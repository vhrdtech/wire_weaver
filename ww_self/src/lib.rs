#![cfg_attr(not(feature = "std"), no_std)]
// The model's enums are `no_future_version` (SW-47): a bundle carries `ww_self_version`, and every reader walks the
// whole model, so a newer kind of type or item can't be skipped as `Unknown(_)`. Tolerating newer model variants is
// INT-8.

#[cfg(feature = "std")]
mod alloc;
#[cfg(feature = "std")]
pub use alloc::{HUMAN_DEFINITION_MAX_NODES, StreamFnRef, qualified_name};
mod common;
#[cfg(feature = "std")]
pub use common::COMMON_ERROR;
#[cfg(feature = "std")]
pub mod definition_cache;
#[cfg(feature = "std")]
pub mod errors;
#[cfg(feature = "std")]
pub mod explain;
#[cfg(feature = "expr")]
pub mod expr;
#[cfg(all(feature = "std", any(test, fuzzing, feature = "fuzz")))]
#[doc(hidden)]
pub mod fuzz_bundle;
/// The dynamic `Series` decoder against the generated one, for the `series-dynamic` fuzz target.
#[cfg(all(feature = "std", any(test, fuzzing)))]
#[doc(hidden)]
pub mod fuzz_series;
#[cfg(feature = "std")]
pub mod generics;
#[cfg(feature = "std")]
pub mod inline;
#[cfg(feature = "std")]
mod series;
#[cfg(feature = "std")]
pub use series::{DecodedSeries, FieldsKind, SeriesColumn, VariantColumns};
#[cfg(feature = "std")]
mod series_write;
#[cfg(feature = "std")]
pub use series_write::ENCODE_MAX_VALUES;
#[cfg(feature = "std")]
pub mod signature;
#[cfg(feature = "std")]
pub mod snapshot;
#[cfg(feature = "std")]
mod value;
#[cfg(feature = "std")]
pub use value::{DEFAULT_MAX_VALUES, MAX_DECODE_DEPTH};
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
    /// The trait's id in the `ww_global` registry (`crate::Trait`), when it has one: servers answer
    /// `PathKind::GlobalCompact` requests with it (API-20). `None` in bundles made before ids existed.
    #[default = None]
    pub compact_id: Option<UNib32>,
    /// Attributes of the trait definition beyond the wire contract (`#[claim(..)]`, SRV-34), applying to every
    /// `ww_impl!` of the trait. `None`: none.
    #[default = None]
    pub meta: Option<Meta<'i>>,
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
    /// Attributes beyond the wire contract (`#[claim(..)]`, `#[shares(..)]`, `#[label(..)]`, SRV-34): what a person
    /// or the hub needs to know about the item. `None`: none.
    #[default = None]
    pub meta: Option<Meta<'i>>,
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

/// Typed attributes of an [ApiItem] or an [ApiLevel] that don't change the wire contract (SRV-34): checked by the
/// codegen like `#[optional]` and `#[errors]`, carried in introspection, read by the hub, `ww` and GUIs. See
/// `docs/api/metadata.md`. New attributes are new fields here.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq, Default)
)]
pub struct Meta<'i> {
    /// `#[claim(mode)]` on a `ww_impl!` or a trait definition: a client can claim the resource through the hub, and
    /// what the claim takes from other clients. On a trait definition it applies to every `ww_impl!` of the trait,
    /// one on the `ww_impl!` wins. A device's root is always claimable ([ClaimMode::Exclusive]).
    pub claim: Option<ClaimMode>,
    /// `#[shares(..)]` on a `ww_impl!`: pins and peripherals the resource uses that others may use too, so the hub
    /// refuses a claim that overlaps one held on another resource (a UART and the GPIO bank of its pins).
    pub shares: RefVec<'i, Shares<'i>>,
    /// `#[label(..)]`: what a person calls the item ("CAN1, DB-9 on the front"), shown by `ww`, in `InUse` errors
    /// and GUIs. One label, or one per element of a resource array, by index.
    pub labels: RefVec<'i, &'i str>,
}

/// What a claim on a resource takes from other clients (SRV-34), `#[claim(exclusive | shared | observe)]`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Copy, Debug, PartialEq, Eq),
    ww_repr = nib,
    no_future_version
)]
pub enum ClaimMode {
    /// One client at a time: other clients' calls and property writes at or under the resource are refused, their
    /// reads, gets, streams and observing stay open. `#[claim]` alone.
    Exclusive,
    /// Several clients may hold a claim together (tools that cooperate on a bus); calls and writes of clients
    /// without one are refused, and an exclusive claim waits until the last shared one is released.
    Shared,
    /// Exclusive, and reads, gets, streams and observing are taken too: for a resource where reading has an effect
    /// (a FIFO that a read empties, a capture another client would restart).
    Observe,
}

/// Pins and peripherals a resource uses (SRV-34), `#[shares(pins = "PA9, PA10", peripherals = "USART1")]`: two
/// resources naming the same pin or peripheral conflict, so the hub doesn't let two clients claim them at once.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq)
)]
pub struct Shares<'i> {
    /// Element of a resource array this applies to (`index = 2`), `None`: the resource, every element of an array.
    pub index: Option<UNib32>,
    /// Pin names as the board knows them (`PA9`), ranges like `PA0-PA15` already expanded.
    pub pins: RefVec<'i, &'i str>,
    /// Peripherals or any other named thing the resource needs (`USART1`, `DMA1`, `5V rail`).
    pub peripherals: RefVec<'i, &'i str>,
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
        /// `+batched` (API-39, implies `observe`): a host may observe every sample the device takes, sent in batches
        /// (`ObserveMode::Samples`), the property's observe slot is a `Batched<T>`. [Batching::SharedClock], the
        /// only form of properties: the samples of a level share one time column per update, the `sample_times` of
        /// its `PropertyUpdate` event. `None` for a property that isn't batched.
        #[default = None]
        batched: Option<Batching>,
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
        /// Items are sent in batches (API-41): `batched` in `#[stream(T, delivery, batched)]`, `#[batched]` next to
        /// `stream!`. Each event carries one `Series<Timed<T>>` (`Series<T>` when [Batching::Untimed]), its `units`
        /// are the batch's items. None: one `T` per event.
        #[default = None]
        batched: Option<Batching>,
        /// [Batching::Timed] only (API-41): the `Timed<T>` instance of the wire item (`Series` of it is the wire
        /// form), computed once when the model is built. `Timed` isn't otherwise referred to from `ty` alone, so a
        /// dynamic decoder (the `ww` CLI, Python) couldn't find its definition without this. `None` for
        /// [Batching::Untimed] (its wire item is `Series<T>`, already `ty`) and for a stream that isn't batched.
        #[default = None]
        batch_item_ty: Option<Type<'i>>,
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

/// How the items of a batched [ApiItemKind::StreamFn] (API-41) or the samples of a `+batched` property (API-39)
/// are sent.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Copy, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum Batching {
    /// `Series<Timed<T>>`: every item with the device time it was taken at.
    Timed,
    /// `Series<T>`, `batched(untimed)`: `T` has its own time, or needs none.
    Untimed,
    /// Properties only (`+batched`, API-39): `Series<T>` in the property's `Batched::Samples` slot, one item per
    /// sample of its level, their times once per update for the whole level, a `Series<Second>` in the level's
    /// `sample_times` slot.
    SharedClock,
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
    /// Property can only be read but can change on the server and be observed for changes (`+observe`; with
    /// `+batched` also every sample, see [ApiItemKind::Property::batched])
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
        len: Length,
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
        max_len: Length,
        ty: RefBox<'i, Type<'i>>,
    },
    /// A use of a generic struct or enum from the ApiBundle types array with its arguments, `Timed<Ampere>`
    /// (API-43): like [OutOfLine](Type::OutOfLine), the definition's [generics](ItemStruct::generics) replaced
    /// with `args`, one per parameter, in the same order and of the same kind.
    Instance {
        type_idx: UNib32,
        args: RefVec<'i, GenericArg<'i>>,
    },
    /// Type parameter of the generic struct or enum this type is in: index into its
    /// [generics](ItemStruct::generics), which must be a [type parameter](GenericParamKind::Type).
    Param(UNib32),
    /// List of `T` written column by column, one column per leaf field, `Series<'i, T>` / `SeriesOwned<T>` (SW-60).
    /// Same place in its parent as [Vec](Type::Vec) (`Unsized`), its own layout inside (`docs/serdes/series.md`).
    /// `T` is a struct, enum, tuple, array, `Option` or primitive of Series v1, fields marked `#[series(opaque)]`
    /// carry [Field::series_opaque].
    Series(RefBox<'i, Type<'i>>),
}

/// Length of an [array](Type::Array) or the bound of a [MaxLen](Type::MaxLen): a number, or a const parameter of the
/// generic struct or enum it is in (`[u8; N]`, API-43).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Copy, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = u1,
    no_future_version
)]
pub enum Length {
    Fixed(UNib32),
    /// Index into the [generics](ItemStruct::generics) of the enclosing definition, a
    /// [const parameter](GenericParamKind::Const) of an integer type.
    Param(UNib32),
}

/// Type or const parameter of a generic struct or enum (API-43), see [ItemStruct::generics].
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq)
)]
pub struct GenericParam<'i> {
    /// Name as written, `T`, `N`.
    pub ident: &'i str,
    pub kind: GenericParamKind<'i>,
}

#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq),
    ww_repr = u1,
    no_future_version
)]
pub enum GenericParamKind<'i> {
    /// `T`, used as [Type::Param].
    Type,
    /// `const N: usize`, used as [Length::Param] or passed on as [GenericArg::ConstParam]. `ty` is
    /// [Bool](Type::Bool) or an integer [NumericAny](Type::NumericAny) (`usize` is `u64`, `isize` is `i64`).
    Const { ty: Type<'i> },
}

/// Where a concrete definition comes from: generic definition `type_idx` with `args`, as in a [Type::Instance]
/// (see [ItemStruct::instance_of]).
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq)
)]
pub struct InstanceOf<'i> {
    pub type_idx: UNib32,
    pub args: RefVec<'i, GenericArg<'i>>,
}

/// Argument of a [Type::Instance], for the parameter at the same position.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq),
    ww_repr = u2,
    no_future_version
)]
pub enum GenericArg<'i> {
    /// For a type parameter.
    Type(Type<'i>),
    /// For a const parameter: a [Value::Bool] or [Value::Numeric] of its type.
    Const(Value<'i>),
    /// For a const parameter: const parameter `idx` of the enclosing generic definition, passed on
    /// (`struct Outer<const N: usize> { inner: Buf<N> }`).
    ConstParam(UNib32),
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
    /// Type and const parameters (API-43), empty for a non-generic struct. A generic definition is only used
    /// through a [Type::Instance], its fields refer to the parameters with [Type::Param], [Length::Param] and
    /// [GenericArg::ConstParam].
    pub generics: RefVec<'i, GenericParam<'i>>,
    pub fields: Fields<'i>,
    /// `#[derive_shrink_wrap(sparse, ..)]` (DER-13): every field is an `Option<T>`, written as one slot of an
    /// `OptionAnyVec` (its [Field::slot]) instead of the plain, sequential field encoding. A dynamic decoder
    /// (TOOL-11) must know this to read the struct's bytes correctly.
    #[default = false]
    pub sparse: bool,
    /// Modules from the crate root to where other crates name this definition (API-46), `None` at the crate root:
    /// `["units"]` for `ww_si::units::Volt`. The shortest path through `pub mod`s and `pub use` re-exports, so a type
    /// in a private module re-exported at the root has `None`. Its identity in its crate is the path and [Self::ident]
    /// (`units::Volt` and a root `Volt` are different definitions), see [ItemStructOwned::qualified_name].
    #[default = None]
    pub path: Option<RefVec<'i, &'i str>>,
    /// The generic definition and arguments this concrete definition was made from by
    /// [instantiate](crate::generics::instantiate) (`Quantity<units::Volt>` for the concrete `Quantity<Volt>`), so
    /// that a dynamic client can tell which generic type a value is of. Set only by `instantiate`, on the host: never
    /// in a bundle from a device or a file ([prepare](crate::generics::prepare) rejects one), and left out of
    /// [signatures](crate::signature).
    #[default = None]
    pub instance_of: Option<InstanceOf<'i>>,
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
    /// `#[series(opaque)]` (SW-60): inside a [Series](Type::Series), the field is an opaque run of its value's own
    /// bytes instead of its columns.
    #[default = false]
    pub series_opaque: bool,
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
    /// Type and const parameters, as for [ItemStruct::generics].
    pub generics: RefVec<'i, GenericParam<'i>>,
    pub variants: RefVec<'i, Variant<'i>>,
    /// Modules from the crate root to where other crates name this definition (API-46), `None` at the crate root:
    /// `["units"]` for `ww_si::units::Volt`. The shortest path through `pub mod`s and `pub use` re-exports, so a type
    /// in a private module re-exported at the root has `None`. Its identity in its crate is the path and [Self::ident]
    /// (`units::Volt` and a root `Volt` are different definitions), see [ItemEnumOwned::qualified_name].
    #[default = None]
    pub path: Option<RefVec<'i, &'i str>>,
    /// The generic definition and arguments this concrete definition was made from by
    /// [instantiate](crate::generics::instantiate) (`Quantity<units::Volt>` for the concrete `Quantity<Volt>`), so
    /// that a dynamic client can tell which generic type a value is of. Set only by `instantiate`, on the host: never
    /// in a bundle from a device or a file ([prepare](crate::generics::prepare) rejects one), and left out of
    /// [signatures](crate::signature).
    #[default = None]
    pub instance_of: Option<InstanceOf<'i>>,
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
