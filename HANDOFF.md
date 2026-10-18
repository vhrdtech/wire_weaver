# Handoff: batching-docs (API-41/API-39 piece 8)

Session batching-docs, branch `s/18oct2026-batching-docs`, two worktrees:
- wire_weaver: `/home/roman/git/.wt/batching-docs/wire_weaver` (this one)
- motor_sim: `/home/roman/git/.wt/batching-docs/motor_sim` (separate repo, tpm set up a sibling worktree for it
  since piece 8 touches both; its `Cargo.toml` path deps `../wire_weaver/...` resolve to this wire_weaver worktree
  correctly — don't touch `/home/roman/git/motor_sim` or `/home/roman/git/wire_weaver`, those are the user's main
  checkouts, left alone).

Plan: `~/git/tpm/prj/P2401-wireweaver/design/batched_streams.md`, piece 8 ("Docs, motor demo, FEATURES").

## Done (committed)

- **motor_sim** `39397e2` (piece 1): `motor_demo` builds again against master's API-39 — `device::LIMITS` gets
  `samples_max_age_us`/`samples_min_every_us`, `args::mode`/`mode_str` gain `samples:20ms` /
  `samples:1ms/20ms`. Reported to parent (`batched-streams`) already so it can land early.
- **motor_sim** `1dc00d7`: `motor_demo_api`'s every `Foc` property and `Control`'s 13 measured values (not its 5
  setpoints) are `+observe+batched`, matching `design/motor_demo_api_si.rs`'s sketch exactly. `device.rs`:
  `tier!` macro split into two arms (`tier_common!` shared, one arm plain, one `sample(sampler, Samples)` —
  macro_rules can't nest a field's `$(...)* ` repetition inside an optional `$(...)?` for an unrelated
  variable) so `mark_foc`/`mark_control` also feed `FocSamples`/`ControlSamples` to
  `self.observe.samplers().root_foc`/`root_control` once per tier; `ObserveState::new(LIMITS,
  ObserveBatches::new(DEFAULT_POLICY))`; `poll_properties` takes a `&mut Scratch<SCRATCH_BYTES>` now (added a
  `let mut scratch` next to `buf` in `run()`). `stats.rs`: `levels!` macro has `plain:`/`batched:` sections per
  level (Foc all batched, Control split 5 plain + 13 batched, Telemetry all plain); `present_batched()` expands
  a `BatchedOwned::Series` into one `Present` per sample (oldest first) so `Stats::record`'s per-property count
  and cost follow every sample; `Carried.data` is now `u.to_ww_bytes_owned()` directly (the client's `FocUpdate`
  etc. are already the owned type) instead of rebuilding the device-side struct through a `MAX_MESSAGE_LEN`
  buffer. New test `samples_mode_gets_every_sample` (observes `i_q` in `Samples` mode, checks ~every sample
  arrives in far fewer updates) — note it can't use the shared `run()` helper's sim-span wait (Samples-mode
  updates carry no head `t`), so it also observes `telemetry.time` for the span, or (in this specific test) just
  sleeps a real window since `time_scale` is 1. `each_tier_at_its_rate`'s `control.bytes < 0.6 *
  stream_data_bytes` assertion loosened to `0.75`: every `+batched` property now pays a `Batched::One` tag even
  when observed unbatched (confirmed, not a bug). `just test`/`just lint` clean.
- **wire_weaver** `ead0922b`: FEATURES.md API-41 and API-39 flipped to ✅, noting the motor demo and the (not yet
  written) `docs/api/batching.md`.

## Measured (not committed — was a scratch example, deleted per
`design/observe_batching_measure.md`'s own precedent of "scratch, not to land")

`Options::default()` (`VescOutrunner`, `SpeedSteps`, `Load::Fan{k:2e-7}`, seed 1), `CodecPolicy::SMALLEST`
(`ww_stream::batch::DEFAULT_POLICY` on `std`), `n = 32` (`OBSERVE_BATCH` default), one level observed per device
session at its own `LIMITS.max_level_budget` (4 000 000 B/s) so budget never throttles what's measured:

| | foc (21 props) | control (13 measured) |
|---|---|---|
| today, periodic every sample (I32 quantities, unbatched) | **3.73 MB/s** | **99.0 kB/s** |
| batched, `Samples` mode, n=32 | **995 kB/s** | **26.2 kB/s** |
| gain | **3.7x** | **3.8x** |

Compare to `design/observe_batching_measure.md`'s predictions: FOC batched 913.1 kB/s (measured 995, +9%),
Control batched 19.9 kB/s (measured 26.2, +32%). **Not a bug** — reasoned through carefully, see the commit
message of motor_sim `1dc00d7` and below:
- The design's numbers were a **planning-stage prediction** from a hand-written `SeriesColumns` mirror of
  `Timed<Quantity>` ("no derive yet" — piece 1/SW-62's real derive landed after), so some drift from the real
  generated code is expected; FOC (large, noisy values) drifts less (+9%) than Control (small, precise values,
  +32%) because fixed per-item/per-update overhead (tags, headers) is a bigger fraction of a smaller value.
- The design's **"today" baseline measured raw `f32`** (pre-STD-38, before motor_demo_api's `ww_si` quantity
  migration): 2400.0 / 66.0 kB/s. Today's `motor_demo_api` already uses `ww_si` quantities even when unbatched
  (quantization is baked into the property's type, not toggled by observe mode), so my "today" numbers
  (3.73 MB/s / 99.0 kB/s) are **not the same baseline** — they already pay the quantity self-description tax
  (prefix + `NumericValue` tags) with **no series/delta compression to amortize it** (a single unbatched value
  is its own event, no sequence to predict from). That's why "today" is higher than the design's f32 number, and
  it's also why the *batching* win (3.7-3.8x, measured against this already-upgraded baseline) is bigger than
  the design's predicted win (913.1/2400=2.6x, 19.9/66=3.3x) — delta compression across a `Series` claws back
  the self-description tax that an unbatched quantity can't.
- A second, smaller effect on Control specifically: its batches don't seal in lockstep across all 13 properties
  (measured 238 updates in a window where FOC's 21 properties sealed almost exactly on the idealized count) —
  worth a line in the docs citing `design/batched_streams.md`'s idea **I-5** ("align observe batch ages within a
  level"), not yet implemented.
- Reproduction recipe (not committed, rebuild from scratch if needed): `motor_demo/examples/scratch_measure_batched.rs`
  — one `measure(name, level, mode, carries_t)` helper per scenario, each its own `device::spawn` session so
  budgets don't collide; observe `telemetry.time` (`on_change`) alongside a `Samples`-mode level to get a real
  sim-time span, since `Samples` updates carry no head `t`.

## Still to do (piece 8)

1. **`docs/api/batching.md`** (new page, `wire_weaver/docs/`): level `basic` section — the how-to (`+batched` on
   `property!`, `samples(max_age)` / `samples_every(every, max_age)` client helpers,
   `self.observe.samplers().<level>.sample(t, <Level>Samples{..})` on the device, default batch 32 / outbox 4096,
   `Batched::One` vs `::Series` on the wire, no head `t` for an all-`Series` update). `advanced` section: sealing
   (size/age/budget causes), budgets, the double-buffered `Batcher`, the `SeriesStore`/`Series` encode. The motor
   demo numbers table above (what it gains, the flip side — unbatched already pays the quantity tax, Control's
   batches not sealing in lockstep yet, point at I-5). The Cortex-M CPU table from `design/batched_streams.md`'s
   piece 4 + the "After SW-63" section (instruction/cycle counts, `DeltaPacked` as the `no_std` default, "which
   rates fit which MCU" — full-rate FOC isn't realistic on M3/M4/M4F, "FOC subset 5" is, shared clock (I-2) is a
   building idea by another session (**shared-clock**) — leave one line pointing at it, don't describe its
   design (I already told that session we're not touching `docs/api/observing.md` or the `Batched<T>` wire shape
   it's reworking; if its change lands before this one, the "no head `t`" / `Batched::Series(Series<Timed<T>>)`
   wording here may need a follow-up edit — check `ww_stream::observe::Batched`'s current shape before writing
   the wire-level parts).
2. Nav entry in `zensical.toml` for the new page; give it a `level:` per `docs/levels.md`'s front matter
   convention (basic, probably, matching the SerDes section's pattern of Basic-then-Advanced subsections in one
   nav group — check if `docs/api/batching.md` should be one page with two `## ` sections, or two pages; the
   SerDes precedent docs/levels.md describes is one nav group with Basic/Advanced subsections, likely the
   simpler route here is one page, badge at the top, an `## Advanced` heading partway through — check
   `docs/levels.md` for the exact convention before deciding).
