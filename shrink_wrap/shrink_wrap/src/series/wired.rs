//! The columns of a `#[wire(W)] field: T` (DER-19): `W`'s numeric column, from the plain values range-checked as
//! they are written ([Error::SubtypeOutOfRange] before anything of the group is written, never cut).
//! [WiredOpaque] is the column of a user wire type (DER-23).

use super::columns::numeric::{Word, write_numeric};
use super::columns::run::write_opaque_values;
use super::group::{Level, OpaqueReader, SeriesColumns, SeriesCtx};
use super::leaves::NumericLeaf;
use super::{BitSink, SeriesCells, SeriesPolicy, SeriesStore};
use crate::wire_as::{PlainSigned, PlainUnsigned};
use crate::{
    BufReader, DeserializeShrinkWrap, Error, Nibble, SerializeShrinkWrap, UNib32, UVlq32, UVlq64,
    WireAs,
};
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::ops::Range;

/// A plain value `T` in a `Series` as the wire type `W` of its `#[wire(W)]` field: what the derive's series code
/// names the field's column with.
#[repr(transparent)]
pub struct Wired<W, T>(pub T, PhantomData<fn() -> W>);

impl<W, T> Wired<W, T> {
    #[inline(always)]
    pub fn new(value: T) -> Self {
        Wired(value, PhantomData)
    }

    /// The field `value` seen as its column.
    #[inline(always)]
    pub fn from_ref(value: &T) -> &Self {
        // SAFETY: `Wired` is `repr(transparent)` over `T` (`PhantomData` is a ZST with alignment 1)
        unsafe { &*(value as *const T as *const Self) }
    }
}

/// Words of `values`, the range checked by the caller: a value out of range never gets here.
#[inline(always)]
fn word<W: NumericLeaf + WireAs<T>, T>(value: T) -> W::Word {
    W::from_plain(value).map_or(<W::Word as Word>::ZERO, |w| w.word())
}

macro_rules! wired {
    ($plain:ident: $($wire:ty),* $(,)?) => {
        $(
            impl<'i, T: $plain> SeriesColumns<'i> for Wired<$wire, T> {
                type Reader = <$wire as SeriesColumns<'i>>::Reader;
                const MAX_ITEM_BITS: Option<usize> = <$wire as SeriesColumns<'i>>::MAX_ITEM_BITS;
                const MAX_FIXED_BITS: Option<usize> = <$wire as SeriesColumns<'i>>::MAX_FIXED_BITS;
                const TAGGED: bool = false;

                fn write_group<'a, I, S, P>(
                    items: I,
                    sink: &mut S,
                    policy: &P,
                    _level: Level,
                ) -> Result<(), Error>
                where
                    I: Iterator<Item = (&'a Self, bool)> + Clone,
                    S: BitSink,
                    P: SeriesPolicy + ?Sized,
                    Self: 'a,
                {
                    if items.clone().next().is_none() {
                        return Ok(());
                    }
                    for (v, _) in items.clone() {
                        <$wire as WireAs<T>>::from_plain(v.0)?;
                    }
                    write_numeric(
                        sink,
                        <$wire as NumericLeaf>::KIND,
                        items.map(|(v, restart)| (word::<$wire, T>(v.0), restart)),
                        policy.numeric(),
                    )?;
                    Ok(())
                }

                fn skim<C: SeriesCtx>(
                    rd: &mut BufReader<'i>,
                    ctx: C,
                    level: Level,
                ) -> Result<Self::Reader, Error> {
                    <$wire as SeriesColumns<'i>>::skim(rd, ctx, level)
                }

                fn next(reader: &mut Self::Reader, restart: bool) -> Result<Self, Error> {
                    let wire = <$wire as SeriesColumns<'i>>::next(reader, restart)?;
                    Ok(Wired::new(<$wire as WireAs<T>>::into_plain(wire)?))
                }
            }

            /// The store of `T`, its values range-checked when the store is written.
            impl<T: $plain + SeriesStore> SeriesStore for Wired<$wire, T> {
                type Store<const N: usize> = T::Store<N>;

                fn empty_store<const N: usize>() -> Self::Store<N> {
                    T::empty_store::<N>()
                }

                fn init_store<const N: usize>(slot: &mut MaybeUninit<Self::Store<N>>) -> &mut Self::Store<N> {
                    T::init_store::<N>(slot)
                }

                #[inline(always)]
                fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
                    T::set::<N>(store, i, &item.0);
                }

                #[inline(always)]
                fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
                    Wired::new(T::get::<N>(store, i))
                }

                fn columnar<const N: usize>(_store: &Self::Store<N>, _rows: Range<usize>) -> bool {
                    true
                }

                fn write_store<S: BitSink, P: SeriesPolicy + ?Sized, const N: usize>(
                    store: &Self::Store<N>,
                    rows: Range<usize>,
                    sink: &mut S,
                    policy: &P,
                    _level: Level,
                ) -> Result<(), Error> {
                    for i in rows.clone() {
                        <$wire as WireAs<T>>::from_plain(T::get::<N>(store, i))?;
                    }
                    write_numeric(
                        sink,
                        <$wire as NumericLeaf>::KIND,
                        rows.map(|i| (word::<$wire, T>(T::get::<N>(store, i)), false)),
                        policy.numeric(),
                    )?;
                    Ok(())
                }
            }

            // SAFETY: the cells of `T`, which upholds the contract
            unsafe impl<T: $plain + SeriesCells> SeriesCells for Wired<$wire, T> {
                type Cells<const R: usize> = T::Cells<R>;

                #[inline(always)]
                fn put<const R: usize>(cells: &Self::Cells<R>, i: usize, item: &Self) {
                    T::put::<R>(cells, i, &item.0);
                }

                fn copy_rows<const R: usize, const N: usize>(
                    cells: &Self::Cells<R>,
                    from: usize,
                    store: &mut Self::Store<N>,
                    to: usize,
                    n: usize,
                ) {
                    T::copy_rows::<R, N>(cells, from, store, to, n);
                }
            }
        )*
    };
}

