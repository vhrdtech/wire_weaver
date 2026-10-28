//! The Rust derive's side of every case: the value as the Rust type `#[derive_shrink_wrap]` generates (owned),
//! encoded with `SerializeShrinkWrapOwned` and compared with the golden bytes decoded with
//! `DeserializeShrinkWrapOwned`; where the borrowed type differs, the golden bytes also go through it and back.
//!
//! A case here has the name of its corpus case; the corpus test checks both lists have the same names.

use conformance_api::*;
use core::fmt::Debug;
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::series::{Series, SeriesOwned};
use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
use wire_weaver::shrink_wrap::{
    ArrayString, ArrayVec, DeserializeShrinkWrapOwned, Error as SwError, SerializeShrinkWrapOwned,
    UVlq32, UVlq64,
};

type Enc = Box<dyn Fn() -> Result<Vec<u8>, String>>;
type Dec = Box<dyn Fn(&[u8]) -> Decoded>;

/// What decoding the golden bytes gave.
pub enum Decoded {
    /// The case's value.
    Equal,
    /// Another value.
    Differs(String),
    /// An error.
    Failed(String),
}

fn compare<T: Debug>(
    got: Result<T, SwError>,
    eq: impl Fn(&T) -> bool,
    want: impl Debug,
) -> Decoded {
    match got {
        Ok(got) if eq(&got) => Decoded::Equal,
        Ok(got) => Decoded::Differs(format!("decoded {got:?}, expected {want:?}")),
        Err(e) => Decoded::Failed(format!("{e:?}")),
    }
}
type RoundTrip = fn(&[u8]) -> Result<Vec<u8>, SwError>;

pub struct Typed {
    pub name: &'static str,
    /// The value's bytes, or why it can't be encoded.
    pub encode: Enc,
    /// The golden bytes decoded and compared with the value.
    pub decode: Dec,
    /// The bytes decoded with the borrowed type and encoded again.
    pub borrowed: Option<RoundTrip>,
}

