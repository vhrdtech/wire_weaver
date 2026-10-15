# ww_onewire

`ww_onewire` is a global WireWeaver trait for 1-Wire masters: reset the bus, find the devices on it (paged or
bit-level search), and run addressed transactions, with strong pull-up for parasite-powered parts. It is `no_std`
by default and depends only on `wire_weaver` and `ww_si` (for the `Second` type `OneWireTransaction::strong_pullup`
uses as a duration).

Add it to a device API crate with:

```toml
[dependencies]
ww_onewire = "0.1"
```

## The protocol, briefly

A 1-Wire bus is a single data line shared by a master and one or more slaves, each identified by an 8-byte ROM id
(`RomId`): a family code, a 48-bit serial number, and a CRC8 of the other seven bytes. Every operation starts with
a reset pulse and a presence pulse from any slave listening (`OneWire::reset`); `OneWire::transaction` does this
implicitly. A slave is picked with its `RomId` (`Match ROM`) or, on a bus with one device, skipped entirely
(`rom: None`, `Skip ROM`).

Finding the ROM ids on a bus (there is no broadcast "who's there") is a bit-level search: a reset, the `Search ROM`
command (`0xF0`) written through `OneWire::transaction` with an empty read, then up to 64 calls to
`OneWire::triplet`, one per ROM bit, walking a binary tree of every bit position where connected devices disagree.
`OneWire::search` runs this on the device and returns whole ids a page at a time, which is what most hosts want;
`OneWire::triplet` exposes the same primitive for a host that wants to drive the tree walk itself (e.g. to stop
early once it has found the family code it is looking for).

## Trait

```rust
#[ww_trait]
pub trait OneWire {
    fn transaction(t: OneWireTransaction<'i>) -> Result<MaxLen<RefVec<'i, u8>, OW_MAX>, OneWireError>;

    fn reset() -> Result<bool, OneWireError>;

    fn search(from: Option<SearchState>, family: Option<u8>) -> Result<SearchPage<'i>, OneWireError>;

    fn capabilities() -> OneWireCapabilities<'i>;

    fn configure(config: OneWireConfig) -> Result<OneWireConfig, OneWireError>;

    fn triplet(direction: bool) -> Result<Triplet, OneWireError>;
}
```

* `transaction` - reset, ROM select (or skip ROM), write, read `read_len` bytes, optional strong pull-up after the
  last byte. The common way to talk to one known device.
* `reset` - reset pulse on its own; `true` when a device answered with a presence pulse (not an error - an empty
  bus is a normal `false`).
* `search` - paged ROM search: start with `from: None`, continue with each page's `next` until it comes back
  `None`. `family`, when given, only returns ids whose first byte matches.
* `capabilities` - pins the bus can be put on, IO levels, and whether overdrive / strong pull-up are supported.
* `configure` - puts the bus on one of `capabilities().pins` at a given speed and IO level (`io_level: None` leaves
  it as it is), or disables it; answers with what was actually set, as the `configure` of the other IO crates does.
* `triplet` - one search step at bit level, for a search run on the host instead of the device (see
  [The protocol, briefly](#the-protocol-briefly)).

## Strong pull-up

Parasite-powered parts (no separate Vcc pin) draw their supply from the data line and need it actively driven high
for the duration of certain operations (EEPROM writes, DS18B20 temperature conversions) rather than left to the bus
pull-up resistor. `OneWireTransaction::strong_pullup` asks the device to do this for the given duration right after
the transaction's last byte; `OneWireCapabilities::strong_pullup` says whether it can.

## Types

* `RomId` - `bytes: [u8; 8]`: family code, 48-bit serial, CRC8, most to least significant.
* `OneWireTransaction<'i>` - `rom: Option<RomId>` (`None` = skip ROM), `write: MaxLen<RefVec<'i, u8>, OW_MAX>`,
  `read_len: u16`, `strong_pullup: Option<Second>`.
* `SearchState` - where a paged `search` left off: `last_rom: RomId`, `last_discrepancy: u8` (the classic
  Dallas/Maxim search state, carried from one page to the next).
* `SearchPage<'i>` - `roms: MaxLen<RefVec<'i, RomId>, MAX_ROMS>`, `next: Option<SearchState>` (`None` when all
  devices were found).
* `Triplet` - one `triplet` step's result: `id_bit`, `cmp_bit` (both set when every remaining device agrees on this
  bit, both clear at a fork), `direction` (the bit this step actually drove onto the bus).
* `OneWireConfig` - `enabled: bool`, `pin: u8` (index into `OneWireCapabilities::pins`), `speed: OneWireSpeed`,
  `io_level: Option<Volt>`; the argument and the answer of `configure`.
* `OneWireSpeed` - `Standard` / `Overdrive`; `sized`, 1 bit.
* `OneWireCapabilities<'i>` - `pins: MaxLen<RefVec<'i, MaxLen<&'i str, 16>>, MAX_PINS>` (by name), `io_levels:
  MaxLen<RefVec<'i, Volt>, MAX_IO_LEVELS>`, `io_level_min` / `io_level_max: Option<Volt>`, `overdrive: bool`,
  `strong_pullup: bool`.
* `OneWireError` - `Unsupported`, `InUse`, `NoIoLevel`, `Refused`, `InvalidArgument`, `WrongMode`, `NoPresence`,
  `CrcMismatch`, `ShortCircuit`; self-describing (`unib32` discriminant).

Constants: `OW_MAX` (256, largest write or read of one transaction), `MAX_ROMS` (16, most ROM ids in one search
page), `MAX_PINS` (8, most pins a bus can be put on).

All enums and structs are behind `#[derive_shrink_wrap]` with `owned(feature = "std")`, so owned (`*Owned`)
variants are available by enabling the crate's `std` feature (on by default).

## Composing into a device API

A device with one 1-Wire bus implements `OneWire` once, flat, on its API root:

```rust
#[ww_api_root]
pub trait MyAdapter {
    ww_impl!(onewire: ww_onewire::OneWire);
}
```

## Testing (`tests/io_onewire`)

`tests/io_onewire` (with its `tests/io_onewire_api` companion) is a worked, in-process example: a mock bus server
driven by the generated client through `tests_common::start_device`. The mock implements the actual Dallas/Maxim
search algorithm against a handful of simulated `RomId`s (not just canned answers), so `search`'s paging and
`triplet`'s bit-level walk are both exercised for real, including the `family` filter and a full 64-bit
`triplet`-driven search that reconstructs a known `RomId`.
