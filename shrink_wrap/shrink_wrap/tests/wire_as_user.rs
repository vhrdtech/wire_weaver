//! `#[wire(W)] field: T` with a user type `W` (DER-23): a one-byte handle in Rust, the full value it stands for on
//! the wire. Each type here has a twin with `W` as the field type, whose bytes it must match, also in a `Series` (as
//! a `#[series(opaque)]` field); a value without a handle is an error when it is read.

use core::mem::MaybeUninit;
use shrink_wrap::Error;
use shrink_wrap::prelude::*;
use shrink_wrap::series::{
    CodecPolicy, SeriesCells, SeriesColumns, SeriesOwned, SeriesStore, empty_cells,
    write_store_series,
};

/// The value a handle stands for.
#[derive_shrink_wrap(
    borrowed,
    owned,
    self_describing,
    series,
    derive(Debug, PartialEq, Clone, Copy)
)]
pub struct Scale {
    num: u32,
    #[wire(UNib32)]
    den: u32,
}

static SCALES: [Scale; 3] = [
    Scale { num: 1, den: 1 },
    Scale { num: 1, den: 65535 },
    Scale {
        num: 360,
        den: 65536,
    },
];

/// An index into [SCALES]: what Rust code keeps.
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct ScaleId(u8);

impl WireAs<ScaleId> for Scale {
    fn from_plain(value: ScaleId) -> Result<Self, Error> {
        Ok(SCALES[value.0 as usize])
    }

    fn into_plain(self) -> Result<ScaleId, Error> {
        SCALES
            .iter()
            .position(|s| *s == self)
            .map(|i| ScaleId(i as u8))
            .ok_or(Error::SubtypeOutOfRange)
    }
}

/// The store and cells of the `u8` index.
impl SeriesStore for ScaleId {
    type Store<const N: usize> = <u8 as SeriesStore>::Store<N>;

    fn empty_store<const N: usize>() -> Self::Store<N> {
        u8::empty_store::<N>()
    }

    fn init_store<const N: usize>(slot: &mut MaybeUninit<Self::Store<N>>) -> &mut Self::Store<N> {
        u8::init_store::<N>(slot)
    }

    fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
        u8::set::<N>(store, i, &item.0);
    }

    fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
        ScaleId(u8::get::<N>(store, i))
    }
}

// SAFETY: the cells of `u8`, and every `u8` (0 included) is a valid `ScaleId` here
unsafe impl SeriesCells for ScaleId {
    type Cells<const R: usize> = <u8 as SeriesCells>::Cells<R>;

    fn put<const R: usize>(cells: &Self::Cells<R>, i: usize, item: &Self) {
        u8::put::<R>(cells, i, &item.0);
    }

    fn copy_rows<const R: usize, const N: usize>(
        cells: &Self::Cells<R>,
        from: usize,
        store: &mut Self::Store<N>,
        to: usize,
        n: usize,
    ) {
        u8::copy_rows::<R, N>(cells, from, store, to, n);
    }
}

#[derive_shrink_wrap(borrowed, owned, series(cells), derive(Debug, PartialEq, Clone, Copy))]
struct Reading {
    value: u16,
    #[wire(Scale)]
    scale: ScaleId,
}

#[derive_shrink_wrap(borrowed, owned, series, derive(Debug, PartialEq, Clone, Copy))]
struct ReadingTwin {
    value: u16,
    #[series(opaque)]
    scale: Scale,
}

/// The shape of a prefix with an exact scale: named variants, and one holding a handle.
#[derive_shrink_wrap(
    borrowed,
    owned,
    self_describing,
    series(cells),
    ww_repr = unib32,
    derive(Debug, PartialEq, Clone, Copy)
)]
enum Prefix {
    Unit,
    Milli,
    Exact(#[wire(Scale)] ScaleId),
}

#[derive_shrink_wrap(
    borrowed,
    owned,
    self_describing,
    series,
    ww_repr = unib32,
    derive(Debug, PartialEq, Clone, Copy)
)]
enum PrefixTwin {
    Unit,
    Milli,
    // a named field: the same bytes, and `#[series(opaque)]` is only on named fields
    Exact {
        #[series(opaque)]
        scale: Scale,
    },
}

fn twin(p: Prefix) -> PrefixTwin {
    match p {
        Prefix::Unit => PrefixTwin::Unit,
        Prefix::Milli => PrefixTwin::Milli,
        Prefix::Exact(id) => PrefixTwin::Exact {
            scale: SCALES[id.0 as usize],
        },
    }
}

fn bytes<T: SerializeShrinkWrap>(value: &T) -> Vec<u8> {
    let mut buf = [0u8; 64];
    value.to_ww_bytes(&mut buf).unwrap().to_vec()
}