3. `tests/features_page/entries.md`: add lines for **API-41** and **API-39** (reading level, one short line on
   what it gives a user, a docs link to `docs/api/batching.md`) — the test names what's missing if skipped.
4. `just features-page` (regenerates `docs/features_today.md`).
5. `just build-docs` — **ask the user first if `just serve-docs` might be running** (AGENTS.md's instruction;
   running `build-docs` while another instance has `serve-docs` open breaks that instance). Fix any
   `page does not exist` / `anchor does not exist` warnings it reports for the new page's links.
6. `just test` in wire_weaver (haven't run it this session; no `.rs` changed here, only `FEATURES.md`, so it
   should be unaffected, but it's explicitly in piece 8's "Done" line).
7. `~/git/tpm/prj/P2401-wireweaver/design/batched_streams.md`: add a "piece 8, measured" subsection under
   "Measured or to be measured" with the real numbers and the explanation above (matching how session
   shared-clock added its own "Shared clock, measured" subsection) — this is tpm-side, not a code repo, fine to
   edit directly.
8. Report ready: `tpm work say batched-streams "..."` with a one-line summary, then `tpm work ready --note
   "..."`. `git rm HANDOFF.md` from **both** worktrees before that (motor_sim's HANDOFF, if any, and this one) —
   this file should NOT be removed yet though: `tpm work handoff` (if that's how this continues) wants it
   present so the next conversation starts with it; only strip it right before the real `tpm work ready`.

## Gotchas learned the hard way (don't re-discover these)

- A `LevelRequest`'s `budget: Some(n)` for two levels observed at once must leave room under
  `device::LIMITS.total_budget` (8 000 000 B/s) for every other level too, including `Telemetry`'s default
  budget — asking for `4_000_000` on both FOC and Control alone already exhausts it, and `Telemetry` silently
  gets `budget: 0` (granted, but nothing ever sends). Observe one level per device session when measuring to
  avoid this entirely.
- `ObserveMode::Samples` updates never carry the head `t` (confirmed empirically: `first_t`/`last_t` stay `None`
  even with `LIMITS.timestamps: true`) — each `Series` item has its own. Any span-measuring code (like the
  shared `run()` test helper) that reads the head `t` can't be used on a level observed purely in `Samples`
  mode; observe a plain property (e.g. `telemetry.time`) alongside for the span, or just use a real-time sleep
  window since `Options::default().time_scale == 1.0`.
- `cargo fmt --all` must run after editing `stats.rs`'s long `use` lines / fn signatures — `just lint`'s
  `cargo fmt --all --check` caught a couple of these.
