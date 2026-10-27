//! `#[derive_shrink_wrap(transparent)]` (DER-18): a struct of one field that isn't zero bits is that field on the
//! wire: its `ELEMENT_SIZE`, the same bytes in every position (an `Unsized` and a `sized` parent, `Vec`, `Option`,
//! an array), the same series columns and tags, its `SeriesStore` and `SeriesCells`.

use core::marker::PhantomData;
use shrink_wrap::prelude::*;
use shrink_wrap::series::{
    CodecPolicy, Series, SeriesCells, SeriesOwned, SeriesStore, empty_cells, write_store_series,
};
use shrink_wrap::{ElementSize, SerializeShrinkWrap};

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Micro;
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Second;

/// The shape of `ww_si::In`: a prefix, a unit and the storage, `P` among them (the derive's own `P` once clashed).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    transparent,
    series(cells),
    derive(Clone, Copy, Debug, PartialEq, Default)
)]
#[repr(transparent)]
struct In<P, U, T> {
    value: T,
    _prefix: PhantomData<P>,
    _unit: PhantomData<(U, P)>,
}

type Us<T> = In<Micro, Second, T>;

fn us<T>(value: T) -> Us<T> {
    In {
        value,
        _prefix: PhantomData,
        _unit: PhantomData,
    }
}

#[test]
fn repr_is_kept() {
    assert_eq!(core::mem::size_of::<Us<u16>>(), 2);
    assert_eq!(
        core::mem::align_of::<Us<u64>>(),
        core::mem::align_of::<u64>()
    );
}

#[test]
fn element_size_is_the_fields() {
    fn size<T: SerializeShrinkWrap>() -> ElementSize {
        T::ELEMENT_SIZE
    }
    assert_eq!(size::<Us<u64>>(), size::<u64>());
    assert_eq!(size::<Us<f32>>(), size::<f32>());
    assert_eq!(size::<Us<Nibble>>(), size::<Nibble>());
    assert_eq!(size::<Us<UNib32>>(), size::<UNib32>());
    assert_eq!(size::<Us<Vec<u16>>>(), size::<Vec<u16>>());
    assert_eq!(size::<Us<u64>>(), ElementSize::Sized { size_bits: 64 });
    assert_eq!(size::<Us<UNib32>>(), ElementSize::SelfDescribing);
    assert_eq!(
        <Us<u64> as SerializeShrinkWrap>::MAX_SIZE,
        <u64 as SerializeShrinkWrap>::MAX_SIZE
    );
    const { assert!(!<Us<u64> as SerializeShrinkWrap>::PUSHES_LEN) };
}

#[derive_shrink_wrap(owned, derive(Clone, Debug, PartialEq))]
struct PlainEvolvableOwned {
    f: bool,
    a: u64,
    b: UNib32,
    c: f32,
    n: Nibble,
    d: Vec<u16>,
    e: Option<u64>,
    g: [u32; 2],
}

#[derive_shrink_wrap(owned, derive(Clone, Debug, PartialEq))]
struct TypedEvolvableOwned {
    f: bool,
    a: Us<u64>,
    b: Us<UNib32>,
    c: Us<f32>,
    n: Us<Nibble>,
    d: Vec<Us<u16>>,
    e: Option<Us<u64>>,
    g: [Us<u32>; 2],
}

#[test]
fn same_bytes_in_an_unsized_parent() {
    let plain = PlainEvolvableOwned {
        f: true,
        a: 0x1_0000_0000,
        b: UNib32(1000),
        c: -1.5,
        n: Nibble::new(9).unwrap(),
        d: vec![1, 2, 3],
        e: Some(7),
        g: [5, 6],
    };
    let typed = TypedEvolvableOwned {
        f: true,
        a: us(0x1_0000_0000),
        b: us(UNib32(1000)),
        c: us(-1.5),
        n: us(Nibble::new(9).unwrap()),
        d: vec![us(1), us(2), us(3)],
        e: Some(us(7)),
        g: [us(5), us(6)],
    };
    let bytes = plain.to_ww_bytes_owned().unwrap();
    assert_eq!(typed.to_ww_bytes_owned().unwrap(), bytes);
    assert_eq!(
        TypedEvolvableOwned::from_ww_bytes_owned(&bytes).unwrap(),
        typed
    );
}

#[derive_shrink_wrap(borrowed, owned, sized, derive(Clone, Copy, Debug, PartialEq))]
struct PlainSized {
    f: bool,
    a: u32,
    n: Nibble,
    x: f32,
}

/// `sized` builds: with `final_structure` instead of `transparent` this was "TypedSized must be Sized".
#[derive_shrink_wrap(borrowed, owned, sized, derive(Clone, Copy, Debug, PartialEq))]
struct TypedSized {
    f: bool,
    a: Us<u32>,
    n: Us<Nibble>,
    x: Us<f32>,
}

