//! Observe cells: the ring, its writer and reader, the staging batch, against the default path's batcher. The
//! threaded test also runs under miri (`just miri`).

use super::*;
use wire_weaver::shrink_wrap::series::{CodecPolicy, SeriesCells, SeriesPolicy, SeriesStore};
use ww_si::{Prefix, Second};

/// A level's row: the time and two value columns.
type Row = (Second, i32, u32);

impl LevelRow for Row {
    const COLUMNS: usize = 2;
    const TIME_BITS: (usize, usize) = column_bits::<Second>();
    const COLUMN_BITS: &'static [(usize, usize)] = &[column_bits::<i32>(), column_bits::<u32>()];

    fn encode_column<P: SeriesPolicy + Copy, const N: usize, const B: usize>(
        store: &<Row as SeriesStore>::Store<N>,
        rows: core::ops::Range<usize>,
        column: Option<usize>,
        policy: P,
        scratch: &mut Scratch<B>,
        buf: Option<&mut [u8]>,
    ) -> Result<usize, wire_weaver::shrink_wrap::Error> {
        match column {
            None => scratch.encode_column::<Second, P, N>(&store.0, rows, policy, buf),
            Some(0) => scratch.encode_column::<i32, P, N>(&store.1, rows, policy, buf),
            Some(1) => scratch.encode_column::<u32, P, N>(&store.2, rows, policy, buf),
            Some(_) => Err(wire_weaver::shrink_wrap::Error::OutOfBoundsWriteU8),
        }
    }
}

impl LevelRowCells for Row {
    fn put_time<const R: usize>(cells: &<Row as SeriesCells>::Cells<R>, i: usize, t: &Second) {
        Second::put::<R>(&cells.0, i, t);
    }
}

const SLOTS: [u16; 2] = [3, 7];
type Scr = Scratch<{ scratch_bytes::<Second>(16) }>;

/// The values of row `k`: each a function of the time, so a mixed row shows.
fn values(k: u32) -> (i32, u32) {
    ((k as i32).wrapping_mul(-37) + 5, k.wrapping_mul(2654435761))
}

fn time(k: u32) -> Second {
    Second::micro(k * 40)
}

fn us(t: &Second) -> u32 {
    t.to_i128_in(Prefix::Micro).unwrap() as u32
}

fn write<const R: usize>(w: &mut RowWriter<'_, Row, R>, k: u32) -> bool {
    let (a, b) = values(k);
    let slot = w.slot();
    i32::put::<R>(&w.rows().1, slot, &a);
    w.mark(0);
    u32::put::<R>(&w.rows().2, slot, &b);
    w.mark(1);
    w.commit(time(k))
}

/// Every row in `store[..n]` is consistent: its values are those of its time.
fn check_rows<const N: usize>(store: &<Row as SeriesStore>::Store<N>, n: usize) -> Vec<u32> {
    (0..n)
        .map(|i| {
            let (t, a, b) = Row::get::<N>(store, i);
            let k = us(&t) / 40;
            assert_eq!((a, b), values(k), "row {i} at {k} is mixed");
            k
        })
        .collect()
}

#[test]
fn claims_once_and_gives_back() {
    let cells = RowCells::<Row, 4>::new();
    let w = cells.writer().unwrap();
    assert!(cells.writer().is_none());
    drop(w);
    assert!(cells.writer().is_some());
    let r = cells.reader::<8>().unwrap();
    assert!(cells.reader::<8>().is_none());
    drop(r);
    assert!(cells.reader::<8>().is_some());
}

#[test]
fn gate_closed_takes_nothing() {
    let cells = RowCells::<Row, 4>::new();
    let mut w = cells.writer().unwrap();
    assert!(!w.is_sampling());
    assert!(!write(&mut w, 1));
    assert_eq!(cells.committed(), 0);
    cells.gate().set(Some(0));
    assert!(w.is_sampling());
    assert!(write(&mut w, 2));
    assert_eq!(cells.committed(), 1);
}

#[test]
fn decimates_on_the_grid() {
    let cells = RowCells::<Row, 64>::new();
    cells.gate().set(Some(400));
    let mut w = cells.writer().unwrap();
    let taken: Vec<u32> = (0..50).filter(|&k| write(&mut w, k)).collect();
    assert_eq!(taken, vec![0, 10, 20, 30, 40]);
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "wasn't set before commit")]
fn a_value_not_set_panics_in_debug() {
    let cells = RowCells::<Row, 4>::new();
    cells.gate().set(Some(0));
    let mut w = cells.writer().unwrap();
    let slot = w.slot();
    i32::put::<4>(&w.rows().1, slot, &1);
    w.mark(0);
    w.commit(time(0));
}

