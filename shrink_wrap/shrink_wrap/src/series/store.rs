//! [SeriesStore]: items kept as a struct of arrays until they are written as a [Series](super::Series) (SW-62).

use super::columns::float::{write_float, write_float_slice};
use super::columns::numeric::{NumericKind, write_numeric_slice, write_numeric128};
use super::columns::run::{RunKind, write_run};
use super::group::Level;
use super::leaves::NumericLeaf;
use super::{BitSink, FloatCodec, SeriesPolicy, XorBits};
use crate::un::{IN, U3, UN};
use crate::{BufWriter, Error, Nibble, RefVec, UNib32, UVlq32, UVlq64};
use core::marker::PhantomData;
use core::ops::Range;

/// Room for `N` items of `Self` as a struct of arrays, one array per leaf field: what a device keeps while it
/// collects the items of a batch, before writing them as one [Series](super::Series) (API-41).
///
/// A struct's store is the stores of its fields, so there is no padding between fields of different sizes
/// (`Timed<Ampere>` with four `NumericValue` variants takes 12 bytes per item where the item itself takes 24); an
/// `Option<X>` is an array of flags and `X`'s store; an enum is an array of discriminants (`u8`, `u16` or `u32`,
/// whichever holds the largest) and the stores of its data variants' fields; numbers, bools and strings are arrays
/// of themselves. `#[derive_shrink_wrap(.., series)]` implements it for structs and enums whose fields all have it
/// (a type with a `#[series(opaque)]` field has none, `series(no_store)` leaves it out to write one by hand).
///
/// [set](Self::set) and [get](Self::get) panic on an index of `N` or more, like an array does. A fresh store
/// ([empty_store](Self::empty_store)) holds zeros, `false`, empty strings, `None` and an enum's first variant at
/// every index, so `get` of an index never `set` gives a value, not garbage.
///
/// ```
/// use shrink_wrap::prelude::*;
/// use shrink_wrap::series::SeriesStore;
///
/// #[derive_shrink_wrap(borrowed, series, final_structure, derive(Clone, Copy, Debug, PartialEq))]
/// struct Sample {
///     ok: bool,
///     value: i32,
/// }
///
/// let mut store = Sample::empty_store::<32>();
/// Sample::set(&mut store, 3, &Sample { ok: true, value: -7 });
/// assert_eq!(Sample::get(&store, 3), Sample { ok: true, value: -7 });
/// assert_eq!(core::mem::size_of_val(&store), 32 * 5); // 8 bytes per item as `[Sample; 32]`
/// ```
pub trait SeriesStore: Sized {
    /// The arrays of `N` items.
    type Store<const N: usize>;

    /// A store with the empty value at every index.
    fn empty_store<const N: usize>() -> Self::Store<N>;

    /// Puts `item` at index `i`.
    fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self);

    /// The item at index `i`.
    fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self;

    /// Whether [write_store](Self::write_store) can write rows `rows` (1 or more) of `store` column by column,
    /// straight from the arrays. `false` by default; numbers, floats, bools, `Option`s, tuples, arrays and derived
    /// structs and enums say `true` when every enum and `Option` in the rows keeps one variant (so no column
    /// restarts) and every leaf below can (strings, byte lists, `UN` / `IN` can't).
    fn columnar<const N: usize>(store: &Self::Store<N>, rows: Range<usize>) -> bool {
        let _ = (store, rows);
        false
    }

    /// Writes rows `rows` of `store` as [SeriesColumns::write_group](super::SeriesColumns::write_group) writes the
    /// group of those items (none restarting) at `level`: the same bits, without the per-item walk. Only when
    /// [columnar](Self::columnar) says so; a type that can't is `MalformedSeries`.
    fn write_store<S: BitSink, P: SeriesPolicy + ?Sized, const N: usize>(
        store: &Self::Store<N>,
        rows: Range<usize>,
        sink: &mut S,
        policy: &P,
        level: Level,
    ) -> Result<(), Error> {
        let _ = (store, rows, sink, policy, level);
        Err(Error::MalformedSeries)
    }
}

/// The bits of `Series::<T>::with_policy(items, policy)` for the items in rows `rows` of `store`, written column by
/// column from the arrays ([SeriesStore::write_store]) after `wr`'s position: `Ok(false)` and nothing written when
/// `T` can't write these rows that way ([SeriesStore::columnar]), for the caller to walk the items instead.
pub fn write_store_series<T: SeriesStore, P: SeriesPolicy + ?Sized, const N: usize>(
    wr: &mut BufWriter<'_>,
    store: &T::Store<N>,
    rows: Range<usize>,
    policy: &P,
) -> Result<bool, Error> {
    if rows.len() > super::SERIES_MAX_ITEMS {
        return Err(Error::LenTooLong);
    }
    if !rows.is_empty() && !T::columnar(store, rows.clone()) {
        return Ok(false);
    }
    wr.write_unib32(rows.len() as u32)?;
    if !rows.is_empty() {
        T::write_store(
            store,
            rows,
            &mut crate::buf_writer::BitWriter::new(wr),
            policy,
            Level::Outermost,
        )?;
    }
    wr.align_byte();
    Ok(true)
}

