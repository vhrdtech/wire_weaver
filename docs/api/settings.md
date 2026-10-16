---
level: advanced
---

# Settings store

`ww_settings` (STD-32) keeps a device's settings in its own NOR flash: a plain struct with a default for every
field, stored as a record of the fields that differ from their default. It survives a power cut at any point of a
write or an erase. Old and new firmware read each other's records without losing anything, and the fields are
served as ordinary `rw` properties.

```rust
use ww_settings::{Debounce, Key, Settings, Store};

#[derive(ww_settings::Settings, Clone, Debug, PartialEq)]
#[settings(retired(3))] // slots of removed fields, never used again
pub struct UserSettings {
    #[setting(slot = 0, default = ArrayString::new())]
    pub label: ArrayString<32>,
    #[setting(slot = 1, default = false)]
    pub plug_vbus_at_boot: bool,
    #[setting(slot = 2, default = ButtonAction::Toggle)]
    pub button_action: ButtonAction,
    #[setting(slot = 4, default = 100)]
    pub meter_window_ms: u16,
}
```

The derive writes `Default` (every field at its `default`) and `ww_settings::SettingsGroup`: the slot table with
field names, the crate's version, and per field the code to compare it with its default, write it and read it.

- **Slots** are `0..=255`, unique, in any order. A slot belongs to its field for good: renaming the field is fine,
  giving the slot to another field is not. A removed field's slot goes into `retired(..)`, and the derive refuses to
  reuse it.
- **Field types** are owned values with ShrinkWrap serdes and `PartialEq`: numbers, `bool`, enums and structs with
  `#[derive_shrink_wrap]`, arrays, and `shrink_wrap::ArrayString<N>` / `ArrayVec<T, N>` (SW-57) for text and lists
  without an allocator. The same types work for properties, so a field and the property that serves it share
  one type.
- **Defaults** are expressions: `default = 100`, `default = ButtonAction::Toggle`, a `const`.

## On the device

```rust
// a range at the end of bank 2 of an STM32G0B1 (4 pages of 2 KiB), see "How many pages" below
let mut store: Store<_, 512> = Store::new(flash, 0x0807_8000..0x0808_0000)?;
let mut user: Settings<UserSettings, 128> = Settings::new(Key::USER, Debounce::default());
let report = user.load(&mut store).await?; // defaults for anything not stored
// apply them: here to the server's value_on_changed fields
server.button_action = user.get().button_action;
```

`Settings<G, N>` holds the value in RAM, the stored record (up to `N` bytes) and whether something changed since.
`Store<F, B, K>` holds the flash, a buffer of `B` bytes and the location of the current record of up to `K` keys
(groups). Each group has its own key: `Key::USER` (0) for what users change, `Key::FACTORY` (1) for calibration and
production data, written through a debug API only.

