//! The borrowed form of every type as a consumer (TEST-37): `#[derive_shrink_wrap]` generates a borrowed type
//! (`Inner<'i>`, `RefVec`, `&str`) beside the owned one, with its own writer and reader, and firmware writes with
//! the borrowed one. Each case's golden bytes are read with the borrowed type and written again, which must give
//! the golden bytes back; the decode half reads what it wrote with the owned type of the derive consumer, which
//! must give the case's value.
//!
//! A shape case (TEST-37, [crate::shapes]) is also built from slices, as firmware builds a value: written, it must
//! give the same bytes, and the golden bytes must read as it.
//!
//! The borrowed type of a case is found by the case's `type`: the property of `Conformance` (the table below has
//! one line per property, and [missing_properties] fails when one is left out), or the Rust type of one outside the
//! model. A type without a borrowed form is not listed, so its cases need a gap mark, as for any consumer.

use crate::corpus::{Case, Direction};
use crate::derive::{Decoded, Derive};
use conformance_api::*;
// the corpus enum, not the handler-side `Error` of the prelude
use conformance_api::Error;
use core::ops::{Range, RangeInclusive};
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::series::Series;
use wire_weaver::shrink_wrap::{Error as SwError, UVlq32, UVlq64};

/// Read with the borrowed type (the outer error), then written with it (the inner error).
pub type RoundTrip = fn(&[u8]) -> Result<Result<Vec<u8>, SwError>, SwError>;

/// A shape case's borrowed value built from slices (TEST-37, [crate::shapes]): what writing it gives, and whether
/// reading bytes gives it.
pub struct SliceCase {
    pub write: fn(&mut [u8]) -> Result<Vec<u8>, SwError>,
    pub read: fn(&[u8]) -> Result<bool, SwError>,
}

/// `name => Type,` lines as a table of round trips through the borrowed `Type`.
macro_rules! round_trips {
    ($($name:expr => $ty:ty,)*) => {
        &[$(($name, |bytes| {
            let value = <$ty>::from_ww_bytes(bytes)?;
            let mut buf = vec![0u8; bytes.len() * 2 + 64];
            Ok(value.to_ww_bytes(&mut buf).map(|b| b.to_vec()))
        }),)*]
    };
}
pub(crate) use round_trips;

macro_rules! borrowed_types {
    ($($name:ident => $ty:ty,)*) => {
        /// The `type` names of the cases with a borrowed form and their round trips; the shape types are in
        /// [crate::shape_values::TYPES].
        const TABLE: &[(&str, RoundTrip)] = round_trips![$(stringify!($name) => $ty,)*];
    };
}