fn case<T>(name: &'static str, value: T) -> Typed
where
    T: SerializeShrinkWrapOwned + DeserializeShrinkWrapOwned + PartialEq + Debug + Clone + 'static,
{
    let v = value.clone();
    Typed {
        name,
        encode: Box::new(move || v.to_ww_bytes_owned().map_err(|e| format!("{e:?}"))),
        decode: Box::new(move |bytes| {
            compare(T::from_ww_bytes_owned(bytes), |got| *got == value, &value)
        }),
        borrowed: None,
    }
}

/// A type without `Clone` / `PartialEq` (`TailBytesOwned`): compared by its `Debug` text.
fn case_debug<T>(name: &'static str, value: fn() -> T) -> Typed
where
    T: SerializeShrinkWrapOwned + DeserializeShrinkWrapOwned + Debug + 'static,
{
    Typed {
        name,
        encode: Box::new(move || value().to_ww_bytes_owned().map_err(|e| format!("{e:?}"))),
        decode: Box::new(move |bytes| {
            let want = value();
            compare(
                T::from_ww_bytes_owned(bytes),
                |got| format!("{got:?}") == format!("{want:?}"),
                &want,
            )
        }),
        borrowed: None,
    }
}

/// A borrowed-only type without a lifetime (`wire_weaver::DisconnectReason`): its borrowed traits, both ways.
fn case_borrowed_only<T>(name: &'static str, value: T) -> Typed
where
    T: SerializeShrinkWrap
        + for<'i> DeserializeShrinkWrap<'i>
        + PartialEq
        + Debug
        + Copy
        + Send
        + Sync
        + 'static,
{
    Typed {
        name,
        encode: Box::new(move || {
            let mut buf = [0u8; 64];
            value
                .to_ww_bytes(&mut buf)
                .map(|b| b.to_vec())
                .map_err(|e| format!("{e:?}"))
        }),
        decode: Box::new(move |bytes| compare(T::from_ww_bytes(bytes), |got| *got == value, value)),
        borrowed: None,
    }
}

/// A `head_tail` type (SW-46): `to_head_tail` / `from_head_tail`, its own format.
fn case_head_tail(name: &'static str, value: HtRecord<'static>) -> Typed {
    Typed {
        name,
        encode: Box::new(move || {
            let mut buf = [0u8; 256];
            let n = value.to_head_tail(&mut buf).map_err(|e| format!("{e:?}"))?;
            Ok(buf[..n].to_vec())
        }),
        decode: Box::new(move |bytes| {
            compare(HtRecord::from_head_tail(bytes), |got| *got == value, value)
        }),
        borrowed: None,
    }
}

/// Decode with the borrowed type `$b`, encode again.
macro_rules! borrowed {
    ($b:ty) => {
        |bytes: &[u8]| -> Result<Vec<u8>, SwError> {
            let v = <$b>::from_ww_bytes(bytes)?;
            let mut buf = vec![0u8; bytes.len() * 2 + 64];
            Ok(v.to_ww_bytes(&mut buf)?.to_vec())
        }
    };
}

impl Typed {
    fn borrowed(mut self, f: RoundTrip) -> Self {
        self.borrowed = Some(f);
        self
    }
}

fn s(v: &str) -> String {
    v.to_string()
}

fn inner(id: u8, note: &str) -> InnerOwned {
    InnerOwned { id, note: s(note) }
}

fn shape_rect() -> ShapeOwned {
    ShapeOwned::Rect {
        w: 1,
        h: 2,
        label: s("rect"),
    }
}

fn config_v1() -> ConfigV1Owned {
    ConfigV1Owned {
        rate: 1000,
        name: s("abc"),
    }
}

fn config_v2() -> ConfigV2Owned {
    ConfigV2Owned {
        rate: 1000,
        name: s("abc"),
        timeout: 20,
        retries: Some(9),
        tags: Some(vec![1, 2]),
    }
}

fn tree() -> TreeOwned {
    let leaf = |label: &str| TreeOwned {
        label: s(label),
        children: vec![],
    };
    TreeOwned {
        label: s("root"),
        children: vec![
            leaf("a"),
            TreeOwned {
                label: s("b"),
                children: vec![leaf("c")],
            },
        ],
    }
}

fn nib(v: u8) -> Nibble {
    Nibble::new(v).unwrap()
}

fn volts(value: f32) -> Volts<Marker> {
    Volts {
        value,
        _unit: core::marker::PhantomData,
    }
}

fn u3(v: u8) -> U3 {
    U3::new(v).unwrap()
}

pub fn table() -> Vec<Typed> {
    vec![
        // numbers
        case("int.u8.zero", 0u8),
        case("int.u8.max", u8::MAX),
        case("int.u16.le", 0xBEEFu16),
        case("int.u32.le", 0xDEAD_BEEFu32),
        case("int.u64.max", u64::MAX),
        case("int.u128.max", u128::MAX),
        case("int.i8.min", i8::MIN),
        case("int.i16.negative", -300i16),
        case("int.i32.negative", -70_000i32),
        case("int.i64.min", i64::MIN),
        case("int.i128.min", i128::MIN),
        case("float.f32", 1.5f32),
        case("float.f64", -2.25f64),
        case("bool.true", true),
        case("bool.false", false),
        case("unib32.one_nibble", UNib32(7)),
        case("unib32.two_nibbles", UNib32(8)),
        case("unib32.max", UNib32(u32::MAX)),
        case("uvlq32.two_bytes", UVlq32(128)),
        case("uvlq64.max", UVlq64(u64::MAX)),
        case("bits.nibble", Nibble::new(0xA).unwrap()),
        case("bits.u4", U4::new(9).unwrap()),
        case("bits.u10", U10::new(0x2AB).unwrap()),
        case("bits.u20", U20::new(0xF_EDCB).unwrap()),
        case("bits.i5", I5::new(-7).unwrap()),
        case("bits.i12", I12::new(-2).unwrap()),
        // lists and strings
        case("str.empty", s("")),
        case("str.ascii", s("hello")),
        case("str.utf8", s("größe")),
        case(
            "str.array_string",
            ArrayString::<8>::try_from("hello").unwrap(),
        ),
        case("bytes.empty", Vec::<u8>::new()),
        case("bytes.three", vec![1u8, 2, 3]),
        case("vec.u16", vec![1u16, 0xFFFF]),
        case("vec.array_vec", {
            let mut v = ArrayVec::<u16, 4>::default();
            v.push(1).unwrap();
            v.push(0xFFFF).unwrap();
            v
        }),
        case("vec.string", vec![s("a"), s("bc")]),
        case("vec.bool", vec![true, false, true]),
        case("vec.option", vec![Some(1u8), None, Some(3)]),
        case("vec.vec", vec![vec![1u8], vec![], vec![2, 3]]),
        case(
            "max_len.str.at_bound",
            MaxLen::<String, 4>::new_unchecked(s("abcd")),
        ),
        case(
            "max_len.str.over_bound",
            MaxLen::<String, 4>::new_unchecked(s("abcde")),
        ),
        case(
            "max_len.str.over_bound_bytes",
            MaxLen::<String, 4>::new_unchecked(s("abcde")),
        ),
        case(
            "max_len.bytes",
            MaxLen::<Vec<u8>, 3>::new_unchecked(vec![7, 8, 9]),
        ),
        // composites
        case("option.none", None::<u8>),
        case("option.some", Some(5u8)),
        case("option.str", Some(s("hi"))),
        case("option.option.some_some", Some(Some(5u8))),
        case("option.option.some_none", Some(None::<u8>)),
        case("result.ok", Ok::<u8, Error>(7)),
        case("result.err", Err::<u8, Error>(Error::Missing)),
        case("array.u16", [1u16, 2, 0xFFFF]),
        case("array.bool", [true, false, true]),
        case("tuple", (7u8, 100_000u32, true)),
        case("range", 3u16..10),
        case("range_inclusive", -5i8..=5),
        // structs
        case(
            "struct.bits",
            Bits {
                on: true,
                small: U4::new(9).unwrap(),
                signed: I5::new(-7).unwrap(),
                nib: Nibble::new(0xA).unwrap(),
                byte: 0x42,
                wide: U10::new(0x3FF).unwrap(),
                last: true,
            },
        ),
        case(
            "struct.nested",
            OuterOwned {
                tag: 0x1234,
                inner: inner(3, "note"),
                items: vec![inner(1, ""), inner(2, "x")],
                after: 0xAA,
            },
        )
        .borrowed(borrowed!(Outer<'_>)),
        case(
            "struct.flags.relocated",
            FlaggedOwned {
                a: u3(5),
                early: Some(s("hi")),
                res: Ok(7),
                tagged: Tagged::Pair {
                    first: Some(1),
                    second: None,
                },
                late: Some(300),
            },
        )
        .borrowed(borrowed!(Flagged<'_>)),
        case(
            "struct.flags.relocated_other",
            FlaggedOwned {
                a: u3(0),
                early: None,
                res: Err(Error::TooBig),
                tagged: Tagged::Pair {
                    first: None,
                    second: Some(2),
                },
                late: None,
            },
        ),
        case("struct.tuple", PairOwned(7, Some(s("hi")))).borrowed(borrowed!(Pair<'_>)),
        case("struct.tuple.none", PairOwned(0, None)),
        case("struct.tuple.plain", Rgb(1, 2, 3)),
        case("struct.unit", Marker),
        case(
            "struct.sized",
            SizedPoint {
                x: -1,
                y: 300,
                visible: true,
            },
        ),
        case(
            "struct.final_structure",
            FinalHeaderOwned {
                version: 1,
                name: s("fw"),
                crc: 0xBEEF,
            },
        ),
        case(
            "struct.generic",
            Timed {
                t: 7,
                value: 300u16,
            },
        ),
        case("struct.generic_const", Window { samples: [1, 2, 3] }),
        case(
            "struct.phantom",
            Quantity::<Marker> {
                value: -5,
                _unit: core::marker::PhantomData,
            },
        ),
        case("struct.validate.ok", Ordered { lo: 1, hi: 2 }),
        case("struct.validate.rejected", Ordered { lo: 1, hi: 2 }),
        case(
            "struct.tail_size",
            FramedOwned {
                magic: 0xABCD,
                size: TailSize(7),
                name: s("ab"),
                count: 7,
            },
        ),
        case("struct.sparse.empty", SparseOwned::default()),
        case(
            "struct.sparse.slots",
            SparseOwned {
                speed: Some(1500),
                name: None,
                samples: Some(vec![-1, 0, 1000]),
                on: Some(true),
            },
        )
        .borrowed(borrowed!(Sparse<'_>)),
        case("struct.recursive.list", tree()).borrowed(borrowed!(Tree<'_>)),
        case(
            "struct.recursive.box",
            ExprOwned::Add(
                Box::new(ExprOwned::Lit(2)),
                Box::new(ExprOwned::Neg(Box::new(ExprOwned::Lit(3)))),
            ),
        )
        .borrowed(borrowed!(Expr<'_>)),
        // enums
        case("enum.unit", ShapeOwned::Empty),
        case("enum.tuple", ShapeOwned::Circle(5)),
        case("enum.struct", shape_rect()).borrowed(borrowed!(Shape<'_>)),
        case("enum.explicit_discriminant", ShapeOwned::Far),
        case("enum.unknown_variant", ShapeOwned::Unknown(10)),
        case("enum.appended_variant", ShapeV2Owned::Polygon(vec![1, 2])),
        case(
            "enum.sub_byte.seven",
            I2cAddress::Seven(U7::new(0x50).unwrap()),
        ),
        case(
            "enum.sub_byte.ten",
            I2cAddress::Ten(U10::new(0x3FF).unwrap()),
        ),
        case("enum.sized", Command::Go(513)),
        case("enum.sized.wide_discriminant", Command::Hold(1)),
        case("enum.u2", Error::TooBig),
        case("enum.nibble.unit", Level::Low),
        case("enum.nibble.payload", Level::High(7)),
        case("enum.cfg.kept", CfgSubset::A(5)),
        case("enum.cfg.kept_after_gap", CfgSubset::C),
        case("enum.cfg.compiled_out", CfgSubset::C),
        // evolution
        case("evolution.v1", config_v1()),
        case("evolution.v2", config_v2()),
        case(
            "evolution.new_reads_old",
            ConfigV2Owned {
                rate: 1000,
                name: s("abc"),
                timeout: 7,
                retries: None,
                tags: None,
            },
        ),
        case("evolution.old_reads_new", config_v1()),
        // fixed layout
        case(
            "fixed.numbers",
            FixedNumbers {
                a: 0x12,
                b: 0x3456,
                c: 0x789A_BCDE,
                d: 0x0102_0304_0506_0708,
                e: -2,
                f: -300,
                g: -70_000,
                h: i64::MIN + 1,
                x: 0.25,
                y: -2.5,
                words: [0xAAAA, 7],
            },
        ),
        case(
            "fixed.bits",
            FixedBits {
                a: true,
                n: Nibble::new(0xA).unwrap(),
                b: u3(5),
                s: I5::new(-3).unwrap(),
                x: 0x1234,
                wide: U20::new(0xF_EDCB).unwrap(),
                i: I12::new(-2).unwrap(),
            },
        ),
        case(
            "fixed.nested",
            FixedOuter {
                tag: 9,
                inner: FixedInner {
                    id: 0xCAFE,
                    hash: [1, 2, 3, 4],
                },
                signed: [-1, 2],
            },
        ),
        case(
            "fixed.option.none",
            FixedMaybe {
                id: 0x0102,
                on: true,
                extra: None,
            },
        ),
        case(
            "fixed.option.some",
            FixedMaybe {
                id: 0x0102,
                on: false,
                extra: Some(0xDEAD_BEEF),
            },
        ),
        case("fixed.enum.unit", FixedOp::Stop),
        case(
            "fixed.enum.fields",
            FixedOp::Copy {
                dst: 0x10,
                src: 0x2030,
            },
        ),
        // Series
        case("series.u16", SeriesOwned(vec![1u16, 2, 3, 100])),
        case(
            "series.struct",
            SeriesOwned(vec![
                Sample {
                    t: 0,
                    value: -5,
                    ok: true,
                },
                Sample {
                    t: 10,
                    value: -4,
                    ok: true,
                },
                Sample {
                    t: 20,
                    value: 7,
                    ok: false,
                },
            ]),
        )
        .borrowed(borrowed!(Series<'_, Sample>)),
        // #[wire(W)] (DER-19)
        case(
            "wire.struct",
            Wired {
                small: -7,
                count: 300,
                raw: 0xABC,
                last: true,
            },
        ),
        case(
            "wire.struct.out_of_range",
            Wired {
                small: 9,
                count: 0,
                raw: 0,
                last: false,
            },
        ),
        case("wire.enum.tuple", WiredOp::Step(-3)),
        case("wire.enum.named", WiredOp::Set { value: 70_000 }),
        case(
            "wire.series",
            SeriesOwned(vec![
                WiredSample { t: 0, value: -5 },
                WiredSample { t: 10, value: 2047 },
                WiredSample {
                    t: 20,
                    value: -2048,
                },
            ]),
        )
        .borrowed(borrowed!(Series<'_, WiredSample>)),
        // the derive's field attributes behind cfg_attr (API-60)
        case(
            "cfg_attr.v1",
            GatedV1 {
                small: -7,
                last: true,
            },
        ),
        case(
            "cfg_attr.v2",
            GatedV2 {
                small: -7,
                last: true,
                extra: 5,
            },
        ),
        case(
            "cfg_attr.new_reads_old",
            GatedV2 {
                small: -7,
                last: true,
                extra: 3,
            },
        ),
        // transparent (DER-18), each with the bytes of its plain twin
        case("transparent.u64", Count(u64::MAX)),
        case("transparent.unib32", Index(UNib32(u32::MAX))),
        case(
            "transparent.fields.plain",
            PlainFieldsOwned {
                count: 300,
                digit: nib(10),
                index: UNib32(8),
                volts: 1.5,
                label: s("hi"),
                end: 7,
            },
        )
        .borrowed(borrowed!(PlainFields<'_>)),
        case(
            "transparent.fields",
            WrappedFieldsOwned {
                count: Count(300),
                digit: Digit(nib(10)),
                index: Index(UNib32(8)),
                volts: volts(1.5),
                label: LabelOwned(s("hi")),
                end: 7,
            },
        )
        .borrowed(borrowed!(WrappedFields<'_>)),
        case(
            "transparent.sized.plain",
            PlainSized {
                on: true,
                digit: nib(5),
                count: 1,
                volts: -2.0,
            },
        ),
        case(
            "transparent.sized",
            WrappedSized {
                on: true,
                digit: Digit(nib(5)),
                count: Count(1),
                volts: volts(-2.0),
            },
        ),
        case(
            "transparent.vec.unib32.plain",
            vec![UNib32(7), UNib32(8), UNib32(300)],
        ),
        case(
            "transparent.vec.unib32",
            vec![Index(UNib32(7)), Index(UNib32(8)), Index(UNib32(300))],
        ),
        case("transparent.vec.nibble.plain", vec![nib(1), nib(2), nib(3)]),
        case(
            "transparent.vec.nibble",
            vec![Digit(nib(1)), Digit(nib(2)), Digit(nib(3))],
        ),
        case("transparent.option.plain", Some(300u64)),
        case("transparent.option", Some(Count(300))),
        case("transparent.array.plain", [nib(1), nib(2), nib(3)]),
        case(
            "transparent.array",
            [Digit(nib(1)), Digit(nib(2)), Digit(nib(3))],
        ),
        case(
            "transparent.series.plain",
            SeriesOwned(vec![1u64, 2, 3, 100]),
        ),
        case(
            "transparent.series",
            SeriesOwned(vec![Count(1), Count(2), Count(3), Count(100)]),
        )
        .borrowed(borrowed!(Series<'_, Count>)),
        case(
            "transparent.series.struct.plain",
            SeriesOwned(vec![
                PlainSample { t: 0, volts: 1.5 },
                PlainSample { t: 10, volts: 2.0 },
            ]),
        ),
        case(
            "transparent.series.struct",
            SeriesOwned(vec![
                WrappedSample {
                    t: Count(0),
                    volts: volts(1.5),
                },
                WrappedSample {
                    t: Count(10),
                    volts: volts(2.0),
                },
            ]),
        )
        .borrowed(borrowed!(Series<'_, WrappedSample>)),
        // not in the API model
        case_head_tail(
            "head_tail.struct",
            HtRecord {
                ch: 7,
                maybe: Some(0x0304),
                list: wire_weaver::shrink_wrap::head_tail::List::new(&[0x0A0B, 2]),
                name: "ab",
                on: true,
            },
        ),
        // ww_link's own message, not in the API model
        case_debug("link_setup.without_meta", || link_setup(None)),
        case_debug("link_setup.with_meta", || {
            link_setup(Some(ww_link::ClientMetaOwned {
                program: "ww".into(),
                app_version: "0.5.1".into(),
                label: "bench".into(),
                kind: ww_link::ClientKind::Notebook,
                pid: 4711,
                user: "roman".into(),
                host: "pc1".into(),
                purpose: Some("dfu".into()),
                instance_id: 0x0102_0304_0506_0708,
            }))
        }),
        // ww_link's Disconnect payload, not in the API model
        case_borrowed_only(
            "disconnect_reason.hub_handover",
            wire_weaver::DisconnectReason::HubHandover,
        ),
        // a reason appended after HubHandover, as this version reads it
        case_borrowed_only(
            "disconnect_reason.from_newer",
            wire_weaver::DisconnectReason::Newer(7),
        ),
        case("stdlib.timebase.steer_point", steer_point(None)),
        case(
            "stdlib.timebase.steer_point.steerer",
            steer_point(Some(steerer(0x0102_0304_0506_0708, "pc1", "ww hub", 4711))),
        ),
        case(
            "stdlib.si.dim.volt",
            dim(<ww_si::units::Volt as ww_si::Unit>::DIM),
        ),
        case(
            "stdlib.si.dim.rad_bit",
            dim(ww_si::Dim::new(-1, 0, 0, 0, 0, 0, 0)
                .with_rad(1)
                .with_bit(-3)),
        ),
        case(
            "stdlib.si.dim.counted",
            dim(ww_si::Dim::counted("ww_si", "Sample")
                .div(ww_si::Dim::counted("ww_si", "Frame"))
                .mul(ww_si::Dim::new(-1, 0, 0, 0, 0, 0, 0))),
        ),
        case("stdlib.si.dim.rejected", dim(ww_si::Dim::ONE)),
        case(
            "stdlib.si.dim.root",
            dim(<ww_si::units::Volt as ww_si::Unit>::DIM
                .div(<ww_si::units::Hertz as ww_si::Unit>::DIM.sqrt())),
        ),
        case("stdlib.si.dim.root.rejected", dim(ww_si::Dim::ONE)),
        case(
            "stdlib.si.fixed.pow10",
            ww_si::Fixed::<ww_si::units::Second, ww_si::prefix::Pow10<-4>, u32>::new(0xDEAD_BEEF),
        ),
        case("stdlib.si.fixed.duty16", ww_si::Duty16::new(0xBEEF)),
        case("stdlib.si.fixed.bounded", fixed_bounded(5000)),
        case("stdlib.si.fixed.bounded.rejected", fixed_bounded(1)),
        case(
            "stdlib.si.fixed.customary",
            ww_si::Fixed::<
                ww_si::units::Times<ww_si::units::PoundForce, ww_si::units::Second>,
                ww_si::prefix::Milli,
                i32,
            >::new(-2000),
        ),
        case("stdlib.si.quantity.milli", ww_si::Volt::milli(3300u32)),
        case(
            "stdlib.si.quantity.kibi",
            ww_si::Quantity::<ww_si::units::Byte>::new(ww_si::Prefix::Kibi, 64u32),
        ),
        case(
            "stdlib.si.quantity.pow10",
            ww_si::Quantity::<ww_si::units::Second>::new(ww_si::Prefix::from_pow10(-4).unwrap(), 12345u32),
        ),
        case(
            "stdlib.si.quantity.pow2",
            ww_si::Quantity::<ww_si::units::Second>::new(ww_si::Prefix::from_pow2(-15).unwrap(), 32768u32),
        ),
        case(
            "stdlib.si.quantity.fullscale",
            ww_si::Quantity::<ww_si::units::Ratio>::new(
                ww_si::Prefix::from_magnitude(&ww_si::Magnitude::ratio(1, 65535)).unwrap(),
                32768u32,
            ),
        ),
        case(
            "stdlib.si.quantity.unknown_scale",
            ww_si::Quantity::<ww_si::units::Ratio>::new(ww_si::Prefix::Unknown(1000), 5u32),
        ),
        case("stdlib.si.quantity.bounded", quantity_rail(3300)),
        case("stdlib.si.quantity.bounded.rejected", quantity_rail(3300)),
        case("stdlib.si.magnitude.one", ww_si::Magnitude::ONE),
        case("stdlib.si.magnitude.milli", ww_si::Magnitude::pow10(-3)),
        case("stdlib.si.magnitude.kibi", ww_si::Magnitude::pow2(10)),
        case(
            "stdlib.si.magnitude.duty16",
            ww_si::Magnitude::ratio(1, 65535),
        ),
        case(
            "stdlib.si.magnitude.degree",
            ww_si::Magnitude::new(1, 180, 0, 0, 1),
        ),
        case(
            "stdlib.si.magnitude.sqrt2",
            ww_si::Magnitude::ratio(2, 1).sqrt(),
        ),
        case(
            "stdlib.si.magnitude.decibel_in_neper",
            ww_si::Magnitude::LN10.div(ww_si::Magnitude::ratio(20, 1)),
        ),
        case(
            "stdlib.si.magnitude.pound",
            ww_si::Magnitude::ratio(45359237, 100_000_000),
        ),
        case("stdlib.si.magnitude.rejected", ww_si::Magnitude::ONE),
        case("stdlib.si.point.celsius", PointCelsius::from_raw(-1234)),
        case("stdlib.si.point.bounded", point_bounded(-400)),
        case("stdlib.si.point.bounded.rejected", point_bounded(1250)),
        case(
            "stdlib.si.index.in_struct",
            WithIndex {
                slot: ww_si::Index::new(1000),
                tag: 7,
            },
        ),
        case("stdlib.si.index_from_1", index_from_1(4)),
        case("stdlib.si.index_from_1.zero.rejected", index_from_1(1)),
        case("stdlib.si.index_from_1.bounded.rejected", index_from_1(1)),
        case("stdlib.si.seq.nibble_wrap", seq_wrap(15)),
        case("stdlib.si.offset.zero", ww_si::Offset::ZERO),
        case("stdlib.si.offset.celsius", ww_si::Offset::decimal(27315, -2)),
        case("stdlib.si.offset.minus_forty", ww_si::Offset::int(-40)),
        case("stdlib.si.offset.fahrenheit", ww_si::Offset::ratio(45967, 180)),
        case("stdlib.si.offset.rejected", ww_si::Offset::ZERO),
        case(
            "stdlib.si.si_expr.volt",
            si_expr(<ww_si::units::Volt as ww_si::Unit>::EXPR),
        ),
        case(
            "stdlib.si.si_expr.pound_force",
            si_expr(<ww_si::units::PoundForce as ww_si::Unit>::EXPR),
        ),
        case(
            "stdlib.si.si_expr.sqrt",
            si_expr(
                <ww_si::units::Per<ww_si::units::Volt, ww_si::units::Sqrt<ww_si::units::Hertz>> as ww_si::Unit>::EXPR,
            ),
        ),
        case(
            "stdlib.si.si_expr.rejected",
            si_expr(<ww_si::units::Volt as ww_si::Unit>::EXPR),
        ),
        case("model.marker_def.unit", rpm()),
        case("model.marker_def.unit_conventional", inch_of_mercury_32f()),
        case(
            "model.marker_def.scale",
            ww_self::MarkerDefOwned::Scale(ww_self::ScaleDefOwned {
                symbol: String::new(),
                magnitude: ww_si::Magnitude::ratio(7, 100),
            }),
        ),
        case("model.marker_def.per", ww_self::MarkerDefOwned::Per),
        case("model.marker_def.origin", celsius()),
        case("model.marker_def.angle", ww_self::MarkerDefOwned::Angle),
        case(
            "model.item_struct_extra.transparent",
            ww_self::ItemStructExtraOwned {
                transparent: true,
                marker: None,
            },
        ),
        case(
            "model.item_struct_extra.marker",
            ww_self::ItemStructExtraOwned {
                transparent: false,
                marker: Some(rpm()),
            },
        ),
        case_debug("stdlib.timebase.sync_status", || {
            ww_timebase::SyncStatusOwned {
                state: ww_timebase::SyncState::Free,
                offset: None,
                last_lock: None,
                source: None,
                error: None,
                slew_us: None,
                steerer: None,
                noted: None,
            }
        }),
        case_debug("stdlib.timebase.sync_status.steerers", || {
            ww_timebase::SyncStatusOwned {
                state: ww_timebase::SyncState::Locked,
                offset: None,
                last_lock: Some(micros(5)),
                source: Some(ww_timebase::SyncSource::Host),
                error: Some(micros(300)),
                slew_us: Some(-12),
                steerer: Some(steerer(0x0102_0304_0506_0708, "pc1", "ww hub", 4711)),
                noted: Some(steerer(2, "lab", "app", 7)),
            }
        }),
        case_debug("tail_bytes", || WithTailOwned {
            kind: 3,
            rest: TailBytesOwned(vec![1, 2, 3]),
        }),
    ]
}

type FixedBounded =
    ww_si::Fixed<ww_si::units::Second, ww_si::prefix::Milli, u16, ww_si::Range<1, 5000>>;

fn fixed_bounded(ms: u16) -> FixedBounded {
    FixedBounded::try_new(ms).unwrap()
}

type QuantityRail = ww_si::Quantity<ww_si::units::Volt, ww_si::RangeDec<0, 33, -1>>;

fn quantity_rail(mv: u32) -> QuantityRail {
    QuantityRail::try_new(ww_si::Prefix::Milli, mv).unwrap()
}

type PointCelsius = ww_si::Point<
    ww_si::origins::Celsius,
    ww_si::Fixed<ww_si::units::Kelvin, ww_si::prefix::Centi, i16>,
>;
type PointBounded = ww_si::Point<
    ww_si::origins::Celsius,
    ww_si::Fixed<ww_si::units::Kelvin, ww_si::prefix::Deci, i16, ww_si::Range<-400, 1250>>,
>;

fn point_bounded(dd: i16) -> PointBounded {
    PointBounded::new(ww_si::Fixed::try_new(dd).unwrap())
}

fn index_from_1(n: u8) -> ww_si::IndexFrom1<ww_si::units::Item, u8, ww_si::Range<1, 4>> {
    ww_si::IndexFrom1::try_new(n).unwrap()
}

/// `n`, then the one after it: 15 wraps to 0.
fn seq_wrap(n: u8) -> SeqPair {
    let last = ww_si::Seq::new(Nibble::new(n).unwrap());
    SeqPair {
        last,
        next: last.next(),
    }
}

fn dim(d: ww_si::Dim<'_>) -> ww_si::DimOwned {
    d.into()
}

fn si_expr(e: &ww_si::UnitExpr) -> ww_si::SIExprOwned {
    e.into()
}

/// A user's `unit!(Rpm, "rpm", ..)`: rad/s times π/30.
fn rpm() -> ww_self::MarkerDefOwned {
    ww_self::MarkerDefOwned::Unit(ww_self::UnitDefOwned {
        symbol: "rpm".into(),
        dim: dim(ww_si::Dim::new(-1, 0, 0, 0, 0, 0, 0).with_rad(1)),
        magnitude: ww_si::Magnitude::new(1, 30, 0, 0, 1),
    })
}

/// `unit!(InchOfMercury32f, .., conventional("NIST SP 811", 2008))`: a scale that is a value a source prints.
fn inch_of_mercury_32f() -> ww_self::MarkerDefOwned {
    ww_self::MarkerDefOwned::ConventionalUnit(ww_self::ConventionalUnitDefOwned {
        unit: ww_self::UnitDefOwned {
            symbol: "inHg (32 °F)".into(),
            dim: dim(ww_si::Dim::new(-2, -1, 1, 0, 0, 0, 0)),
            magnitude: ww_si::Magnitude::ratio(338638, 100),
        },
        source: "NIST SP 811".into(),
        year: 2008,
    })
}

/// °C as `origin!` declares it, from 0 K: no reference, no modulus.
fn celsius() -> ww_self::MarkerDefOwned {
    ww_self::MarkerDefOwned::Origin(ww_self::OriginDefOwned {
        unit: ww_self::UnitDefOwned {
            symbol: "K".into(),
            dim: dim(ww_si::Dim::new(0, 0, 0, 0, 1, 0, 0)),
            magnitude: ww_si::Magnitude::ONE,
            },
        symbol: "°C".into(),
        reference: None,
        offset: ww_si::Offset::decimal(27315, -2),
        modulus: None,
        step: ww_si::Magnitude::ONE,
        shown: ww_si::Offset::decimal(27315, -2),
    })
}

fn micros(value: u32) -> ww_si::Second {
    ww_si::Second::from_parts(
        ww_si::Prefix::Micro,
        ww_si::ww_numeric::NumericValue::U32(value),
    )
}

fn steerer(id: u64, host: &str, program: &str, pid: u32) -> ww_timebase::SteererOwned {
    ww_timebase::SteererOwned {
        id,
        host: host.into(),
        program: program.into(),
        pid,
    }
}

fn steer_point(steerer: Option<ww_timebase::SteererOwned>) -> ww_timebase::SteerPointOwned {
    ww_timebase::SteerPointOwned {
        local: micros(1_000_000),
        shared_us: 1_700_000_000_000_000,
        rate_ppb: -9_900_990,
        error_us: 250,
        steerer,
    }
}

fn link_setup(client_meta: Option<ww_link::ClientMetaOwned>) -> ww_link::LinkSetupOwned {
    ww_link::LinkSetupOwned {
        host_user_version: wire_weaver::ww_version::FullVersionOwned::new(
            "test".into(),
            wire_weaver::ww_version::VersionOwned::new(0, 1, 0),
        ),
        host_max_message_len: 4096,
        token: 7,
        client_meta,
    }
}

/// The Rust derive as a consumer of the corpus.
pub struct Derive(std::collections::HashMap<&'static str, Typed>);

impl Default for Derive {
    fn default() -> Self {
        Derive(table().into_iter().map(|t| (t.name, t)).collect())
    }
}

impl Derive {
    pub fn get(&self, name: &str) -> Option<&Typed> {
        self.0.get(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.0.keys().copied()
    }
}

impl crate::Consumer for Derive {
    const NAME: &'static str = "derive";

    fn encode(&self, case: &crate::corpus::Case) -> Option<Result<Vec<u8>, String>> {
        Some((self.get(&case.name)?.encode)())
    }

    fn decode(&self, case: &crate::corpus::Case) -> Option<crate::DecodeResult> {
        Some(match (self.get(&case.name)?.decode)(&case.golden()) {
            Decoded::Equal => Ok(Ok(())),
            Decoded::Differs(e) => Ok(Err(e)),
            Decoded::Failed(e) => Err(e),
        })
    }

    fn extra(&self, case: &crate::corpus::Case) -> Result<(), String> {
        let Some(round_trip) = self.get(&case.name).and_then(|t| t.borrowed) else {
            return Ok(());
        };
        let golden = case.golden();
        match round_trip(&golden) {
            Ok(bytes) if bytes == golden => Ok(()),
            Ok(bytes) => Err(format!(
                "borrowed type: {}",
                crate::corpus::bytes_mismatch(&bytes, &golden)
            )),
            Err(e) => Err(format!("borrowed type: {e:?}")),
        }
    }
}