**Served as properties.** The API declares the settings as plain `rw` properties; the server keeps them in
`value_on_changed` fields ([properties](properties.md#value_on_changed-a-field)), and each `changed_<name>` handler
hands the new value to the settings, which only changes RAM:

```rust
impl Server {
    fn changed_settings_button_action(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {
        let v = self.settings_button_action;
        self.user.set(now_ms(), |s| s.button_action = v);
    }
}
```

**Written when it settles.** `set` starts a `Debounce`: the record is written once the value was left alone for
`quiet_ms` (default 1 s), or `max_ms` (default 10 s) after the first change not stored yet, so a slider dragged for
a minute is written every 10 s, not on every step. The device's loop writes it:

```rust
loop {
    let wake = user.due_at(); // None: nothing to write
    match select(server.wait(), timer_at(wake)).await {
        Either::First(ev) => server.handle(ev).await,
        Either::Second(()) => {
            user.persist_if_due(&mut store, now_ms()).await?;
        }
    }
}
```

A write happens only when the record changed: setting a value and setting it back before the write costs nothing,
writing the same value again costs nothing.

**Written before answering.** Where an `Ok` must mean "survives a power cut", as `ww_user_info` promises for a
device name, the handler calls `persist` right away (an `async` handler, or a deferred reply):

```rust
user.set(now_ms(), |s| s.label = name);
user.persist(&mut store).await?; // Ok: written
```

**Factory reset.** `user.reset_to_defaults(now)` and `persist`: every field of the group at its default, an empty
record written. Other keys (`Key::FACTORY`) are not touched.

**What `load` reports.** `LoadReport` says whether a record was found, which version wrote it (`writer`) and the
highest version whose data is in it (`newest`, above the running firmware's after a downgrade), and how many fields
were read, how many slots belong to a newer firmware (kept on the next write) or to removed fields (dropped). A
record in an unknown format (a newer `ww_settings`) is left alone: `is_read_only()`, every field at its default,
`persist` refuses with `Error::ReadOnly` until a factory reset.

**Dropping a write.** A `persist` or `maintain` future dropped half way (a `select!` with a timeout) leaves the
flash as a power cut at that point would; the store goes on from there.

## What is stored

A page holds a log of records, not one struct: each write appends a new record for its key, and the newest one
wins. The record is the group's version and the fields that are not at their default, each as its own
ShrinkWrap bytes:

```text
record: | format: u8 = 1 | writer: 3 × UNib32 | newest: 3 × UNib32 | pad | slots: OptionAnyVec of byte blobs |
```

Every slot of the [OptionAnyVec](../serdes/special_types.md#optionanyvec-sparse-slots-of-any-types) holds the field's
bytes as a blob (a length and the bytes). That is what lets a firmware skip a slot it doesn't know, keep it when it
writes the record again, and drop a retired one, which a plain [`sparse`](../serdes/layouts.md#sparse-a-struct-of-optional-fields)
struct can't do (its values carry no length). The cost is a length nibble and byte alignment per stored field.

In flash each record is an item: a 12-byte header (length, key, a 32-bit sequence number per key, CRC-32) and the
record, padded to the flash word. Sizes for a Donguru-like group (label, a `bool`, an enum, two `u16`, a UART
configuration), measured with `record::encode` and `Store::footprint`:

| Record | Bytes | G0 item / per 2 KiB page | F4 item / per 16 KiB | H7 item / per 128 KiB | SPI NOR item / per 4 KiB |
|---|---|---|---|---|---|
| nothing set | 4 | 16 / 127 | 16 / 1023 | 32 / 4095 | 16 / 255 |
| one `bool` set | 8 | 24 / 84 | 20 / 818 | 32 / 4095 | 20 / 204 |
| label `"bench-3"` | 14 | 32 / 63 | 28 / 584 | 32 / 4095 | 26 / 157 |
| all six set, 16-char label | 39 | 56 / 36 | 52 / 314 | 64 / 2047 | 51 / 80 |

(G0: 8-byte words, F4: 4-byte, H7: 32-byte, SPI NOR: 1-byte; a page loses 8 to 32 bytes to its header.)

**When it compacts.** When a record doesn't fit the current page any more, the store moves on to the next page
(which is always erased or erasable), writes its header, and copies into it the current records that sit on the page
after that one, so that page holds none and can be erased. On the G0 above, with every field set, that is every
36th write; on the H7 every 2047th. Each page is erased once per round, so the pages wear evenly.

## How many pages

At least **2**: the store always keeps the page after the current one free of current records, so it has a page to
compact into, and with only one page there would be nowhere to put the current records while the page is erased.
`Store::new` refuses fewer. The current records of all keys, plus the one being written, must fit into one page:
otherwise a write fails with `StoreError::Full` and what is stored stays readable. Keep them under half a page, so a
cut copy (which wastes its space until the page is erased) can't fill it.

More pages spread the erases: with `p` pages, `n` records per page and an endurance of `E` cycles (10 000 on the
STM32 parts below), the flash lasts about `p · n · E` writes. On the G0 with 4 pages and every field set:
4 · 36 · 10 000 ≈ 1.4 million writes, plenty with the debounce. **On the G0, 4 pages (8 KiB) at the end of bank
2** is the recommendation: 2 would work, 4 halve the wear and leave room for records to grow.

## What an erase stalls

A write programs a few words, an erase takes milliseconds to seconds, and on a single-bank part the CPU can't fetch
code from flash while either runs: every interrupt, the USB stack and the executor stop until it is done (code and
interrupt handlers in RAM keep running). From the datasheets:

| Part | Erase unit | Erase, typ / max | Program | Banks |
|---|---|---|---|---|
| STM32G0B1 (DS13560) | 2 KiB page | 25 / 40 ms | 8 bytes: 56 / 90 µs | 2 on the 512 KiB parts: no stall from the other bank |
| STM32F405/407/411 (DS8626, DS10314), x32 | 16 / 64 / 128 KiB sectors | 250/500 ms, 550/1100 ms, 1/2 s | 4 bytes: 16 / 100 µs | 1 (F42x/F43x: 2) |
| STM32H743/753 (DS12110) | 128 KiB sector | x8: 2 / 4 s, x16: 1.8 / 3.6 s | 32 bytes: 100 / 200 µs (x64) | 2 |
| STM32H723/725/730/733/735 (DS13313) | 128 KiB sector, 8 of them | as H743 | as H743 | 1 |
| STM32H7A3/7B3 (DS13195) | 8 KiB sector | max 2.2 ms | 16 bytes: max 20 µs | 2 |
| W25Q128JV SPI NOR | 4 KiB sector | 45 / 400 ms | 256 bytes: 0.4 / 3 ms | separate chip: no stall |

What that means on a single bank: USB keeps NAKing in hardware, so the host sees a pause, not a disconnect, but a
request with a timeout shorter than the erase fails; the watchdog period has to be longer than the longest erase;
anything with a deadline (a control loop, a UART without DMA) misses it. A G0 page erase (25 ms) is a hiccup; a
128 KiB sector (1 to 4 s) is not.

**`Store::maintain()`** moves the stall to a time the device picks: it erases the page the next compaction will
move to, if it needs it, so the write that starts a new page doesn't erase. `Store::erase_pending()` says whether
one is due. A device can call it when the host is idle, at boot, or answer a request with `Busy` meanwhile.

## Parts with big sectors

On an STM32F4 or H7 the store needs two whole sectors, and on a single-bank part each erase stops the CPU for one
to four seconds. The options, best first:

1. **The other bank.** On a dual-bank part (H743/753, H7A3/7B3, F42x/F43x with `DB1M`), put the store in the bank
   the code doesn't run from: erases don't stall anything. On the H743 that is 2 × 128 KiB of bank 2; on the H7A3
   2 × 8 KiB; on an F42x in dual-bank mode bank 2 starts with 16 KiB sectors (12 to 15), the same as bank 1.
2. **The small sectors at the start of an F4.** Sectors 0 to 3 are 16 KiB each. Sector 0 has to hold the vector
   table (the part boots from `0x0800_0000`), so the store takes sectors 1 and 2 (or 1 to 3), and the code starts
   after them. With `cortex-m-rt` that is one symbol in `memory.x`:

    ```text
    MEMORY
    {
      FLASH : ORIGIN = 0x08000000, LENGTH = 1024K
      RAM   : ORIGIN = 0x20000000, LENGTH = 128K
    }
    /* sector 0: vector table; sectors 1-2 (0x0800_4000..0x0800_C000): ww_settings; code from sector 3 */
    _stext = 0x0800C000;
    ```

    The vector table stays at `0x0800_0000` (no VTOR change); the rest of sector 0, about 15.5 KiB, stays empty
    (or holds a bootloader). An erase still stalls the CPU (250 / 500 ms for 16 KiB), use `maintain()`. Flashing a
    new firmware with probe-rs erases only the sectors the image covers, so the settings survive it; a chip erase
    doesn't.
3. **External SPI / QSPI NOR.** 4 KiB sectors, about 45 ms per erase that doesn't touch the CPU (the bus is
   separate), 100 000 cycles; any `embedded-storage-async` `NorFlash` driver works. Not on a QSPI flash the code
   runs from (XIP): that stalls like internal flash.
4. **Two of the big sectors with `maintain()`.** On a single-bank H723/H733 (8 sectors of 128 KiB) there is no
   way around a 2 s stall per erase; with 64-byte records it happens every ~2000 writes, and `maintain()` puts it
   where it hurts least.
5. **One sector with a RAM copy (not supported).** A store in a single sector has to erase the sector that holds
   the only copy of every record, then write them back from RAM. A power cut from the start of that erase until the
   last record is written back loses every group, user settings and factory data alike: the device boots on its
   defaults. That window is the whole erase plus the rewrite, 1 to 4 s on a 128 KiB sector, once per sector fill.
   The RAM copy survives a soft reset, not a power cut. `ww_settings` doesn't offer this mode (STD-33); a device
   with exactly one sector to spare is better served by external NOR.

## Power cuts

What the store guarantees, at any point of any write or erase:

- `load` returns, for each key, either the record from before the interrupted write or the one being written, never
  anything else, and never an error.
- After the cut, the next write works, and finishes what the interrupted one left (a half done compaction).
- Loading never writes: a device that boots in a brown-out loop wears nothing.
- Every flash word is written once between erases, and only words that read erased are written (STM32G0/H7 ECC
  flash refuses or corrupts a second write).

How: every item carries a sequence number per key and a CRC-32, so the current record of a key is the valid item
with the highest number wherever it is; a cut write leaves an item that fails its CRC and is skipped. A page carries a
sequence number in a header with its own CRC, which says which page is current. Before a page is erased, the current
records on it are copied away, so a cut erase can leave only older records and copies behind, in whatever order the
erase cleared the bytes. The layout is in the `ww_settings::store` module docs.

The tests cut the power at every word written and every byte erased of a run that goes several times around the
pages, on G0-like (8-byte ECC words, 2 KiB pages), H7-like (32-byte ECC words) and F4-like (4-byte words, 16 KiB
sectors) flash, with erases that clear the bytes front to back, back to front or all at once, and cut again while
recovering (`ww_settings/tests/power_cut.rs`; `just test-power-cut` runs every step, `just test` a sample).

**ECC flash needs an ECC-aware driver.** On the G0 and H7 a word whose write was cut has an uncorrectable ECC error,
and reading it raises an NMI (G0) or a bus fault (H7): a plain flash driver turns a power cut into a boot loop. The
driver has to return such a word as zeros, which the store skips like any damaged item (SRV-19, `mcu/ww_ecc_flash`).

**Why not `sequential-storage`.** Its `map` was the first choice. Under the same tests it ends permanently
`Corrupted`, or returns an older value, when an erase is cut while the end-of-page marker already reads erased but
the start marker doesn't: it assumes the start marker clears first, which a sector erase doesn't promise.

## Evolution

The rules for changing a settings struct between firmware versions are in
[evolution rules](../evolution/rules.md#settings-stored-on-a-device). In short: add fields at new slots (also in gaps),
retire removed ones, never change a slot's type except by that type's own rules, and know that changing a default
changes the value of every device that never set the field.