borrowed_types! {
        u8 => u8,
        u16 => u16,
        u32 => u32,
        u64 => u64,
        u128 => u128,
        i8 => i8,
        i16 => i16,
        i32 => i32,
        i64 => i64,
        i128 => i128,
        f32 => f32,
        f64 => f64,
        bool => bool,
        unib32 => UNib32,
        inib32 => INib32,
        uvlq32 => UVlq32,
        uvlq64 => UVlq64,
        nibble => Nibble,
        u4 => U4,
        u10 => U10,
        u20 => U20,
        i5 => I5,
        i12 => I12,
        string => &'_ str,
        bytes => RefVec<'_, u8>,
        vec_u16 => RefVec<'_, u16>,
        vec_string => RefVec<'_, &'_ str>,
        vec_bool => RefVec<'_, bool>,
        vec_option => RefVec<'_, Option<u8>>,
        vec_vec => RefVec<'_, RefVec<'_, u8>>,
        max_len_str => MaxLen<&'_ str, 4>,
        max_len_bytes => MaxLen<RefVec<'_, u8>, 3>,
        option_u8 => Option<u8>,
        option_str => Option<&'_ str>,
        option_option => Option<Option<u8>>,
        result => Result<u8, Error>,
        array_u16 => [u16; 3],
        array_bool => [bool; 3],
        tuple => (u8, u32, bool),
        range => Range<u16>,
        range_inclusive => RangeInclusive<i8>,
        bits => Bits,
        nested => Outer<'_>,
        flagged => Flagged<'_>,
        bytes_end => BytesEnd<'_>,
        bytes_mid => BytesMid<'_>,
        pair => Pair<'_>,
        rgb => Rgb,
        marker => Marker,
        sized => SizedPoint,
        final_struct => FinalHeader<'_>,
        generic => Timed<u16>,
        generic_const => Window<3>,
        sparse => Sparse<'_>,
        tree => Tree<'_>,
        phantom => Quantity<Marker>,
        validated => Ordered,
        expr => Expr<'_>,
        shape => Shape<'_>,
        shape_v2 => ShapeV2<'_>,
        address => I2cAddress,
        command => Command,
        error => Error,
        nib_enum => Level,
        cfg_subset => CfgSubset,
        config_v1 => ConfigV1<'_>,
        config_v2 => ConfigV2<'_>,
        fixed_numbers => FixedNumbers,
        fixed_bits => FixedBits,
        fixed_nested => FixedOuter,
        fixed_maybe => FixedMaybe,
        fixed_op => FixedOp,
        series_u16 => Series<'_, u16>,
        series_sample => Series<'_, Sample>,
        wired => Wired,
        wired_op => WiredOp,
        wired_signed => WiredSigned,
        series_wired => Series<'_, WiredSample>,
        gated_v1 => GatedV1,
        gated_v2 => GatedV2,
        count => Count,
        index => Index,
        signed_index => SignedIndex,
        wrapped_fields => WrappedFields<'_>,
        plain_fields => PlainFields<'_>,
        wrapped_sized => WrappedSized,
        plain_sized => PlainSized,
        vec_index => RefVec<'_, Index>,
        vec_unib32 => RefVec<'_, UNib32>,
        vec_signed_index => RefVec<'_, SignedIndex>,
        vec_inib32 => RefVec<'_, INib32>,
        vec_digit => RefVec<'_, Digit>,
        vec_nibble => RefVec<'_, Nibble>,
        option_count => Option<Count>,
        option_u64 => Option<u64>,
        array_digit => [Digit; 3],
        array_nibble => [Nibble; 3],
        series_count => Series<'_, Count>,
        series_u64 => Series<'_, u64>,
        series_wrapped => Series<'_, WrappedSample>,
        series_plain => Series<'_, PlainSample>,
        steer_point => ww_timebase::SteerPoint<'_>,
        sync_status => ww_timebase::SyncStatus<'_>,
        dim => ww_si::Dim<'_>,
        fixed_pow10 => ww_si::Fixed<ww_si::units::Second, ww_si::prefix::Pow10<-4>, u32>,
        duty16 => ww_si::Duty16,
        fixed_bounded => ww_si::Fixed<ww_si::units::Second, ww_si::prefix::Milli, u16, ww_si::Range<1, 5000>>,
        fixed_f32 => ww_si::Fixed<ww_si::units::Volt, ww_si::prefix::Milli, f32>,
        fixed_maybe_nan => ww_si::Fixed<ww_si::units::Volt, ww_si::prefix::Milli, f32, ww_si::MaybeNan>,
        fixed_finite => ww_si::Fixed<ww_si::units::Volt, ww_si::prefix::Milli, f32, ww_si::Finite>,
        fixed_customary => ww_si::Fixed<ww_si::units::Times<ww_si::units::PoundForce, ww_si::units::Second>, ww_si::prefix::Milli, i32>,
        coded_mu_law => ww_si::codec::Coded<ww_si::codec::MuLaw, ww_si::Fixed<ww_si::units::Ratio, ww_si::prefix::Pow2<-13>, i16>, u8>,
        coded_mu_law_single_zero => ww_si::codec::Coded<ww_si::codec::MuLawSingleZero, ww_si::Fixed<ww_si::units::Ratio, ww_si::prefix::Pow2<-13>, i16>, u8>,
        niched_mu_law => ww_si::exact_units::niche::Niched<ww_si::codec::Coded<ww_si::codec::MuLawSingleZero, ww_si::Fixed<ww_si::units::Ratio, ww_si::prefix::Pow2<-13>, i16>, u8>, ww_si::exact_units::niche::Null<0x7F>, u8>,
        coded_pq => ww_si::codec::Coded<ww_si::codec::Transfer<ww_si::codec::video::Narrow10, ww_si::codec::video::Pq>, ww_si::codec::video::NarrowSignal10, u16>,
        magnitude => ww_si::Magnitude,
        fraction => ww_si::exact_units::Fraction,
        run_scale => ww_si::RunScale,
        ticks => ww_si::Ticks<u64>,
        quantity_volt => ww_si::Volt,
        quantity_byte => ww_si::Quantity<ww_si::units::Byte>,
        quantity_ratio => ww_si::Quantity<ww_si::units::Ratio>,
        quantity_second => ww_si::Second,
        quantity_angle => ww_si::Quantity<ww_si::units::Radian>,
        quantity_rail => ww_si::Quantity<ww_si::units::Volt, ww_si::RangeDec<0, 33, -1>>,
        point_celsius => ww_si::Point<ww_si::origins::Celsius, ww_si::Fixed<ww_si::units::Kelvin, ww_si::prefix::Centi, i16>>,
        si_timed => ww_si::Timed<u16>,
        totals_item => wire_weaver::ww_api::Totals<ww_si::Fixed<ww_si::units::Item, ww_si::prefix::One, i32>>,
        key_event => ww_keyed::KeyEvent<UNib32, u16>,
        mount_presence => wire_weaver::ww_api::mount::MountPresence<'_>,
        mount_name => wire_weaver::ww_api::mount::MountName<'_>,
        si_timed_millis => ww_si::Timed<u16, ww_si::origins::DeviceMillis<u16>>,
        series_si_timed => Series<'_, ww_si::Timed<u16>>,
        series_si_timed_ampere => Series<'_, ww_si::Timed<ww_si::Ampere>>,
        point_bounded => ww_si::Point<ww_si::origins::Celsius, ww_si::Fixed<ww_si::units::Kelvin, ww_si::prefix::Deci, i16, ww_si::Range<-400, 1250>>>,
        offset => ww_si::Offset,
        with_index => WithIndex,
        index_from_1 => ww_si::IndexFrom1<ww_si::units::Item, u8, ww_si::Range<1, 4>>,
        seq_pair => SeqPair,
        si_expr => ww_si::SIExpr<'_>,
        any_quantity => ww_si::AnyQuantity<'_>,
        marker_def => ww_self::MarkerDef<'_>,
        item_struct_extra => ww_self::ItemStructExtra<'_>,
        signed => Signed,
        unsigned => Unsigned,
        // outside the API model
        Framed => Framed<'_>,
        WithTail => WithTail<'_>,
        DeviceInfo => ww_link::DeviceInfo<'_>,
        LinkSetup => ww_link::LinkSetup<'_>,
        DisconnectReason => wire_weaver::DisconnectReason,
        Request => ww_client_server::Request<'_>,
        RequestKind => ww_client_server::RequestKind<'_>,
        Event => ww_client_server::Event<'_>,
        MultiIndex => ww_client_server::MultiIndex<'_>,
        StreamOpened => ww_client_server::stream::StreamOpened<'_>,
        Error => ww_client_server::Error<'_>,
        args_one => ArgsOneArgs,
        args_plain => ArgsPlainArgs,
        args_flatten => ArgsPair,
        args_defaults => ArgsDefaultsArgs,
}

