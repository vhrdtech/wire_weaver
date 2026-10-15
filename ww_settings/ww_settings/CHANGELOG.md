# ww_settings changelog

## Unreleased

### 🚀 Features

- First version (STD-32): a power-cut safe settings store for devices (`docs/api/settings.md`).
    - `#[derive(Settings)]` (from `ww_settings_derive`) on a plain struct, `#[setting(slot = N, default = <expr>)]`
      per field, `#[settings(retired(..))]` for removed fields: implements `Default` and `SettingsGroup` (slot table
      with names, the crate's `Version`).
    - `record`: one record per group, `| format | writer version | newest version | OptionAnyVec of byte blobs |`,
      only the fields not at their default; `encode` keeps the slots of a newer firmware, the bytes of a field it
      didn't change, and bytes it can't decode while the field is at its default; `decode` reports what it found.
    - `Settings<G, N>`: `load` (`LoadReport`), `get`, `set(now_ms, ..)` with write-on-change, `Debounce`
      (`due_at`, `persist_if_due`), `persist` for writes that must be done before answering, `reset_to_defaults`,
      read-only mode for a record of an unknown format.
    - `Store<F, B, K>` over an `embedded-storage-async` `NorFlash`, one record per `Key` (`USER`, `FACTORY`): an
      own log format (per-key seq and crc32 in every item, page headers with a page seq), current records copied off
      a page before it is erased, never a word written twice, loads never write, `maintain()` / `erase_pending()` to
      erase ahead, `Full` / `TooManyKeys` without touching what is stored.
    - `fake_flash::FakeFlash` (feature `fake-flash`): NOR flash in RAM with power cuts at any word written or byte
      erased, three erase orders, ECC words, a hard error on a second write.
- `dump` feature (TOOL-12): a host decoder of a `Store` flash dump. `dump::analyze(image, base, word_size,
  page_size)` reports the current record of every key (run through a real `Store` over the bytes, via
  `fake_flash::FakeFlash`, so it is exactly what a device would read) plus a raw, word-by-word walk of every page
  and item slot (`Valid` / `Cut` / `Garbage` / `Erased`) for forensics, including pages `Store` itself never looks
  at (an invalid header). `dump::decode_group` decodes a current record's bytes into a concrete `SettingsGroup` for
  a caller that knows the type. `ww settings dump` in `wire_weaver_cli` is the CLI front end.
