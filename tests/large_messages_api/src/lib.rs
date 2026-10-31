//! API of the large message tests (CLT-55): a result set as one reply, as a stream function and as pages.
use wire_weaver::prelude::*;
use ww_si::{Fixed, prefix::One, units::Row};

/// How many rows to answer with.
pub type RowCount = Fixed<Row, One, u32>;

#[ww_api_root]
pub trait Archive {
    /// `rows` rows in one reply.
    fn table(rows: RowCount) -> RefVec<'i, Reading<'i>>;
    /// Same as `table`, answered later.
    fn table_later(rows: RowCount) -> RefVec<'i, Reading<'i>>;
    /// The rows of the last `table` call.
    property!(ro last_table: RefVec<'i, Reading<'i>>);
    /// `rows` rows, one item each, closed by the device after the last one.
    #[stream(Reading<'i>)]
    fn scan(rows: RowCount);
    /// Up to `limit` of `rows` rows from `cursor` on, from the start without one.
    fn page(rows: RowCount, cursor: Option<Cursor>, limit: RowCount) -> Page<'i>;
}

/// One row of a result set.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone))]
pub struct Reading<'i> {
    pub id: ReadingId,
    pub name: &'i str,
}

/// Identifies a [Reading].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy)
)]
pub struct ReadingId(pub u32);

/// A part of a result set: `next` is where the next page starts, None on the last one.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone))]
pub struct Page<'i> {
    pub rows: RefVec<'i, Reading<'i>>,
    pub next: Option<Cursor>,
}

/// Where a [Page] starts.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy)
)]
pub struct Cursor(pub u32);