fn series_bytes<T: for<'i> SeriesColumns<'i>>(items: &[T]) -> Vec<u8> {
    Series::new(items).to_ww_bytes_owned().unwrap()
}

#[test]
fn bytes_are_the_wire_type() {
    for id in 0..3 {
        let r = Reading {
            value: 1000 + id as u16,
            scale: ScaleId(id),
        };
        let b = bytes(&r);
        let t = ReadingTwin {
            value: r.value,
            scale: SCALES[id as usize],
        };
        assert_eq!(b, bytes(&t));
        assert_eq!(Reading::from_ww_bytes(&b).unwrap(), r);
        assert_eq!(Reading::from_ww_bytes_owned(&b).unwrap(), r);
        assert_eq!(
            Reading::from_ww_bytes_owned(&b)
                .unwrap()
                .to_ww_bytes_owned()
                .unwrap(),
            b
        );
    }
    for p in [
        Prefix::Unit,
        Prefix::Milli,
        Prefix::Exact(ScaleId(1)),
        Prefix::Exact(ScaleId(2)),
    ] {
        let b = bytes(&p);
        assert_eq!(b, bytes(&twin(p)));
        assert_eq!(Prefix::from_ww_bytes(&b).unwrap(), p);
        assert_eq!(Prefix::from_ww_bytes_owned(&b).unwrap(), p);
    }
    // a named variant stays a nibble
    assert_eq!(bytes(&Prefix::Milli).len(), 1);
}

#[test]
fn a_value_without_a_handle_is_an_error() {
    let unknown = Scale { num: 7, den: 3 };
    let b = bytes(&PrefixTwin::Exact { scale: unknown });
    assert_eq!(Prefix::from_ww_bytes(&b), Err(Error::SubtypeOutOfRange));
    assert_eq!(
        Prefix::from_ww_bytes_owned(&b),
        Err(Error::SubtypeOutOfRange)
    );
    let b = bytes(&ReadingTwin {
        value: 1,
        scale: unknown,
    });
    assert_eq!(Reading::from_ww_bytes(&b), Err(Error::SubtypeOutOfRange));
}

#[test]
fn series_columns_are_the_wire_type() {
    let readings: Vec<Reading> = (0..20)
        .map(|k| Reading {
            value: k * 3,
            scale: ScaleId((k % 3) as u8),
        })
        .collect();
    let twins: Vec<ReadingTwin> = readings
        .iter()
        .map(|r| ReadingTwin {
            value: r.value,
            scale: SCALES[r.scale.0 as usize],
        })
        .collect();
    let b = series_bytes(&readings);
    assert_eq!(b, series_bytes(&twins));
    assert_eq!(
        SeriesOwned::<Reading>::from_ww_bytes_owned(&b).unwrap().0,
        readings
    );

    let prefixes = [
        Prefix::Milli,
        Prefix::Exact(ScaleId(1)),
        Prefix::Exact(ScaleId(1)),
        Prefix::Unit,
        Prefix::Exact(ScaleId(2)),
    ];
    let twins: Vec<PrefixTwin> = prefixes.iter().copied().map(twin).collect();
    let b = series_bytes(&prefixes);
    assert_eq!(b, series_bytes(&twins));
    assert_eq!(
        SeriesOwned::<Prefix>::from_ww_bytes_owned(&b).unwrap().0,
        prefixes
    );
}

#[test]
fn series_store_and_cells_keep_the_handle() {
    const N: usize = 6;
    let items: Vec<Prefix> = (0..N)
        .map(|k| match k % 3 {
            0 => Prefix::Milli,
            _ => Prefix::Exact(ScaleId((k % 3) as u8)),
        })
        .collect();
    let mut store = Prefix::empty_store::<N>();
    for (i, item) in items.iter().enumerate() {
        Prefix::set(&mut store, i, item);
    }
    for (i, item) in items.iter().enumerate() {
        assert_eq!(&Prefix::get(&store, i), item);
    }
    let cells = empty_cells::<Prefix, N>();
    for (i, item) in items.iter().enumerate() {
        Prefix::put::<N>(&cells, i, item);
    }
    let mut copied = Prefix::empty_store::<N>();
    Prefix::copy_rows::<N, N>(&cells, 0, &mut copied, 0, N);
    for (i, item) in items.iter().enumerate() {
        assert_eq!(&Prefix::get(&copied, i), item);
    }

    // not column by column: the caller walks the items
    let mut buf = [0u8; 256];
    let mut wr = BufWriter::new(&mut buf);
    assert_eq!(
        write_store_series::<Reading, _, N>(
            &mut wr,
            &Reading::empty_store::<N>(),
            0..N,
            &CodecPolicy::SMALLEST
        ),
        Ok(false)
    );
}