#[test]
fn same_bytes_in_a_sized_parent() {
    let plain = PlainSized {
        f: true,
        a: 7,
        n: Nibble::new(3).unwrap(),
        x: 0.5,
    };
    let typed = TypedSized {
        f: true,
        a: us(7),
        n: us(Nibble::new(3).unwrap()),
        x: us(0.5),
    };
    assert_eq!(
        <TypedSized as SerializeShrinkWrap>::ELEMENT_SIZE,
        <PlainSized as SerializeShrinkWrap>::ELEMENT_SIZE
    );
    let bytes = plain.to_ww_bytes_owned().unwrap();
    assert_eq!(typed.to_ww_bytes_owned().unwrap(), bytes);
    assert_eq!(TypedSized::from_ww_bytes(&bytes).unwrap(), typed);
}

#[test]
fn same_bytes_alone_and_in_lists() {
    assert_eq!(
        us(0xABCDu64).to_ww_bytes_owned().unwrap(),
        0xABCDu64.to_ww_bytes_owned().unwrap()
    );
    let plain: Vec<UNib32> = (0..10).map(|i| UNib32(i * 100)).collect();
    let typed: Vec<Us<UNib32>> = plain.iter().copied().map(us).collect();
    let bytes = plain.to_ww_bytes_owned().unwrap();
    assert_eq!(typed.to_ww_bytes_owned().unwrap(), bytes);
    assert_eq!(
        Vec::<Us<UNib32>>::from_ww_bytes_owned(&bytes).unwrap(),
        typed
    );
    let plain: Option<[Nibble; 3]> = Some([Nibble::new(1).unwrap(); 3]);
    let typed: Option<[Us<Nibble>; 3]> = Some([us(Nibble::new(1).unwrap()); 3]);
    assert_eq!(
        typed.to_ww_bytes_owned().unwrap(),
        plain.to_ww_bytes_owned().unwrap()
    );
}

/// Borrowed and owned: a wrapped string, a tuple struct with `()`.
#[derive_shrink_wrap(owned, transparent, derive(Clone, Debug, PartialEq))]
struct Name<'i> {
    name: &'i str,
}

#[derive_shrink_wrap(
    borrowed,
    owned,
    transparent,
    series,
    derive(Clone, Copy, Debug, PartialEq)
)]
struct Count((), u32);

#[test]
fn borrowed_owned_and_tuple() {
    let bytes = "abc".to_ww_bytes_owned().unwrap();
    let mut buf = [0u8; 8];
    assert_eq!(
        Name { name: "abc" }.to_ww_bytes(&mut buf).unwrap(),
        &bytes[..]
    );
    assert_eq!(Name::from_ww_bytes(&bytes).unwrap(), Name { name: "abc" });
    assert_eq!(
        NameOwned::from_ww_bytes_owned(&bytes).unwrap(),
        NameOwned { name: "abc".into() }
    );
    assert_eq!(
        Count((), 9).to_ww_bytes_owned().unwrap(),
        9u32.to_ww_bytes_owned().unwrap()
    );
    let items = [Count((), 1), Count((), 5), Count((), 9)];
    let bytes = Series::new(&items).to_ww_bytes_owned().unwrap();
    assert_eq!(
        bytes,
        Series::new(&[1u32, 5, 9]).to_ww_bytes_owned().unwrap()
    );
}

/// The `Bounds` of `ww_si::In` will check on decode: `validate` with `transparent`.
#[derive_shrink_wrap(borrowed, owned, transparent, validate = Percent::check, derive(Clone, Copy, Debug, PartialEq))]
struct Percent {
    value: u8,
}

impl Percent {
    fn check(&self) -> Result<(), ShrinkWrapError> {
        if self.value <= 100 {
            Ok(())
        } else {
            Err(ShrinkWrapError::SubtypeOutOfRange)
        }
    }
}

#[test]
fn validate_checks_on_decode() {
    assert_eq!(
        Percent::from_ww_bytes(&[100]).unwrap(),
        Percent { value: 100 }
    );
    assert_eq!(
        Percent::from_ww_bytes(&[101]),
        Err(ShrinkWrapError::SubtypeOutOfRange)
    );
    assert_eq!(
        Vec::<Percent>::from_ww_bytes_owned(&vec![50u8, 101].to_ww_bytes_owned().unwrap()),
        Err(ShrinkWrapError::SubtypeOutOfRange)
    );
}

#[derive_shrink_wrap(borrowed, owned, series(cells), derive(Clone, Copy, Debug, PartialEq))]
struct PlainRow {
    t: u64,
    v: f32,
    n: Nibble,
    u: UNib32,
    o: Option<u16>,
}

#[derive_shrink_wrap(borrowed, owned, series(cells), derive(Clone, Copy, Debug, PartialEq))]
struct TypedRow {
    t: Us<u64>,
    v: Us<f32>,
    n: Us<Nibble>,
    u: Us<UNib32>,
    o: Us<Option<u16>>,
}

