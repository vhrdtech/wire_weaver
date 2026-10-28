//! Types of the wire-format conformance matrix (TEST-23, `docs/serdes/conformance.md`).
//!
//! Every case of `tests/conformance/corpus.json` names a type by the property of `Conformance` that has it
//! (`"type": "u8"` is `property!(rw u8: u8)`), so each consumer finds it: the Rust derive through the case table of
//! `tests/conformance`, the dynamic client and Python through this trait's API model, loaded from source. Types
//! without a sw serdes (`head_tail`) or not in the API model yet are defined here but not listed in the trait; the
//! corpus marks them for the consumers that can't read them.

use core::ops::{Range, RangeInclusive};
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::head_tail::List;
use wire_weaver::shrink_wrap::series::Series;
use wire_weaver::shrink_wrap::tail_bytes::{TailBytes, TailBytesOwned};

#[ww_trait]
trait Conformance {
    // numbers
    property!(rw u8: u8);
    property!(rw u16: u16);
    property!(rw u32: u32);
    property!(rw u64: u64);
    property!(rw u128: u128);
    property!(rw i8: i8);
    property!(rw i16: i16);
    property!(rw i32: i32);
    property!(rw i64: i64);
    property!(rw i128: i128);
    property!(rw f32: f32);
    property!(rw f64: f64);
    property!(rw bool: bool);
    property!(rw unib32: UNib32);
    property!(rw uvlq32: UVlq32);
    property!(rw uvlq64: UVlq64);
    property!(rw nibble: Nibble);
    property!(rw u4: U4);
    property!(rw u10: U10);
    property!(rw u20: U20);
    property!(rw i5: I5);
    property!(rw i12: I12);
    // lists and strings
    property!(rw string: &'i str);
    property!(rw bytes: RefVec<'i, u8>);
    property!(rw vec_u16: RefVec<'i, u16>);
    property!(rw vec_string: RefVec<'i, &'i str>);
    property!(rw vec_bool: RefVec<'i, bool>);
    property!(rw vec_option: RefVec<'i, Option<u8>>);
    property!(rw vec_vec: RefVec<'i, RefVec<'i, u8>>);
    property!(rw max_len_str: MaxLen<&'i str, 4>);
    property!(rw max_len_bytes: MaxLen<RefVec<'i, u8>, 3>);
    // composites
    property!(rw option_u8: Option<u8>);
    property!(rw option_str: Option<&'i str>);
    property!(rw option_option: Option<Option<u8>>);
    property!(rw result: Result<u8, Error>);
    property!(rw array_u16: [u16; 3]);
    property!(rw array_bool: [bool; 3]);
    property!(rw tuple: (u8, u32, bool));
    property!(rw range: Range<u16>);
    property!(rw range_inclusive: RangeInclusive<i8>);
    // structs
    property!(rw bits: Bits);
    property!(rw nested: Outer<'i>);
    property!(rw flagged: Flagged<'i>);
    property!(rw pair: Pair<'i>);
    property!(rw rgb: Rgb);
    property!(rw marker: Marker);
    property!(rw sized: SizedPoint);
    property!(rw final_struct: FinalHeader<'i>);
    property!(rw generic: Timed<u16>);
    property!(rw generic_const: Window<3>);
    property!(rw sparse: Sparse<'i>);
    property!(rw tree: Tree<'i>);
    property!(rw phantom: Quantity<Marker>);
    property!(rw validated: Ordered);
    property!(rw expr: Expr<'i>);
    // enums
    property!(rw shape: Shape<'i>);
    property!(rw shape_v2: ShapeV2<'i>);
    property!(rw address: I2cAddress);
    property!(rw command: Command);
    property!(rw error: Error);
    property!(rw nib_enum: Level);
    property!(rw cfg_subset: CfgSubset);
    // evolution
    property!(rw config_v1: ConfigV1<'i>);
    property!(rw config_v2: ConfigV2<'i>);
    // fixed layout
    property!(rw fixed_numbers: FixedNumbers);
    property!(rw fixed_bits: FixedBits);
    property!(rw fixed_nested: FixedOuter);
    property!(rw fixed_maybe: FixedMaybe);
    property!(rw fixed_op: FixedOp);
    // Series
    property!(rw series_u16: Series<'i, u16>);
    property!(rw series_sample: Series<'i, Sample>);
    // plain integers written as another wire type (DER-19)
    property!(rw wired: Wired);
    property!(rw wired_op: WiredOp);
    property!(rw series_wired: Series<'i, WiredSample>);
    // the derive's field attributes behind #[cfg_attr(pred, ..)] (API-60)
    property!(rw gated_v1: GatedV1);
    property!(rw gated_v2: GatedV2);
    // transparent (DER-18), each next to its plain twin: the same bytes
    property!(rw count: Count);
    property!(rw index: Index);
    property!(rw wrapped_fields: WrappedFields<'i>);
    property!(rw plain_fields: PlainFields<'i>);
    property!(rw wrapped_sized: WrappedSized);
    property!(rw plain_sized: PlainSized);
    property!(rw vec_index: RefVec<'i, Index>);
    property!(rw vec_unib32: RefVec<'i, UNib32>);
    property!(rw vec_digit: RefVec<'i, Digit>);
    property!(rw vec_nibble: RefVec<'i, Nibble>);
    property!(rw option_count: Option<Count>);
    property!(rw option_u64: Option<u64>);
    property!(rw array_digit: [Digit; 3]);
    property!(rw array_nibble: [Nibble; 3]);
    property!(rw series_count: Series<'i, Count>);
    property!(rw series_u64: Series<'i, u64>);
    property!(rw series_wrapped: Series<'i, WrappedSample>);
    property!(rw series_plain: Series<'i, PlainSample>);
    // ww_stdlib types whose evolution is checked here
    property!(rw steer_point: ww_timebase::SteerPoint<'i>);
    property!(rw sync_status: ww_timebase::SyncStatus<'i>);
    property!(rw dim: ww_si::Dim<'i>);
    // ww_si::Fixed (STD-55), each next to its plain number: the same bytes
    property!(rw fixed_pow10: ww_si::Fixed<ww_si::units::Second, ww_si::prefix::Pow10<-4>, u32>);
    property!(rw duty16: ww_si::Duty16);
    property!(rw fixed_bounded: ww_si::Fixed<ww_si::units::Second, ww_si::prefix::Milli, u16, ww_si::Range<1, 5000>>);
    // an imperial unit (STD-72): the same i32, lbf·s only in the type
    property!(rw fixed_customary: ww_si::Fixed<ww_si::units::Times<ww_si::units::PoundForce, ww_si::units::Second>, ww_si::prefix::Milli, i32>);
    property!(rw magnitude: ww_si::Magnitude);
    // ww_si::Quantity (STD-59): a prefix and a number; binary and Exact prefixes, bounds in the type
    property!(rw quantity_volt: ww_si::Volt);
    property!(rw quantity_byte: ww_si::Quantity<ww_si::units::Byte>);
    property!(rw quantity_ratio: ww_si::Quantity<ww_si::units::Ratio>);
    property!(rw quantity_second: ww_si::Second);
    property!(rw quantity_angle: ww_si::Quantity<ww_si::units::Radian>);
    property!(rw quantity_rail: ww_si::Quantity<ww_si::units::Volt, ww_si::RangeDec<0, 33, -1>>);
    // ww_si::Point (STD-64): the number alone, the origin is in the type
    property!(rw point_celsius: ww_si::Point<ww_si::origins::Celsius, ww_si::Fixed<ww_si::units::Kelvin, ww_si::prefix::Centi, i16>>);
    property!(rw point_bounded: ww_si::Point<ww_si::origins::Celsius, ww_si::Fixed<ww_si::units::Kelvin, ww_si::prefix::Deci, i16, ww_si::Range<-400, 1250>>>);
    property!(rw offset: ww_si::Offset);
    // ww_si::Index, IndexFrom1, Seq (STD-58): the number alone, the table it counts in is in the type
    property!(rw with_index: WithIndex);
    property!(rw index_from_1: ww_si::IndexFrom1<ww_si::units::Item, u8, ww_si::Range<1, 4>>);
    property!(rw seq_pair: SeqPair);
    property!(rw si_expr: ww_si::SIExpr<'i>);
    // the API model's own definitions of units and scales (INT-13), as a device sends them in its introspection data
    property!(rw marker_def: ww_self::MarkerDef<'i>);
    property!(rw item_struct_extra: ww_self::ItemStructExtra<'i>);
}

// ---------------------------------------------------------------------------------------------------------------
// structs

/// Bit-sized fields and nibbles mixed with byte fields: a byte field starts a fresh byte, a nibble a 4-bit boundary.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy))]
pub struct Bits {
    pub on: bool,
    pub small: U4,
    pub signed: I5,
    pub nib: Nibble,
    pub byte: u8,
    pub wide: U10,
    pub last: bool,
}

#[derive_shrink_wrap(owned, derive(Debug, PartialEq, Clone))]
pub struct Inner<'i> {
    pub id: u8,
    pub note: &'i str,
}

/// An evolvable struct inside another: written with its size.
#[derive_shrink_wrap(owned, derive(Debug, PartialEq, Clone))]
pub struct Outer<'i> {
    pub tag: u16,
    pub inner: Inner<'i>,
    pub items: RefVec<'i, Inner<'i>>,
    pub after: u8,
}

/// Relocated `Option` and `Result` flags (`#[flag]`), also in an enum variant.
#[derive_shrink_wrap(owned, derive(Debug, PartialEq, Clone))]
pub struct Flagged<'i> {
    pub a: U3,
    #[flag]
    late: bool,
    #[flag]
    res: bool,
    pub early: Option<&'i str>,
    pub res: Result<u8, Error>,
    pub tagged: Tagged,
    pub late: Option<u16>,
}

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy), ww_repr = u1)]
pub enum Tagged {
    Plain,
    Pair {
        #[flag]
        second: bool,
        first: Option<u8>,
        second: Option<u8>,
    },
}

/// A tuple struct.
#[derive_shrink_wrap(owned, derive(Debug, PartialEq, Clone))]
pub struct Pair<'i>(pub u8, pub Option<&'i str>);

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy))]
pub struct Rgb(pub u8, pub u8, pub u8);

/// A unit struct.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy))]
pub struct Marker;

/// `sized`: no size of its own inside a parent, bits packed with the parent's.
#[derive_shrink_wrap(borrowed, owned, sized, derive(Debug, PartialEq, Clone, Copy))]
pub struct SizedPoint {
    pub x: i16,
    pub y: i16,
    pub visible: bool,
}

/// `final_structure`: no size of its own, may hold variable-length fields.
#[derive_shrink_wrap(owned, final_structure, derive(Debug, PartialEq, Clone))]
pub struct FinalHeader<'i> {
    pub version: u8,
    pub name: &'i str,
    pub crc: u16,
}

/// A generic struct (DER-14, API-43).
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy))]
pub struct Timed<T> {
    pub t: u32,
    pub value: T,
}

/// A const generic.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy))]
pub struct Window<const N: usize> {
    pub samples: [u8; N],
}

/// `sparse`: an `OptionAnyVec` of its fields, a gap in the slots.
#[derive_shrink_wrap(sparse, owned, derive(Debug, PartialEq, Clone, Default))]
pub struct Sparse<'i> {
    pub speed: Option<u16>,
    pub name: Option<&'i str>,
    #[slot = 5]
    pub samples: Option<RefVec<'i, i16>>,
    pub on: Option<bool>,
}

/// A `PhantomData` field takes no bits (DER-15).
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy))]
pub struct Quantity<U> {
    pub value: i32,
    pub _unit: core::marker::PhantomData<U>,
}

/// `validate`: a value just read is checked (SW-58).
#[derive_shrink_wrap(borrowed, owned, validate = Ordered::check, derive(Debug, PartialEq, Clone, Copy))]
pub struct Ordered {
    pub lo: u8,
    pub hi: u8,
}

impl Ordered {
    fn check(&self) -> Result<(), ShrinkWrapError> {
        if self.lo <= self.hi {
            Ok(())
        } else {
            Err(ShrinkWrapError::SubtypeOutOfRange)
        }
    }
}

/// `TailSize` (SW-18): the size of the rest of the value, filled in when written.
#[derive_shrink_wrap(owned, derive(Debug, PartialEq, Clone))]
pub struct Framed<'i> {
    pub magic: u16,
    pub size: TailSize<2>,
    pub name: &'i str,
    pub count: u32,
}

/// Self-reference through a list.
#[derive_shrink_wrap(owned, derive(Debug, PartialEq, Clone))]
pub struct Tree<'i> {
    pub label: &'i str,
    pub children: RefVec<'i, Tree<'i>>,
}

/// Self-reference through `RefBox`.
#[derive_shrink_wrap(owned, derive(Debug, PartialEq, Clone), ww_repr = u4)]
pub enum Expr<'i> {
    Add(RefBox<'i, Expr<'i>>, RefBox<'i, Expr<'i>>),
    Neg(RefBox<'i, Expr<'i>>),
    Lit(i32),
}

// ---------------------------------------------------------------------------------------------------------------
// enums

/// Unit, tuple and struct variants, an explicit discriminant, a 4-bit `ww_repr`, `Unknown` added by default.
#[derive_shrink_wrap(owned, derive(Debug, PartialEq, Clone), ww_repr = u4)]
pub enum Shape<'i> {
    Empty,
    Circle(u16),
    Rect { w: u8, h: u8, label: &'i str },
    Far = 9,
}

/// [Shape] with a variant appended: its bytes are an unknown variant to a reader of [Shape].
#[derive_shrink_wrap(owned, derive(Debug, PartialEq, Clone), ww_repr = u4)]
pub enum ShapeV2<'i> {
    Empty,
    Circle(u16),
    Rect { w: u8, h: u8, label: &'i str },
    Far = 9,
    Polygon(RefVec<'i, u8>) = 10,
}

/// 7-bit or 10-bit I2C address in 8 or 11 bits: a sub-byte discriminant, `self_describing`.
#[derive_shrink_wrap(
    borrowed,
    owned,
    derive(Debug, PartialEq, Clone, Copy),
    ww_repr = u1,
    self_describing
)]
pub enum I2cAddress {
    Seven(U7),
    Ten(U10),
}

/// `sized` enum, equal variants.
#[derive_shrink_wrap(
    borrowed,
    owned,
    derive(Debug, PartialEq, Clone, Copy),
    ww_repr = u8,
    sized
)]
pub enum Command {
    Stop(u16),
    Go(u16) = 20,
    Hold(u16) = 200,
}

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy), ww_repr = u2)]
pub enum Error {
    TooBig,
    Missing,
}

/// A nibble discriminant.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy), ww_repr = nib)]
pub enum Level {
    Low,
    High(u8) = 12,
}

/// A variant compiled out keeps its discriminant (DER-8): its bytes read as `EnumVariantDisabled`.
#[derive_shrink_wrap(
    borrowed,
    owned,
    derive(Debug, PartialEq, Clone, Copy),
    self_describing,
    ww_repr = unib32
)]
pub enum CfgSubset {
    A(u8),
    #[cfg(any())]
    B(u16),
    C,
}

// ---------------------------------------------------------------------------------------------------------------
// evolution

#[derive_shrink_wrap(owned, derive(Debug, PartialEq, Clone))]
pub struct ConfigV1<'i> {
    pub rate: u16,
    pub name: &'i str,
}

/// [ConfigV1] with fields appended: reads old data with the defaults.
#[derive_shrink_wrap(owned, derive(Debug, PartialEq, Clone))]
pub struct ConfigV2<'i> {
    pub rate: u16,
    pub name: &'i str,
    #[default = 7]
    pub timeout: u16,
    #[default = None]
    pub retries: Option<u8>,
    #[default = None]
    pub tags: Option<RefVec<'i, u8>>,
}

// ---------------------------------------------------------------------------------------------------------------
// fixed layout (SW-44, SW-45): same bytes as the sw serdes, plus offsets and a C reader

#[derive_shrink_wrap(borrowed, owned, fixed_layout, derive(Debug, PartialEq, Clone, Copy))]
pub struct FixedNumbers {
    pub a: u8,
    pub b: u16,
    pub c: u32,
    pub d: u64,
    pub e: i8,
    pub f: i16,
    pub g: i32,
    pub h: i64,
    pub x: f32,
    pub y: f64,
    pub words: [u16; 2],
}

#[derive_shrink_wrap(borrowed, owned, fixed_layout, derive(Debug, PartialEq, Clone, Copy))]
pub struct FixedBits {
    pub a: bool,
    pub n: Nibble,
    pub b: U3,
    pub s: I5,
    pub x: u16,
    pub wide: U20,
    pub i: I12,
}

#[derive_shrink_wrap(borrowed, owned, fixed_layout, derive(Debug, PartialEq, Clone, Copy))]
pub struct FixedInner {
    pub id: u32,
    pub hash: [u8; 4],
}

#[derive_shrink_wrap(borrowed, owned, fixed_layout, derive(Debug, PartialEq, Clone, Copy))]
pub struct FixedOuter {
    pub tag: u8,
    pub inner: FixedInner,
    pub signed: [i16; 2],
}

/// A variable last field: an `Option` with its flag as a bit field.
#[derive_shrink_wrap(borrowed, owned, fixed_layout, derive(Debug, PartialEq, Clone, Copy))]
pub struct FixedMaybe {
    pub id: u16,
    pub on: bool,
    pub extra: Option<u32>,
}

#[derive_shrink_wrap(
    borrowed,
    owned,
    self_describing,
    fixed_layout,
    ww_repr = u8,
    derive(Debug, PartialEq, Clone, Copy)
)]
pub enum FixedOp {
    Stop = 1,
    Erase { block: u16 } = 4,
    Copy { dst: u16, src: u16 },
}

// ---------------------------------------------------------------------------------------------------------------
// Series (SW-60)

#[derive_shrink_wrap(borrowed, owned, series, derive(Debug, PartialEq, Clone, Copy))]
pub struct Sample {
    pub t: u32,
    pub value: i16,
    pub ok: bool,
}

// ---------------------------------------------------------------------------------------------------------------
// #[wire(W)] (DER-19): plain Rust integers, written as W; the API model shows W

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy))]
pub struct Wired {
    #[wire(I4)]
    pub small: i8,
    #[wire(UNib32)]
    pub count: u64,
    #[wire(U12)]
    pub raw: u16,
    pub last: bool,
}

#[derive_shrink_wrap(borrowed, owned, self_describing, ww_repr = u4, derive(Debug, PartialEq, Clone, Copy))]
pub enum WiredOp {
    Stop,
    Step(#[wire(I4)] i8),
    Set {
        #[wire(UNib32)]
        value: u32,
    },
}

#[derive_shrink_wrap(borrowed, owned, series, derive(Debug, PartialEq, Clone, Copy))]
pub struct WiredSample {
    pub t: u32,
    #[wire(I12)]
    pub value: i16,
}

// ---------------------------------------------------------------------------------------------------------------
// #[cfg_attr(pred, wire(W))] and #[cfg_attr(pred, default = ..)] (API-60): read as if pred holds, the same bytes and
// API model as the plain attributes; a type crate with shrink_wrap optional writes them this way

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy))]
pub struct GatedV1 {
    #[wire(I4)]
    pub small: i8,
    pub last: bool,
}

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy))]
pub struct GatedV2 {
    #[cfg_attr(all(), wire(I4))]
    pub small: i8,
    pub last: bool,
    #[cfg_attr(all(), default = 3)]
    pub extra: u8,
}