/// An unsigned integer a hand-written [SeriesStore] keeps values of another type in (`NumericValue` keeps every
/// variant in one array of the widest), for [write_numeric_column] and its siblings.
pub trait StoreWord: Copy + sealed::Sealed {
    fn bits64(self) -> u64;
    fn bits128(self) -> u128;
}

mod sealed {
    pub trait Sealed {}
}

macro_rules! store_word {
    ($($ty:ty),*) => {
        $(
            impl sealed::Sealed for $ty {}

            impl StoreWord for $ty {
                #[inline(always)]
                fn bits64(self) -> u64 {
                    self as u64
                }

                #[inline(always)]
                fn bits128(self) -> u128 {
                    self as u128
                }
            }
        )*
    };
}

store_word!(u8, u16, u32, u64, u128);

/// The numeric column of a `width`-bit (1 to 64) number kept in `values` (its bits in the low `width` bits, zero
/// or sign-extended above), as the number's own [SeriesColumns](super::SeriesColumns) writes it for these rows
/// without restarts: what a hand-written [SeriesStore::write_store] writes for a numeric field. Aligned plain is
/// one copy when `W` is exactly `width` bits wide.
pub fn write_numeric_column<W: StoreWord, S: BitSink, P: SeriesPolicy + ?Sized>(
    sink: &mut S,
    width: u8,
    signed: bool,
    values: &[W],
    policy: &P,
) -> Result<(), Error> {
    let kind = NumericKind::new(width, signed);
    let native = width as usize == 8 * core::mem::size_of::<W>();
    if width <= 32 {
        write_numeric_slice(
            sink,
            kind,
            values,
            |v| v.bits64() as u32,
            native,
            policy.numeric(),
        )?;
    } else {
        write_numeric_slice(sink, kind, values, |v| v.bits64(), native, policy.numeric())?;
    }
    Ok(())
}

/// The column of a `u128` or `i128` kept in `values`: raw plain at 128 bits, as [write_numeric_column].
pub fn write_numeric128_column<W: StoreWord, S: BitSink>(
    sink: &mut S,
    signed: bool,
    values: &[W],
) -> Result<(), Error> {
    write_numeric128(sink, signed, values.iter().map(|v| v.bits128()))
}

/// The float column of a `T` (`f32`, `f64`) kept as its bits in `values`, as [write_numeric_column]: aligned plain
/// is one copy when `W` is exactly as wide as `T`.
pub fn write_float_column<T: XorBits, W: StoreWord, S: BitSink, P: SeriesPolicy + ?Sized>(
    sink: &mut S,
    values: &[W],
    policy: &P,
) -> Result<(), Error> {
    let codec = policy.float();
    if codec == Some(FloatCodec::Aligned)
        && cfg!(target_endian = "little")
        && 8 * core::mem::size_of::<W>() == T::BITS as usize
    {
        sink.put_bits(4, FloatCodec::Aligned as u64)?;
        sink.align_byte();
        // SAFETY: `W` is an unsigned integer (sealed), plain bytes, and `u8` has no alignment
        let bytes = unsafe {
            core::slice::from_raw_parts(
                values.as_ptr().cast::<u8>(),
                core::mem::size_of_val(values),
            )
        };
        return sink.put_bytes(bytes);
    }
    write_float(
        sink,
        values.iter().map(|v| (T::from_bits(v.bits64()), false)),
        codec,
    )?;
    Ok(())
}

/// Whether `values` are all the same.
fn constant<V: PartialEq>(values: &[V]) -> bool {
    values
        .split_first()
        .is_none_or(|(first, rest)| rest.iter().all(|v| v == first))
}

/// A numeric leaf kept as an array of itself, written as a slice.
macro_rules! numeric_store {
    () => {
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
            write_numeric_slice(
                sink,
                <Self as NumericLeaf>::KIND,
                &store[rows],
                <Self as NumericLeaf>::word,
                <Self as NumericLeaf>::NATIVE,
                policy.numeric(),
            )?;
            Ok(())
        }
    };
}

macro_rules! float_store {
    () => {
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
            write_float_slice(sink, &store[rows], policy.float())?;
            Ok(())
        }
    };
}

macro_rules! numeric128_store {
    ($signed:literal) => {
        fn columnar<const N: usize>(_store: &Self::Store<N>, _rows: Range<usize>) -> bool {
            true
        }

        fn write_store<S: BitSink, P: SeriesPolicy + ?Sized, const N: usize>(
            store: &Self::Store<N>,
            rows: Range<usize>,
            sink: &mut S,
            _policy: &P,
            _level: Level,
        ) -> Result<(), Error> {
            write_numeric128(sink, $signed, store[rows].iter().map(|v| *v as u128))
        }
    };
}