wired!(PlainUnsigned: u8, u16, u32, u64, Nibble, UNib32, UVlq32, UVlq64);
wired!(PlainSigned: i8, i16, i32, i64);

macro_rules! wired_un {
    ($plain:ident, $prefix:ident: $($bits:literal),*) => {
        paste::paste! {
            wired!($plain: $(crate::un::[<$prefix $bits>]),*);
        }
    };
}

wired_un!(PlainUnsigned, U: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53,
    54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64);
wired_un!(PlainSigned, I: 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54,
    55, 56, 57, 58, 59, 60, 61, 62, 63, 64);

/// A plain value `T` in a `Series` as the user wire type `W` of its `#[wire(W)]` field (DER-23): an opaque run of
/// `W`'s bytes, each changed value (by `T`'s `==`) built with [WireAs::from_plain] as it is written, the same bytes as
/// a `#[series(opaque)]` field of type `W`; the store and cells keep `T` (a one-byte handle stays one byte per row).
/// Nothing needs to lend a `&W`, so a handle can stand for values computed on the fly instead of a `static` table.
#[repr(transparent)]
pub struct WiredOpaque<W, T>(pub T, PhantomData<fn() -> W>);

impl<W, T> WiredOpaque<W, T> {
    #[inline(always)]
    pub fn new(value: T) -> Self {
        WiredOpaque(value, PhantomData)
    }

    /// The field `value` seen as its column.
    #[inline(always)]
    pub fn from_ref(value: &T) -> &Self {
        // SAFETY: `WiredOpaque` is `repr(transparent)` over `T` (`PhantomData` is a ZST with alignment 1)
        unsafe { &*(value as *const T as *const Self) }
    }
}

impl<'i, W, T> SeriesColumns<'i> for WiredOpaque<W, T>
where
    W: SerializeShrinkWrap + DeserializeShrinkWrap<'i> + WireAs<T>,
    T: Clone + PartialEq,
{
    type Reader = OpaqueReader<'i>;
    const TAGGED: bool = false;
    const MAX_ITEM_BITS: Option<usize> = super::bound::opaque_run_item_bits(W::MAX_SIZE);
    const MAX_FIXED_BITS: Option<usize> = super::bound::opaque_run_fixed_bits(W::MAX_SIZE);

    fn write_group<'a, I, S, P>(
        items: I,
        sink: &mut S,
        _policy: &P,
        _level: Level,
    ) -> Result<(), Error>
    where
        I: Iterator<Item = (&'a Self, bool)> + Clone,
        S: BitSink,
        P: SeriesPolicy + ?Sized,
        Self: 'a,
    {
        // runs restart nothing and don't care for restarts
        write_opaque_values(sink, items.map(|(v, _)| &v.0), |sink, t| {
            sink.put_serialized(&W::from_plain(t.clone())?)
        })
    }

    fn skim<C: SeriesCtx>(
        rd: &mut BufReader<'i>,
        ctx: C,
        _level: Level,
    ) -> Result<Self::Reader, Error> {
        OpaqueReader::skim(rd, &ctx, |bytes| {
            W::from_ww_bytes(bytes)?.into_plain().map(|_| ())
        })
    }

    fn next(reader: &mut Self::Reader, _restart: bool) -> Result<Self, Error> {
        Ok(WiredOpaque::new(
            W::from_ww_bytes(reader.next_bytes()?)?.into_plain()?,
        ))
    }
}

/// The store of `T`, written item by item through the opaque run (never [SeriesStore::write_store]).
impl<W, T: SeriesStore> SeriesStore for WiredOpaque<W, T> {
    type Store<const N: usize> = T::Store<N>;

    fn empty_store<const N: usize>() -> Self::Store<N> {
        T::empty_store::<N>()
    }

    fn init_store<const N: usize>(slot: &mut MaybeUninit<Self::Store<N>>) -> &mut Self::Store<N> {
        T::init_store::<N>(slot)
    }

    #[inline(always)]
    fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
        T::set::<N>(store, i, &item.0);
    }

    #[inline(always)]
    fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
        WiredOpaque::new(T::get::<N>(store, i))
    }

    // `columnar` stays false: the rows are written item by item
}

// SAFETY: the cells of `T`, which upholds the contract
unsafe impl<W, T: SeriesCells> SeriesCells for WiredOpaque<W, T> {
    type Cells<const R: usize> = T::Cells<R>;

    #[inline(always)]
    fn put<const R: usize>(cells: &Self::Cells<R>, i: usize, item: &Self) {
        T::put::<R>(cells, i, &item.0);
    }

    fn copy_rows<const R: usize, const N: usize>(
        cells: &Self::Cells<R>,
        from: usize,
        store: &mut Self::Store<N>,
        to: usize,
        n: usize,
    ) {
        T::copy_rows::<R, N>(cells, from, store, to, n);
    }
}