// ---------------------------------------------------------------------------------------------------------------
// transparent (DER-18): a struct that is its one field on the wire, each case has the bytes of its plain twin

/// A wrapped `u64`, a tuple struct.
#[derive_shrink_wrap(
    borrowed,
    owned,
    transparent,
    series,
    derive(Debug, PartialEq, Clone, Copy)
)]
pub struct Count(pub u64);

/// A wrapped `UNib32`: self-describing, nibble-aligned.
#[derive_shrink_wrap(borrowed, owned, transparent, derive(Debug, PartialEq, Clone, Copy))]
pub struct Index(pub UNib32);

/// A wrapped `Nibble`: 4 bits, packed with its neighbours.
#[derive_shrink_wrap(
    borrowed,
    owned,
    transparent,
    derive(Debug, PartialEq, Clone, Copy, Default)
)]
pub struct Digit(pub Nibble);

/// A wrapped `f32` with named fields, generic over a unit that takes no bits.
#[derive_shrink_wrap(
    borrowed,
    owned,
    transparent,
    series,
    derive(Debug, PartialEq, Clone, Copy)
)]
pub struct Volts<U> {
    pub value: f32,
    pub _unit: core::marker::PhantomData<U>,
}

/// A wrapped string, borrowed.
#[derive_shrink_wrap(borrowed, owned, transparent, derive(Debug, PartialEq, Clone))]
pub struct Label<'i>(pub &'i str);