macro_rules! bool_store {
    () => {
        fn columnar<const N: usize>(_store: &Self::Store<N>, _rows: Range<usize>) -> bool {
            true
        }

        fn write_store<S: BitSink, P: SeriesPolicy + ?Sized, const N: usize>(
            store: &Self::Store<N>,
            rows: Range<usize>,
            sink: &mut S,
            _policy: &P,
            _level: Level,
        ) -> Result<(), Error> {
            write_run(sink, RunKind::Bool, store[rows].iter().map(|b| *b as u32))
        }
    };
}

/// A leaf kept as an array of itself; `$kind` the store writer's macro, if any.
macro_rules! copy_leaf {
    ($($ty:ty = $empty:expr $(=> $kind:ident!($($arg:tt)*))?),* $(,)?) => {
        $(
            impl SeriesStore for $ty {
                type Store<const N: usize> = [$ty; N];

                fn empty_store<const N: usize>() -> [$ty; N] {
                    [$empty; N]
                }

                fn set<const N: usize>(store: &mut [$ty; N], i: usize, item: &Self) {
                    store[i] = *item;
                }

                fn get<const N: usize>(store: &[$ty; N], i: usize) -> Self {
                    store[i]
                }

                $($kind!($($arg)*);)?
            }
        )*
    };
}

copy_leaf!(
    u8 = 0 => numeric_store!(),
    u16 = 0 => numeric_store!(),
    u32 = 0 => numeric_store!(),
    u64 = 0 => numeric_store!(),
    u128 = 0 => numeric128_store!(false),
    i8 = 0 => numeric_store!(),
    i16 = 0 => numeric_store!(),
    i32 = 0 => numeric_store!(),
    i64 = 0 => numeric_store!(),
    i128 = 0 => numeric128_store!(true),
    f32 = 0.0 => float_store!(),
    f64 = 0.0 => float_store!(),
    bool = false => bool_store!(),
    Nibble = Nibble::new_masked(0) => numeric_store!(),
    UNib32 = UNib32(0) => numeric_store!(),
    UVlq32 = UVlq32(0) => numeric_store!(),
    UVlq64 = UVlq64(0) => numeric_store!(),
    UN = UN::UN8 {
        bit_count: U3::zero(),
        value: 0
    },
    IN = IN::IN8 {
        bit_count: U3::zero(),
        value: 0
    },
);

macro_rules! un_leaves {
    ($prefix:ident: $($bits:literal),*) => {
        paste::paste! {
            copy_leaf!($(crate::un::[<$prefix $bits>] = crate::un::[<$prefix $bits>]::zero() => numeric_store!()),*);
        }
    };
}

un_leaves!(U: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54,
    55, 56, 57, 58, 59, 60, 61, 62, 63, 64);
un_leaves!(I: 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54,
    55, 56, 57, 58, 59, 60, 61, 62, 63, 64);

impl<'i> SeriesStore for &'i str {
    type Store<const N: usize> = [&'i str; N];

    fn empty_store<const N: usize>() -> Self::Store<N> {
        [""; N]
    }

    fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
        store[i] = item;
    }

    fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
        store[i]
    }
}

impl<'i> SeriesStore for &'i [u8] {
    type Store<const N: usize> = [&'i [u8]; N];

    fn empty_store<const N: usize>() -> Self::Store<N> {
        [&[]; N]
    }

    fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
        store[i] = item;
    }

    fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
        store[i]
    }
}

impl<'i> SeriesStore for RefVec<'i, u8> {
    type Store<const N: usize> = [RefVec<'i, u8>; N];

    fn empty_store<const N: usize>() -> Self::Store<N> {
        [RefVec::new_bytes(&[]); N]
    }

    fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
        store[i] = *item;
    }

    fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
        store[i]
    }
}

/// Owned leaves: an array of values, cloned in and out.
#[cfg(feature = "std")]
macro_rules! clone_leaf {
    ($($ty:ty),*) => {
        $(
            impl SeriesStore for $ty {
                type Store<const N: usize> = [$ty; N];

                fn empty_store<const N: usize>() -> Self::Store<N> {
                    core::array::from_fn(|_| <$ty>::new())
                }

                fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
                    store[i].clone_from(item);
                }

                fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
                    store[i].clone()
                }
            }
        )*
    };
}

#[cfg(feature = "std")]
clone_leaf!(String, Vec<u8>);

/// No arrays: a `PhantomData` field takes no space.
impl<T: ?Sized> SeriesStore for PhantomData<T> {
    type Store<const N: usize> = ();

