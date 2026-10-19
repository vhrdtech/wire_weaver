# HANDOFF: observe-wake (SRV-25 event-driven-observe)

Design: ~/git/tpm/prj/P2401-wireweaver/design/observe_wake.md (read first).

## Done (committed on s/19oct2026-observe-wake)

- `ww_stream::wake::{Wake, Signal<W>}`: wake flag + AtomicWaker (atomic-waker crate, portable-atomic feature), changed bits. Unit tests incl. a cross-thread lost-wake test.
- `ww_device::Server::wait_until(Option<Instant>)`, `Ready::is_due()`, `Server::now()`; test `wait_until_returns_due`. (A `Server::timer(&self)` can't share a select with `wait(&mut self)`, hence `wait_until`.)
- Batches: `Consumer/Batcher::next_wake`, `Staged::next_wake`, `RowWriter::with_wake(&Wake)` + `CellsSource::wake_after` (writer wakes at the row the last poll waits for; SeqCst fences on both sides against the store-buffer race).
- Codegen: `ObserveSignal` (newtype over `Signal<W>`, `mark_changed_<name>()`), `ObserveState::take_changes(&signal)`.
- tests_common: `TestProcessEventsAsync::wakeup()` (event-driven test devices), `FakeClock` with `until(at)`.
- FEATURES SRV-25 🚧, entries.md line, CHANGELOGs (ww_stream, ww_device, wire_weaver_core).

## Next (after cherry-picking observe-shared's SRV-23/24 commits; they say SHAs via tpm work say)

1. `ObserveTable::next_wake(now, batch_waiting)` on the SRV-23 slice form (periodic grid, on-change min, beat max, start, waiting -> budget time); record the held update's bytes (`need`) at commit when `sel.is_held()`.
2. `ObserveState`: `signal: Option<&'static ObserveSignal>`, `with_signal`; `poll_properties` calls `take_changes` first; `ObserveState::next_wake(now)` = min over tables, batches (`next_wake` of consumers/staged with the level's max_age), outboxes; clamp >= now.
3. Sampler: `with_wake` on generated `<Level>Sampler`, wake on first row (filling()==1 after push) and on any seal; generated `<Level>Writer::with_wake`.
4. tests/observe: an event-driven Bench (FakeClock + wakeup = select(clock.until(next_wake), SIGNAL.wait())): no polls between due times (count polls), a change from another thread delivered at once, budget hold wakes at refill, idle -> no polls.
5. docs/api/observing.md "The server loop" (anchor used by tests/features_page/entries.md: switch it from #the-device-side), scaffold prints the signal, FEATURES ✅, Donguru sketch in the design note.

## Gotchas

- observe-shared (SRV-23/24) rewrites `ObserveTable` (slices, caller storage for plan/selection) and moves the poll loop into ww_stream (`poll_level`): don't edit those before cherry-picking.