fn all_types() -> impl Iterator<Item = &'static (&'static str, RoundTrip)> {
    TABLE.iter().chain(crate::shape_values::TYPES)
}

fn round_trip(case: &Case) -> Option<RoundTrip> {
    all_types()
        .find(|(name, _)| *name == case.ty)
        .map(|(_, rt)| *rt)
}

fn slice_case(case: &Case) -> Option<&'static SliceCase> {
    crate::shape_values::BORROWED
        .iter()
        .find(|(name, _)| *name == case.name)
        .map(|(_, c)| c)
}

/// Properties of `Conformance` without a line in the table: each needs one, the borrowed form is never optional.
pub fn missing_properties() -> Vec<String> {
    crate::dynamic::bundle()
        .root
        .items
        .iter()
        .filter(|item| matches!(item.kind, ww_self::ApiItemKindOwned::Property { .. }))
        .map(|item| item.ident.to_string())
        .filter(|name| !all_types().any(|(n, _)| n == name))
        .collect()
}

#[derive(Default)]
pub struct Borrowed {
    derive: Derive,
}

impl crate::Consumer for Borrowed {
    const NAME: &'static str = "borrowed";

    fn encode(&self, case: &Case) -> Option<Result<Vec<u8>, String>> {
        // it writes what it read: a value that must not encode has no bytes to read
        if case.direction == Direction::RejectValue {
            return None;
        }
        let rt = round_trip(case)?;
        let read_and_written = match rt(&case.golden()) {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(e)) => return Some(Err(format!("write: {e:?}"))),
            Err(e) => return Some(Err(format!("read: {e:?}"))),
        };
        // a shape case is also built from slices: both must write the same bytes
        if let Some(slices) = slice_case(case) {
            let mut buf = [0u8; 256];
            match (slices.write)(&mut buf) {
                Ok(bytes) if bytes == read_and_written => {}
                Ok(bytes) => {
                    return Some(Err(format!(
                        "built from slices: {}",
                        crate::corpus::bytes_mismatch(&bytes, &case.golden())
                    )));
                }
                Err(e) => return Some(Err(format!("built from slices, write: {e:?}"))),
            }
        }
        Some(Ok(read_and_written))
    }

    fn decode(&self, case: &Case) -> Option<crate::DecodeResult> {
        let rt = round_trip(case)?;
        let typed = self.derive.get(&case.name)?;
        if let Some(slices) = slice_case(case) {
            match (slices.read)(&case.golden()) {
                Ok(true) => {}
                Ok(false) => return Some(Ok(Err("read: not the value built from slices".into()))),
                Err(e) => return Some(Err(format!("{e:?}"))),
            }
        }
        Some(match rt(&case.golden()) {
            Err(e) => Err(format!("{e:?}")),
            Ok(Err(e)) => Ok(Err(format!("written again: {e:?}"))),
            Ok(Ok(bytes)) => match (typed.decode)(&bytes) {
                Decoded::Equal => Ok(Ok(())),
                Decoded::Differs(e) => Ok(Err(format!(
                    "written again, then read with the owned type: {e}"
                ))),
                Decoded::Failed(e) => Ok(Err(format!(
                    "written again as {}, which the owned type can't read: {e}",
                    hex::encode(&bytes)
                ))),
            },
        })
    }
}