    fn empty_store<const N: usize>() {}

    fn set<const N: usize>(_store: &mut (), _i: usize, _item: &Self) {}

    fn get<const N: usize>(_store: &(), _i: usize) -> Self {
        PhantomData
    }

    fn columnar<const N: usize>(_store: &(), _rows: Range<usize>) -> bool {
        true
    }

    fn write_store<S: BitSink, P: SeriesPolicy + ?Sized, const N: usize>(
        _store: &(),
        _rows: Range<usize>,
        _sink: &mut S,
        _policy: &P,
        _level: Level,
    ) -> Result<(), Error> {
        Ok(())
    }
}

/// The flags, and `X`'s store for the `Some` items (left as it was at a `None`).
impl<X: SeriesStore> SeriesStore for Option<X> {
    type Store<const N: usize> = ([bool; N], X::Store<N>);

    fn empty_store<const N: usize>() -> Self::Store<N> {
        ([false; N], X::empty_store::<N>())
    }

    fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
        store.0[i] = item.is_some();
        if let Some(x) = item {
            X::set::<N>(&mut store.1, i, x);
        }
    }

    fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
        store.0[i].then(|| X::get::<N>(&store.1, i))
    }

    /// All `Some` with `X` columnar, or all `None`.
    fn columnar<const N: usize>(store: &Self::Store<N>, rows: Range<usize>) -> bool {
        let flags = &store.0[rows.clone()];
        constant(flags) && (!flags[0] || X::columnar::<N>(&store.1, rows))
    }

    fn write_store<S: BitSink, P: SeriesPolicy + ?Sized, const N: usize>(
        store: &Self::Store<N>,
        rows: Range<usize>,
        sink: &mut S,
        policy: &P,
        _level: Level,
    ) -> Result<(), Error> {
        let some = store.0[rows.start];
        write_run(
            sink,
            RunKind::OptionTag,
            core::iter::repeat_n(some as u32, rows.len()),
        )?;
        if some {
            X::write_store::<S, P, N>(&store.1, rows, sink, policy, Level::Nested)?;
        }
        Ok(())
    }
}

/// One store per element.
impl<X: SeriesStore, const M: usize> SeriesStore for [X; M] {
    type Store<const N: usize> = [X::Store<N>; M];

    fn empty_store<const N: usize>() -> Self::Store<N> {
        core::array::from_fn(|_| X::empty_store::<N>())
    }

    fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
        for (s, x) in store.iter_mut().zip(item) {
            X::set::<N>(s, i, x);
        }
    }

    fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
        core::array::from_fn(|j| X::get::<N>(&store[j], i))
    }

    fn columnar<const N: usize>(store: &Self::Store<N>, rows: Range<usize>) -> bool {
        store.iter().all(|s| X::columnar::<N>(s, rows.clone()))
    }

    fn write_store<S: BitSink, P: SeriesPolicy + ?Sized, const N: usize>(
        store: &Self::Store<N>,
        rows: Range<usize>,
        sink: &mut S,
        policy: &P,
        _level: Level,
    ) -> Result<(), Error> {
        for s in store {
            X::write_store::<S, P, N>(s, rows.clone(), sink, policy, Level::Nested)?;
        }
        Ok(())
    }
}

macro_rules! tuple {
    ($($t:ident $idx:tt),+) => {
        /// One store per element.
        impl<$($t: SeriesStore),+> SeriesStore for ($($t,)+) {
            type Store<const N: usize> = ($($t::Store<N>,)+);

            fn empty_store<const N: usize>() -> Self::Store<N> {
                ($($t::empty_store::<N>(),)+)
            }

            fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
                $($t::set::<N>(&mut store.$idx, i, &item.$idx);)+
            }

            fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
                ($($t::get::<N>(&store.$idx, i),)+)
            }

            fn columnar<const N: usize>(store: &Self::Store<N>, rows: Range<usize>) -> bool {
                true $(&& $t::columnar::<N>(&store.$idx, rows.clone()))+
            }

            fn write_store<S: BitSink, P: SeriesPolicy + ?Sized, const N: usize>(
                store: &Self::Store<N>,
                rows: Range<usize>,
                sink: &mut S,
                policy: &P,
                _level: Level,
            ) -> Result<(), Error> {
                $($t::write_store::<S, P, N>(&store.$idx, rows.clone(), sink, policy, Level::Nested)?;)+
                Ok(())
            }
        }
    };
}

tuple!(T0 0);
tuple!(T0 0, T1 1);
tuple!(T0 0, T1 1, T2 2);
tuple!(T0 0, T1 1, T2 2, T3 3);
tuple!(T0 0, T1 1, T2 2, T3 3, T4 4);
tuple!(T0 0, T1 1, T2 2, T3 3, T4 4, T5 5);
tuple!(T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6);
tuple!(T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6, T7 7);