#[test]
fn reads_what_was_written_across_the_wrap() {
    let cells = RowCells::<Row, 5>::new();
    cells.gate().set(Some(0));
    let mut w = cells.writer().unwrap();
    let src: &dyn CellsSource<Row, 16> = &cells;
    let mut store = Row::empty_store::<16>();
    let mut next = 0;
    let mut seen = vec![];
    for round in 0..7 {
        for _ in 0..(round % 4 + 1) {
            write(&mut w, next);
            next += 1;
        }
        let read = src.read_into(&mut store, 0, 16);
        assert_eq!(read.lapped, 0);
        seen.extend(check_rows::<16>(&store, read.rows));
    }
    assert_eq!(seen, (0..next).collect::<Vec<_>>());
}

#[test]
fn lapped_rows_are_counted_not_sent() {
    let cells = RowCells::<Row, 8>::new();
    cells.gate().set(Some(0));
    let mut w = cells.writer().unwrap();
    for k in 0..20 {
        write(&mut w, k);
    }
    let src: &dyn CellsSource<Row, 16> = &cells;
    let mut store = Row::empty_store::<16>();
    let read = src.read_into(&mut store, 0, 16);
    // R - 1 rows can be read, the 13 older ones were written over
    assert_eq!((read.rows, read.lapped), (7, 13));
    assert_eq!(check_rows::<16>(&store, 7), (13..20).collect::<Vec<_>>());
    // a smaller `max` leaves the rest for later
    for k in 20..25 {
        write(&mut w, k);
    }
    let read = src.read_into(&mut store, 0, 2);
    assert_eq!((read.rows, read.lapped), (2, 0));
    assert_eq!(src.waiting(), 3);
    assert_eq!(src.skip_all(), 3);
    assert_eq!(src.waiting(), 0);
}

#[test]
fn the_row_count_wraps() {
    let cells = RowCells::<Row, 3>::new();
    cells.start_at(u32::MAX - 4);
    cells.gate().set(Some(0));
    let mut w = cells.writer().unwrap();
    let src: &dyn CellsSource<Row, 4> = &cells;
    let mut store = Row::empty_store::<4>();
    let mut seen = vec![];
    for k in 0..12 {
        write(&mut w, k);
        let read = src.read_into(&mut store, 0, 4);
        assert_eq!(read.lapped, 0);
        seen.extend(check_rows::<4>(&store, read.rows));
    }
    assert_eq!(seen, (0..12).collect::<Vec<_>>());
    assert_eq!(cells.committed(), 7);
}

/// The same rows through the default path (a [Batcher] of rows) and through cells, sealed at the same rows,
/// give the same outbox bytes.
#[test]
fn the_same_bytes_as_the_batcher() {
    const N: usize = 8;
    let policy = CodecPolicy::SMALLEST;
    let cells = RowCells::<Row, 16>::new();
    cells.gate().set(Some(0));
    let mut w = cells.writer().unwrap();
    let mut staged = Staged::<Row, N, _>::new(cells.reader().unwrap(), policy);
    let mut batcher = Batcher::<Row, N, _>::new_rows(policy);
    let mut scratch = Scr::new();
    let (mut a, mut b) = (Outbox::<4096>::new(), Outbox::<4096>::new());
    let mut now = 0;
    for k in 0..37u32 {
        write(&mut w, k);
        let (x, y) = values(k);
        batcher.push(&(time(k), x, y));
        // the batcher seals at `N` rows by itself, the cells when polled: poll at every `N`
        if k as usize % N == N - 1 {
            now += 1000;
            let columns = [0b11, 0b01, 0b10][k as usize / N % 3];
            batcher.consumer().poll_rows(now, Some(3000), 4096, columns);
            staged.poll_cells(now, Some(3000), 4096, columns);
            a.push_rows_from(&mut batcher.consumer(), columns, &SLOTS, &mut scratch)
                .unwrap();
            b.push_rows_from(&mut staged, columns, &SLOTS, &mut scratch)
                .unwrap();
        }
    }
    batcher.consumer().flush();
    staged.flush();
    a.push_rows_from(&mut batcher.consumer(), 0b11, &SLOTS, &mut scratch)
        .unwrap();
    b.push_rows_from(&mut staged, 0b11, &SLOTS, &mut scratch)
        .unwrap();
    assert!(a.len() > 3);
    let mut rows = 0;
    while let (Some(x), Some(y)) = (a.oldest(), b.oldest()) {
        assert_eq!(x.rows, y.rows);
        assert_eq!(x.time, y.time);
        let xs: Vec<_> = x
            .columns()
            .map(|c| (c.k, c.items, c.bytes.to_vec()))
            .collect();
        let ys: Vec<_> = y
            .columns()
            .map(|c| (c.k, c.items, c.bytes.to_vec()))
            .collect();
        assert_eq!(xs, ys);
        rows += x.rows as u32;
        a.remove_oldest();
        b.remove_oldest();
    }
    assert!(a.is_empty() && b.is_empty());
    assert_eq!(rows, 37);
}