/// An `Unsized` parent of the wrappers.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone))]
pub struct WrappedFields<'i> {
    pub count: Count,
    pub digit: Digit,
    pub index: Index,
    pub volts: Volts<Marker>,
    pub label: Label<'i>,
    pub end: u8,
}

/// `WrappedFields` with the plain fields.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone))]
pub struct PlainFields<'i> {
    pub count: u64,
    pub digit: Nibble,
    pub index: UNib32,
    pub volts: f32,
    pub label: &'i str,
    pub end: u8,
}

/// A `sized` parent of the wrappers: their bits packed with the parent's.
#[derive_shrink_wrap(borrowed, owned, sized, derive(Debug, PartialEq, Clone, Copy))]
pub struct WrappedSized {
    pub on: bool,
    pub digit: Digit,
    pub count: Count,
    pub volts: Volts<Marker>,
}

/// An index into a table of samples next to a byte (STD-58): its `u32` alone.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy))]
pub struct WithIndex {
    pub slot: ww_si::Index<ww_si::units::Sample, u32>,
    pub tag: u8,
}

/// Two nibble sequence numbers, the second the one after the first (STD-58): a nibble each.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy))]
pub struct SeqPair {
    pub last: ww_si::Seq<ww_si::units::Frame, Nibble>,
    pub next: ww_si::Seq<ww_si::units::Frame, Nibble>,
}

