---
level: advanced
---

# ww_memory

`ww_memory` gives a host low-level access to memory or registers of a device: read and write words at addresses, and
optionally watch addresses for changes. It is `no_std` by default and depends only on `wire_weaver`.

```toml
[dependencies]
ww_memory = "0.1"
```

A card or flash read and written in whole blocks (geometry and erase units, a write lease, CRCs, bulk transfers) is a
different contract: that is [`ww_block`](ww_block.md).

## Traits

| Trait      | Address | Word  | For                                                    |
|------------|---------|-------|--------------------------------------------------------|
| `Memory8`  | `u8`    | `u8`  | Register maps of small chips (I2C/SPI sensors, PMICs). |
| `Memory32` | `u32`   | `u32` | Memory and peripheral registers of a 32-bit MCU.       |

Both have the same items; every read, write and watch of a trait uses its one word type, and an address counts words
of that type.

```rust
#[ww_trait]
pub trait Memory32 {
    fn read(addr: u32, allow_side_effects: bool) -> Result<u32, MemoryError<'i>>;
    fn write(addr: u32, value: u32) -> Result<(), MemoryError<'i>>;
    fn read_many(addr: u32, len: u32, allow_side_effects: bool) -> Result<RefVec<'i, u32>, MemoryError<'i>>;
    fn write_many(addr: u32, data: RefVec<'i, u32>) -> Result<(), MemoryError<'i>>;

    #[optional(watches)]
    fn watch(addr: u32) -> Result<(), MemoryError<'i>>;
    #[optional(watches)]
    fn watching() -> RefVec<'i, u32>;
    #[optional(watches)]
    stream!(changed: (u32, u32));

    property!(ro address_range: (u32, u32));
    property!(ro access: Access);
}
```

* `read` / `read_many` - the word at `addr`, or `len` words from it. With `allow_side_effects` false, an address
  whose read changes the device's state (a status register cleared on read) isn't read: `read` answers
  `Common(Refused)`, `read_many` skips it.
* `write` / `write_many` - the word at `addr`, or consecutive words from it; `Common(Refused)` for a read-only
  address.
* `watch`, `watching`, `changed` - the [optional](../api/optional_items.md) group `watches`: start watching a word,
  list the watched addresses, and a stream of `(address, new value)` changes. A device without it writes
  `omit = ["mem.watches"]` (with its own path to the trait), and a host calling them gets `Unimplemented`.
* `address_range` - the first and last valid address.
* `access` - `Access::ReadOnly`, `WriteOnly`, `ReadWrite` or `Custom(Nibble)`.

## Errors

`MemoryError<'i>` starts with `#[common] Common(CommonError)` ([common errors](../api/errors.md#common-errors)), then
`WrongAddress` (outside `address_range`, or a range running past its end) and `OutOfWatchSlots`. Each method lists
the variants it returns with `#[errors(..)]`.
