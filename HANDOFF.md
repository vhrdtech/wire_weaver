# HANDOFF: ww-settings (STD-32, P2401#15oct-2059)

Branch `s/15oct2026-ww-settings`. Plan and datasheet figures: `~/git/tpm/prj/P2401-wireweaver/design/ww_settings.md`
(its "Findings" still describe sequential-storage as the store: superseded, see below). Upstream notes:
`.../design/upstream-sequential-storage-erase-order.md` (not posted).

## Done (committed)

- `ww_settings/ww_settings` + `ww_settings_derive`: `#[derive(Settings)]`, `record` (OptionAnyVec of per-field byte
  blobs; keeps unknown slots, unchanged fields' bytes, undecodable bytes of default fields), `Settings<G, N>`
  (load/get/set/debounce/persist/reset/read-only), own `Store<F, B, K>`, `fake_flash::FakeFlash`.
- **Decision: own store, not sequential-storage 8.** Its map fails the harness when an erase is cut in a realistic
  order (end marker reads erased before the start marker): permanent `Corrupted` (4 pages, one cut) or a lost value.
  Parent (donguru-api) was told. Own store: per-key seq + crc32 per item, page_seq header with crc, records copied
  off a page before it's erased, loads never write, `maintain()` erases ahead, state updated only after the page
  header write (dropped futures = power cut).
- Tests: `tests/settings.rs` (evolution old->new->old->new, newer enum variant kept, unknown format read-only),
  `tests/store.rs`, `tests/power_cut.rs` (every word write and erased byte, G0/H7/F4 geometries, forward/backward/
  random erase, double cuts). `just test` samples in debug (~10 s), `just test-power-cut` every step in release
  (~30 s, nextest profile `power-cut`).
- Docs `docs/api/settings.md` (nav after stream_stores), `docs/evolution/rules.md#settings-stored-on-a-device`,
  FEATURES STD-32 (🧪), STD-33 💡, API-40 💡, changelogs (ww_settings, derive, root Unreleased).

## Children (spawned by this session)

- `sw-array-string` (SW-57 ArrayString/ArrayVec): ready to land.
- `ecc-safe-flash` on omarchy-m1 (SRV-19 `mcu/ww_ecc_flash` + TEST-21 reset loop): ready to land. Real power-cut
  part filed as P2401#15oct-2157 (TEST-21, after API-11 or with the PA6 poke).
- `settings-dump` (TOOL-12): done, its commit cherry-picked here (7312401b) + dump now imports store's layout
  constants (3dd17872). Its own branch must not land separately.
- `settings-demo` (EX-15): done, cherry-picked here (a33a3dbb..0c2ab930) + fix on top (persist without a host,
  maintain() after a write, ECC driver note). Its branch must not land on its own.
  Both branched before 7194c580 (dropped-future fix) and da6adba7 (slot_decodes, docs): the `SettingsGroup` trait
  gained `slot_decodes`, only matters to a hand-written impl.

## Next

1. /mnt/data on gpd-omarchy is offline for a reformat (user, 15 Oct 22:30): no cargo there until the user says
   it's back. Then: `just lint`, `just test`, `just check` on the whole branch (the demo bins passed
   check-examples-mcu-nucleo-*; host `settings` test passes; full lint/test not yet run after the cherry-picks).
2. Done (second conversation): `just lint`, `just test` (1101 pass), `just check` all clean on 197d9359.
3. The docs page refers to `mcu/ww_ecc_flash` (SRV-19, on the ecc-safe-flash branch) and ArrayString (SW-57, on
   sw-array-string): fine once those land.
4. Done: tpm design doc Findings updated to the own-store decision.
5. `git rm HANDOFF.md`, `tpm work ready --note "..."`.

## Gotchas

- FakeFlash ECC mode: a word a cut left half done reads zeros (the SRV-19 driver contract); a word that reads all
  0xff after a cut erase is clean. Non-ECC mode allows bit-clearing rewrites (F4 does), counted in `Stats::rewrites`;
  clean runs assert 0.
- Store record size limit: the current records of all keys + the new one must fit a page (`StoreError::Full`
  otherwise, nothing lost); the key being written isn't carried over at compaction.
- Rustdoc: `Settings` is both a struct and a derive: link with `struct@Settings`.