fn rows() -> (Vec<PlainRow>, Vec<TypedRow>) {
    let plain: Vec<PlainRow> = (0..24u32)
        .map(|k| PlainRow {
            t: 1_000_000 + k as u64 * 1000,
            v: k as f32 * 0.25,
            n: Nibble::new((k % 16) as u8).unwrap(),
            u: UNib32(k * 7),
            o: (k % 5 != 0).then_some(k as u16),
        })
        .collect();
    let typed = plain
        .iter()
        .map(|p| TypedRow {
            t: us(p.t),
            v: us(p.v),
            n: us(p.n),
            u: us(p.u),
            o: us(p.o),
        })
        .collect();
    (plain, typed)
}

#[test]
fn series_columns_are_the_fields() {
    let (plain, typed) = rows();
    let bytes = Series::new(&plain).to_ww_bytes_owned().unwrap();
    assert_eq!(Series::new(&typed).to_ww_bytes_owned().unwrap(), bytes);
    assert_eq!(
        SeriesOwned::<TypedRow>::from_ww_bytes_owned(&bytes)
            .unwrap()
            .0,
        typed
    );
    let back: Vec<TypedRow> = Series::<TypedRow>::from_ww_bytes(&bytes)
        .unwrap()
        .iter()
        .collect();
    assert_eq!(back, typed);
    // a Series of the wrapper itself
    let ts: Vec<Us<u64>> = typed.iter().map(|r| r.t).collect();
    let tp: Vec<u64> = plain.iter().map(|r| r.t).collect();
    assert_eq!(
        Series::new(&ts).to_ww_bytes_owned().unwrap(),
        Series::new(&tp).to_ww_bytes_owned().unwrap()
    );
}

#[derive_shrink_wrap(borrowed, owned, ww_repr = u4, series, derive(Clone, Copy, Debug, PartialEq))]
enum Mode {
    Off,
    On(u8),
}

#[derive_shrink_wrap(borrowed, owned, series, derive(Clone, Copy, Debug, PartialEq))]
struct PlainTagged {
    m: Mode,
    x: u16,
}

#[derive_shrink_wrap(borrowed, owned, series, derive(Clone, Copy, Debug, PartialEq))]
struct TypedTagged {
    m: Us<Mode>,
    x: u16,
}

/// A wrapped enum keeps its tag: a change of it restarts the fields after it, as the plain field does.
#[test]
fn wrapped_enum_restarts_later_fields() {
    let modes = [Mode::Off, Mode::On(1), Mode::On(2), Mode::Off, Mode::On(3)];
    let plain: Vec<PlainTagged> = modes
        .iter()
        .enumerate()
        .map(|(i, m)| PlainTagged {
            m: *m,
            x: 100 + i as u16,
        })
        .collect();
    let typed: Vec<TypedTagged> = plain
        .iter()
        .map(|p| TypedTagged { m: us(p.m), x: p.x })
        .collect();
    let policy = CodecPolicy::SMALLEST;
    let bytes = Series::with_policy(&plain, policy)
        .to_ww_bytes_owned()
        .unwrap();
    assert_eq!(
        Series::with_policy(&typed, policy)
            .to_ww_bytes_owned()
            .unwrap(),
        bytes
    );
    assert_eq!(
        SeriesOwned::<TypedTagged>::from_ww_bytes_owned(&bytes)
            .unwrap()
            .0,
        typed
    );
}

#[test]
fn store_and_cells_are_the_fields() {
    const N: usize = 32;
    let (mut plain, mut typed) = rows();
    // one variant of the Option over the rows, so the store can be written column by column
    for (p, t) in plain.iter_mut().zip(&mut typed) {
        p.o = Some(1);
        t.o = us(Some(1));
    }
    let mut ps = PlainRow::empty_store::<N>();
    let mut ts = TypedRow::empty_store::<N>();
    let cells = empty_cells::<TypedRow, N>();
    for (i, (p, t)) in plain.iter().zip(&typed).enumerate() {
        PlainRow::set(&mut ps, i, p);
        TypedRow::put(&cells, i, t);
    }
    TypedRow::copy_rows(&cells, 0, &mut ts, 0, typed.len());
    for (i, t) in typed.iter().enumerate() {
        assert_eq!(TypedRow::get(&ts, i), *t);
    }
    let rows = 0..typed.len();
    let policy = CodecPolicy::SMALLEST;
    let mut a = [0u8; 1024];
    let mut wr = BufWriter::new(&mut a);
    assert!(write_store_series::<PlainRow, _, N>(&mut wr, &ps, rows.clone(), &policy).unwrap());
    let a = wr.finish_and_take().unwrap().to_vec();
    let mut b = [0u8; 1024];
    let mut wr = BufWriter::new(&mut b);
    assert!(write_store_series::<TypedRow, _, N>(&mut wr, &ts, rows, &policy).unwrap());
    let b = wr.finish_and_take().unwrap();
    assert_eq!(a, b);
    assert_eq!(a, Series::new(&typed).to_ww_bytes_owned().unwrap());
}
