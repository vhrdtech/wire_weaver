//! The columns of a `#[wire(W)] field: T` (DER-19): `W`'s numeric column, from the plain values range-checked as
//! they are written ([Error::SubtypeOutOfRange] before anything of the group is written, never cut).
//! [WiredRef] is the column of a user wire type (DER-23).

use super::columns::numeric::{Word, write_numeric};
use super::group::{Level, SeriesColumns, SeriesCtx, TagRun};
use super::leaves::NumericLeaf;
use super::{BitSink, SeriesCells, SeriesPolicy, SeriesStore};
use crate::wire_as::{PlainSigned, PlainUnsigned};
use crate::{BufReader, Error, Nibble, UNib32, UVlq32, UVlq64, WireAs, WireAsRef};
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

/// A plain value `T` in a `Series` as the user wire type `W` of its `#[wire(W)]` field (DER-23): `W`'s columns,
/// written from the references [WireAsRef] lends, while the store and cells keep `T` (a one-byte handle stays one
/// byte per row).
#[repr(transparent)]
pub struct WiredRef<W, T>(pub T, PhantomData<fn() -> W>);

impl<W, T> WiredRef<W, T> {
    #[inline(always)]
    pub fn new(value: T) -> Self {
        WiredRef(value, PhantomData)
    }

    /// The field `value` seen as its column.
    #[inline(always)]
    pub fn from_ref(value: &T) -> &Self {
        // SAFETY: `WiredRef` is `repr(transparent)` over `T` (`PhantomData` is a ZST with alignment 1)
        unsafe { &*(value as *const T as *const Self) }
    }
}

impl<'i, W: SeriesColumns<'i> + WireAsRef<T>, T> SeriesColumns<'i> for WiredRef<W, T> {
    type Reader = W::Reader;
    const MAX_ITEM_BITS: Option<usize> = W::MAX_ITEM_BITS;
    const MAX_FIXED_BITS: Option<usize> = W::MAX_FIXED_BITS;
    const TAGGED: bool = W::TAGGED;

    fn write_group<'a, I, S, P>(
        items: I,
        sink: &mut S,
        policy: &P,
        level: Level,
    ) -> Result<(), Error>
    where
        I: Iterator<Item = (&'a Self, bool)> + Clone,
        S: BitSink,
        P: SeriesPolicy + ?Sized,
        Self: 'a,
    {
        W::write_group(
            items.map(|(v, restart)| (W::wire_ref(&v.0), restart)),
            sink,
            policy,
            level,
        )
    }

    fn skim<C: SeriesCtx>(
        rd: &mut BufReader<'i>,
        ctx: C,
        level: Level,
    ) -> Result<Self::Reader, Error> {
        W::skim(rd, ctx, level)
    }

    fn next(reader: &mut Self::Reader, restart: bool) -> Result<Self, Error> {
        Ok(WiredRef::new(W::next(reader, restart)?.into_plain()?))
    }

    fn series_tag(&self) -> Option<u32> {
        W::wire_ref(&self.0).series_tag()
    }

    fn tag_run(reader: &Self::Reader) -> Option<TagRun<'i>> {
        W::tag_run(reader)
    }
}

/// The store of `T`, written through `W`'s group (never [SeriesStore::write_store]).
impl<W, T: SeriesStore> SeriesStore for WiredRef<W, T> {
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
        WiredRef::new(T::get::<N>(store, i))
    }

    // `columnar` stays false: the rows are written item by item, through `W`'s group
}

// SAFETY: the cells of `T`, which upholds the contract
unsafe impl<W, T: SeriesCells> SeriesCells for WiredRef<W, T> {
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
