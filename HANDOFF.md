# HANDOFF: observe-wake (SRV-25 event-driven-observe)

Design: ~/git/tpm/prj/P2401-wireweaver/design/observe_wake.md (incl. the Donguru sketch for the parent).

## Done (committed on s/19oct2026-observe-wake)

- SRV-23 (observe-shared 96ce73ca) cherry-picked as ee90b8dd.
- ww_stream: `wake::{Wake, Signal<W>}`; `ObserveTable::next_wake`, `hold`, held `need` kept at commit, `Plan::missed_need`; batches `Consumer/Batcher/Staged::next_wake`, `RowWriter::with_wake`.
- ww_device: `Server::wait_until`, `Ready::is_due`, `Server::now`.
- Codegen: `ObserveSignal`, `ObserveState::{take_changes, with_signal, signal, next_wake}`, `<Level>Sampler/Writer::with_wake`, `take_changes` at the top of `poll_properties`, `hold(missed_need)` where the loop breaks for a missed count waiting for budget; scaffold prints `static OBSERVE`.
- Tests: ww_stream `next_wake_sends_what_polling_does` (+ idle), tests/observe `event_driven_*` (stress 30x under load ok), tests/observe_batched `batches_wake_and_next_wake`.
- docs/api/observing.md "The server loop"; FEATURES SRV-25 ✅; CHANGELOGs. just test / lint / check green.

## Next

1. Cherry-pick observe-shared's SRV-24 (shared poll loop in ww_stream). Conflicts expected in the generated `poll_properties`: carry over (a) `take_changes` from the signal at the top, (b) the hold: SRV-24's `ww_stream::observe::select` returns `Ok(None)` when `plan.is_empty() && !report_missed`; make it say the bytes it waits for and have the loop call `self.observe_state().level(level).0.hold(..)`. Re-run `next_wake_sends_what_polling_does`, tests/observe, observe_batched, then just test/lint/check.
2. `just build-docs` not run (asks the user first, AGENTS.md).
3. `git rm HANDOFF.md`, `tpm work ready`.
