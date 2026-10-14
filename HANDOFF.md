# Handoff: ww-errors-plan

Errors and the trait surface for all APIs (Roman's IO family review, 14 Oct 2026). This session is the integrator:
children build pieces on branches from here, this branch collects them as plain commits (never merge commits:
`tpm land` rebases; take children's feature commits with `git cherry-pick`, skip their "drop handoff" commits).

## Done (on this branch, rebased onto master 0ac511b, 14 Oct late)

- Design (was `docs/api/errors.md`, now only in ~/git/tpm/prj/P2401-wireweaver/design/errors.md), approved by Roman 14 Oct (all recommendations). Condition: `RpcResult` /
  `GetResult` / `SetResult` keep their semantics; `CallError` keeps `Unimplemented` visible.
- SW-47 future-version-default: derive adds `Unknown(u32)` to Unsized enums; `no_future_version`,
  `future_version = Name`. ww_self / NumericAnyType / MultiIndex opt out (INT-8 idea).
- CLT-24 flat-call-errors: `CallError<E> { Device(E), Unimplemented { resource }, Client(Error) }`, incl. the port of
  master's CLT-10 code (`ww dfu reboot`, tests/cross_version).
- DER-12 display directive; convention: messages drop the doc line's trailing period (also `ww_self::doc_message`,
  CommonError). CommonError's Display stays hand-written (leaves out empty string parts).
- API-16 optional items (`#[optional]`, `omit`, `omit_if`, Unimplemented fallback, omitted lists in ww_self);
  tests/traits checks an omitted fallible method gives `CallError::Unimplemented`.
- STD-16 common errors: `ww_error::CommonError`, `#[common]`, `HasCommon`, prelude re-export, Python `.common`.
- `.config/nextest.toml`: ref_vec_skip random-buffer test gets 15 s.
- Roman's rule (14 Oct): no design/review notes in wire_weaver (public). docs/api/errors.md removed from the branch;
  docs/api/optional_items.md is the article for API-16 (in zensical nav). The main chat writes the errors article.
- ww_self::Delivery (API-30, from master) got no_future_version.
- docs/api/errors.md is now the article from the main chat (tpm design/errors_article.md), checked against the code
  and fixed (636401c); its #[errors] section describes API-32 as planned: check it against the child's work after
  the cherry-pick. docs/api/optional_items.md has the details of API-16.
- Last full run (after the 0ac511b rebase): just check, just test (788/788), just lint pass (lint warns about
  ww_firmware_info's Cargo.toml formatting: master's crate, not ours).

## In flight

- Child `ww-error-lists` (opus, task P2401#14oct-1805e): API-32 `#[errors(..)]` per-method lists, started from this
  branch before the last 3 commits (display convention, handoff). Told: strip `#[errors]` in DOC-6's rustdoc trait,
  use the no-trailing-period convention, keep Rpc/Get/SetResult, wire breaks are fine (no shims). When it says ready:
  cherry-pick its feature commits here, run just check / test / lint.

## Next

1. After API-32: cherry-pick, then `git rebase master` if master moved. Child was told not to touch docs/api/errors.md;
   if its commits do, drop that file and move user-facing parts into docs/api/methods.md.
2. Spawn the stdlib migration (task P2401#14oct-1805g, sonnet) from here, coordinated with session ww-io-traits:
   every IO crate that exists by then (ww_gpio, ww_uart, ww_i2c, ww_spi, ww_adc, ww_pwm, ww_onewire, ww_timebase;
   later ww_logic_analyzer, ww_signal_generator) plus ww_dfu, ww_can_bus: `#[common] Common(CommonError)` first,
   `#[errors(..)]` from their `# Errors` rustdoc sections, `#[optional]` where methods answer Unsupported today,
   `no_future_version` on complete-by-nature enums (ww_uart StopBits, Parity). Tell ww-io-traits when it starts.
3. When all is in and green: `git rm HANDOFF.md`, commit, `tpm work ready --note "..."`.

## Gotchas

- Roman's policy (14 Oct): until devices go to testers (end of 2026) wire breaks are fine, no shims or migration
  paths for undeployed formats (usb_io_fw docs/api_design.md Q13).
- /mnt/data filled up twice from parallel builds; check `df -h /mnt/data` before heavy runs. Children whose work is
  integrated may be closed with `tpm work done NAME --force` (user allowed it).
- Local `master` sometimes lags `origin/master`; rebase onto whichever is newer, plainly (no -i/--onto/--exec).
- rerere is on: repeated conflicts (dynamic.rs, changelogs) resolve themselves; check for leftover markers with
  `rg -l "^(<<<<<<<|>>>>>>>) "`.
- Narrow per-method error types: explored and not recommended (API-33 idea), reasons in docs/api/errors.md.
- ww_i2c's `I2cReadKind` gets `Unknown` too; its CHANGELOG lives on the ww-io-traits side.