#[test]
fn staged_seals_full_budget_age_and_drains_bursts() {
    const N: usize = 8;
    let cells = RowCells::<Row, 64>::new();
    cells.gate().set(Some(0));
    let mut w = cells.writer().unwrap();
    let mut staged = Staged::<Row, N, _>::new(cells.reader().unwrap(), CodecPolicy::SMALLEST);
    let mut scratch = Scr::new();
    let mut outbox = Outbox::<8192>::new();
    // a burst of 40 rows: 5 full batches, drained call by call
    for k in 0..40 {
        write(&mut w, k);
    }
    let mut batches = 0;
    while staged.waiting() > 0 || staged.rows() > 0 {
        let polled = staged.poll_cells(0, None, 4096, 0b01);
        assert_eq!(polled.sealed, Some(Cause::Full));
        let done = outbox
            .push_rows_from(&mut staged, 0b01, &SLOTS, &mut scratch)
            .unwrap();
        assert_eq!((done.batches, done.rows), (1, N));
        batches += 1;
    }
    assert_eq!(batches, 5);
    // a small budget seals early
    for k in 40..48 {
        write(&mut w, k);
    }
    let polled = staged.poll_cells(0, None, 20, 0b11);
    assert_eq!(polled.sealed, Some(Cause::Budget));
    assert!(staged.rows() < N);
    staged.discard_all();
    assert_eq!((staged.rows(), staged.waiting()), (0, 0));
    // the age, from the first poll that saw the first row
    write(&mut w, 50);
    assert_eq!(staged.poll_cells(100, Some(500), 4096, 0b11).sealed, None);
    write(&mut w, 51);
    assert_eq!(staged.poll_cells(599, Some(500), 4096, 0b11).sealed, None);
    assert_eq!(
        staged.poll_cells(600, Some(500), 4096, 0b11).sealed,
        Some(Cause::Age)
    );
    assert_eq!(staged.rows(), 2);
}

/// A writer thread and a reader thread: every row read is consistent, and every row committed is read or counted
/// as lapped, once.
#[test]
fn threaded_writer_and_reader() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    const R: usize = 4;
    const N: usize = 3;
    let rows: u32 = if cfg!(miri) { 300 } else { 200_000 };
    let cells = Arc::new(RowCells::<Row, R>::new());
    cells.gate().set(Some(0));
    let done = Arc::new(AtomicBool::new(false));
    let writer = {
        let cells = cells.clone();
        let done = done.clone();
        std::thread::spawn(move || {
            let mut w = cells.writer().unwrap();
            for k in 0..rows {
                assert!(write(&mut w, k));
                if k % 7 == 0 {
                    std::thread::yield_now();
                }
            }
            done.store(true, Ordering::Release);
        })
    };
    let reader = {
        let cells = cells.clone();
        std::thread::spawn(move || {
            let src: &dyn CellsSource<Row, N> = &*cells;
            let mut store = Row::empty_store::<N>();
            let (mut read, mut lapped, mut last) = (0u64, 0u64, None::<u32>);
            loop {
                let finished = done.load(Ordering::Acquire);
                let r = src.read_into(&mut store, 0, N);
                for k in check_rows::<N>(&store, r.rows) {
                    assert!(
                        last.is_none_or(|l| k > l),
                        "out of order: {k} after {last:?}"
                    );
                    last = Some(k);
                }
                read += r.rows as u64;
                lapped += r.lapped;
                if finished && src.waiting() == 0 {
                    break;
                }
                if r.rows == 0 {
                    std::thread::yield_now();
                }
            }
            (read, lapped)
        })
    };
    writer.join().unwrap();
    let (read, lapped) = reader.join().unwrap();
    assert_eq!(read + lapped, rows as u64, "read {read}, lapped {lapped}");
    assert!(read > 0);
}
