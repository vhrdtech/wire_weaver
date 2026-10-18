//! [LevelRow]: the samples of one trait level's `+batched` properties (API-39) with one shared clock. A row is the
//! device time of one `sample()` call and every property's value; a [Batcher](super::Batcher) of rows keeps them
//! as a struct of arrays, and a batch is sent as one `Series` per column: the time column once, then the column of
//! each property observed in `Samples` mode. Generated per level by codegen.

use super::batcher::Bound;
use super::{Encoded, Scratch};
use core::ops::Range;
use wire_weaver::shrink_wrap::Error;
use wire_weaver::shrink_wrap::series::{SeriesColumns, SeriesPolicy, SeriesStore};

/// A set of value columns of a [LevelRow], bit `c` for column `c`: at most 64 columns.
pub type Columns = u64;

/// Most value columns a [LevelRow] can have.
pub const MAX_COLUMNS: usize = Columns::BITS as usize;

/// Worst case of one column as its own `Series` of `C`, in bits: the fixed part and the padding to the next byte,
/// and each item. A `C` without a bound (a string, an opaque field) is a compile error where it is evaluated.
pub const fn column_bits<C: for<'i> SeriesColumns<'i>>() -> (usize, usize) {
    let fixed = match C::MAX_FIXED_BITS {
        Some(bits) => bits,
        None => panic!("a batched type needs a series size bound (no strings or opaque fields)"),
    };
    let item = match C::MAX_ITEM_BITS {
        Some(bits) => bits,
        None => panic!("a batched type needs a series size bound (no strings or opaque fields)"),
    };
    (fixed + 7, item)
}

/// The samples of one trait level: the time (column `None`) and the values (columns `0..COLUMNS`), each column a
/// `Series` of its own on the wire. Implemented by generated code for a `<Trait>Row`.
pub trait LevelRow: SeriesStore {
    /// Value columns, at most [MAX_COLUMNS].
    const COLUMNS: usize;
    /// [column_bits] of the time column.
    const TIME_BITS: (usize, usize);
    /// [column_bits] of each value column.
    const COLUMN_BITS: &'static [(usize, usize)];

    /// Encodes column `column` (`None`: the time) of `rows` of `store` as a `Series` at the start of `buf`, or only
    /// counts its bytes without `buf`, rebuilding the column's values in `scratch` (see
    /// [Scratch::encode_column]). Returns the bytes.
    fn encode_column<P: SeriesPolicy + Copy, const N: usize, const B: usize>(
        store: &Self::Store<N>,
        rows: Range<usize>,
        column: Option<usize>,
        policy: P,
        scratch: &mut Scratch<B>,
        buf: Option<&mut [u8]>,
    ) -> Result<usize, Error>;

    /// Worst case of a batch: the time column and the value columns in `columns`.
    #[doc(hidden)]
    fn bound(columns: Columns) -> Bound {
        let mut bound = Bound {
            fixed: Self::TIME_BITS.0,
            item: Self::TIME_BITS.1,
            series: 1,
        };
        for (c, &(fixed, item)) in Self::COLUMN_BITS.iter().enumerate() {
            if c < MAX_COLUMNS && columns & (1 << c) != 0 {
                bound.fixed += fixed;
                bound.item += item;
                bound.series += 1;
            }
        }
        bound
    }

    /// Most bytes `n` rows take with the value columns in `columns`, the time column included: every column
    /// written as its own `Series`.
    fn max_bytes(n: usize, columns: Columns) -> usize {
        let bound = Self::bound(columns);
        let n_bits = wire_weaver::shrink_wrap::series::bound::nib_bits(n as u32);
        (bound.fixed + bound.series * n_bits + n * bound.item).div_ceil(8)
    }
}

/// What [Consumer::encode_rows](super::Consumer::encode_rows) did: the rows encoded, as for a `Series`, and what
/// the encoder made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowsEncoded<O> {
    pub encoded: Encoded,
    pub out: O,
}

impl<const BYTES: usize> Scratch<BYTES> {
    /// Rows `rows` of one column of a [LevelRow] store, `store` (a column's own store, as [SeriesStore] makes it),
    /// encoded with `policy` as a `Series<C>` at the start of `buf`, or only counted without `buf`. Returns the
    /// bytes. Written straight from the store's arrays when it can ([SeriesStore::columnar]: no tag changes in the
    /// rows), else rebuilt item by item into the scratch memory first. What generated [LevelRow::encode_column]s
    /// call.
    pub fn encode_column<C, P, const N: usize>(
        &mut self,
        store: &C::Store<N>,
        rows: Range<usize>,
        policy: P,
        buf: Option<&mut [u8]>,
    ) -> Result<usize, Error>
    where
        C: SeriesStore + for<'i> SeriesColumns<'i>,
        P: SeriesPolicy + Copy,
    {
        let buf = match buf {
            Some(buf) => {
                let mut wr = wire_weaver::shrink_wrap::BufWriter::new(buf);
                if wire_weaver::shrink_wrap::series::write_store_series::<C, P, N>(
                    &mut wr,
                    store,
                    rows.clone(),
                    &policy,
                )? {
                    return Ok(wr.finish_and_take()?.len());
                }
                Some(wr.deinit())
            }
            None => None,
        };
        let filled = self.fill::<C, N>(store, rows);
        let series = wire_weaver::shrink_wrap::series::Series::with_policy(filled.items(), policy);
        match buf {
            Some(buf) => wire_weaver::shrink_wrap::SerializeShrinkWrap::to_ww_bytes(&series, buf)
                .map(|bytes| bytes.len()),
            None => series.encoded_len(),
        }
    }
}