/// `WrappedSized` with the plain fields.
#[derive_shrink_wrap(borrowed, owned, sized, derive(Debug, PartialEq, Clone, Copy))]
pub struct PlainSized {
    pub on: bool,
    pub digit: Nibble,
    pub count: u64,
    pub volts: f32,
}

/// A `Series` item with wrapped fields: the columns of the plain ones.
#[derive_shrink_wrap(borrowed, owned, series, derive(Debug, PartialEq, Clone, Copy))]
pub struct WrappedSample {
    pub t: Count,
    pub volts: Volts<Marker>,
}

/// `WrappedSample` with the plain fields.
#[derive_shrink_wrap(borrowed, owned, series, derive(Debug, PartialEq, Clone, Copy))]
pub struct PlainSample {
    pub t: u64,
    pub volts: f32,
}

// ---------------------------------------------------------------------------------------------------------------
// not in the API model: their own format or types the model has no form for

/// `head_tail` (SW-46): a wire format of its own, no sw serdes, so it can't be in an API.
#[derive_shrink_wrap(head_tail, derive(Clone, Copy, Debug, PartialEq))]
pub struct HtRecord<'i> {
    pub ch: u8,
    pub maybe: Option<u16>,
    pub list: List<'i, u16>,
    pub name: &'i str,
    pub on: bool,
}

/// `TailBytes` (SW-9) as the last field.
#[derive_shrink_wrap(owned, derive(Debug))]
pub struct WithTail<'i> {
    pub kind: u8,
    pub rest: TailBytes<'i>,
}
