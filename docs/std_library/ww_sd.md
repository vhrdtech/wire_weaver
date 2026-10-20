---
level: advanced
---

# ww_sd

`ww_sd` (STD-40) gives a host raw access to an SD card: its state, its registers as the card sent them, the init
sequence with what each step answered, and single raw commands with their response, status bits and timing. It is for
bring-up, card checks and odd cards; on the device it is a passthrough to the SDMMC (or SPI) driver. It is `no_std` by
default and depends on `wire_weaver` and `ww_error`.

```toml
[dependencies]
ww_sd = "0.1"
```

Reading and writing data is [`ww_block`](ww_block.md)'s `BlockDevice`, served for the same card: it holds the write
lease that `ww_sd`'s changing commands need.

```rust
#[ww_trait]
pub trait SdCard {
    fn capabilities() -> Capabilities;
    fn state() -> CardState;
    stream!(detect: bool);
    fn registers() -> Result<SdRegisters<'i>, SdError<'i>>;
    fn reinit(lease: u32, max_clock_khz: u32, bus_width: BusWidth) -> Result<InitReport<'i>, SdError<'i>>;
    fn command(lease: Option<u32>, cmd: RawCommand<'i>) -> Result<RawResponse<'i>, SdError<'i>>;
}
```

## State and registers

- `capabilities()`: the host controller's `Interface` (`Sd` or `Spi`), widest bus, highest clock, UHS (1.8 V), and
  `unrestricted`, whether this build lets [restricted commands](#raw-commands) through.
- `state()`: `CardState` with `present`, the capacity class (`Sdsc`, `Sdhc`, `Sdxc`, `Sduc`), the bus after the
  last init (`BusState`: width, clock in kHz, `SpeedMode`, 1.8 V or not) and `leased`, whether a `ww_block` lease
  holds the card.
- `detect`: card inserted (`true`) or removed (`false`).
- `registers()`: `SdRegisters` with CID, CSD, OCR, RCA, SCR and SD Status (ACMD13) as raw bytes, MSB first, as the
  card sent them. Decoding is the host's: the device keeps no parser for fields it doesn't use itself. Read at init
  and cached, so safe at any time; `Common(WrongState)` before the card is initialised.

## Init with a report

`reinit(lease, max_clock_khz, bus_width)` runs the init sequence again (CMD0, CMD8, ACMD41, CMD2, CMD3, ..., then bus
width and speed up to the limits given) and answers an `InitReport`: each step's command, argument, the first 32 bits
of its response, its `Outcome`, how often it was sent (ACMD41 polls while the card is busy) and how long it took, then
the state it ended in and the total time. A card that doesn't come up is not an error: the report says where it
stopped. It needs the lease, since the card's user loses the card meanwhile.

## Raw commands

`command(lease, cmd)` sends one `RawCommand`: the index (0..=63), the argument, whether it is an application command
(CMD55 goes first), the `ResponseType` the controller should expect (`None`, `R1`, `R1b`, `R2`, `R3`, `R6`, `R7`), an
optional `DataPhase` (`Read { block_len, blocks }` or `Write { block_len, data }`, at most `MAX_DATA` = 4096 bytes) and
an optional timeout.

The answer `RawResponse` has:

- `outcome`: `Ok`, `ResponseTimeout`, `ResponseCrc`, `DataTimeout`, `DataCrc`, `BusyTimeout` or `Other { code }`.
  What the card did is data, not an error of the call, so the bytes and the timing come back on a failure too (R3 has
  no CRC: a controller that checks it anyway reports `ResponseCrc` with good bytes).
- `response`: the response bytes, MSB first, without start bit, index and CRC (4 bytes, 16 for R2; in SPI mode
  see [below](#in-spi-mode)).
- `status`: the card status bits of an R1 or R1b response as `CardStatus`, with the bits as constants
  (`CardStatus::ILLEGAL_COMMAND`, ...), `current_state()` and `is_error()`.
- `data`: what the data phase read.
- `timing`: microseconds from sending the command to the end of the response, of the data phase and of busy. This is
  what makes raw commands worth having for bring-up and for judging cards.

An error of the call means the command wasn't sent: `NoCard`, `BadLease`, `Restricted`, `Common(Unsupported)` for a
command the interface doesn't have (SPI mode), `Common(InvalidArgument)`.

### In SPI mode

A card in SPI mode answers with R1-based responses of its own, and some commands don't exist there. A device whose
`capabilities().interface` is `Spi` fills the answer like this (as rePort's `sd_proto` engine does):

- `response`: the bytes as the card sent them, R1 first: 1 byte for R1 and R1b, 2 for R2 (CMD13, ACMD13: R1 and the
  second status byte), 5 for R3 and R7 (R1 and the OCR or the CMD8 echo).
- `status`: R1 (and R2's second byte) mapped onto the SD bus card status bits, for every response: the error bits
  where SPI mode has them, `CURRENT_STATE` 0 (idle) while R1 says idle, else 4 (tran).
- The response format comes from the device's command table; the host's `ResponseType` is used only for an index
  the table doesn't know.
- CMD9, CMD10, CMD6, CMD30, ACMD13, ACMD22 and ACMD51 read their block (CSD, CID, the switch status, ...) into
  `data` also with `DataPhase::None`, since the card sends it either way.
- Commands only the SD bus has (CMD2, CMD3, CMD7, CMD11, CMD15, CMD19, ACMD6) answer `Common(Unsupported)`, and so
  does `reinit` with a 4-bit bus.
- `Outcome::Other { code }`: 0x01-0x0f a read answered with this data error token (bit 0 error, 1 CC error, 2 card
  ECC failed, 3 out of range), 0x10 written data refused with a write error, 0x11 a byte that is no data response
  token, 0x12 the bus driver failed.
- `registers().rca` is 0, and an `InitStep`'s `response` holds the OCR (R3) or the CMD8 echo (R7) where the command
  has one, else R1 in bits 7:0 and R2's second byte in bits 15:8.

### What a command needs

`ww_sd::access(index, arg, app)` (also `RawCommand::access()` and `RawCommandOwned::access()`, so a host can tell
before sending) sorts commands into three `Access` classes:

| `Access` | Commands | Needs |
|---|---|---|
| `Free` | reads and status queries: CMD9, CMD10, CMD13, CMD17, CMD18, CMD19, CMD30, CMD55, CMD6 in check mode, CMD56 read, ACMD13, ACMD22, ACMD51, and CMD58/CMD59 of SPI mode | nothing |
| `Lease` | everything else: writes, CMD38 erase, CMD6 switch (changes the speed under the driver), CMD0, bus width, ... | the current lease of `ww_block::BlockDevice::claim` |
| `Restricted` | CMD42 (lock, unlock, passwords, force erase) and CMD27 (program CSD, whose permanent write-protect bit can't be cleared) | refused: they can lock a card or damage it for good |

It is an allow-list: a command not known to only read needs the lease. A firmware built with the feature
**`raw-sd-unrestricted`** of `ww_sd` lets restricted commands through with the lease, for bring-up builds only, and
says so in `Capabilities::unrestricted`. `ww_sd::check_access(&cmd, lease_ok)` is the whole rule for the device's
`command` handler.

```rust
fn sd_command(
    &mut self,
    _cx: &mut Context<'_, impl BlockingEventOut>,
    lease: Option<u32>,
    cmd: ww_sd::RawCommand<'_>,
) -> RpcResult<Result<ww_sd::RawResponse<'_>, ww_sd::SdError<'_>>> {
    let lease_ok = lease.is_some_and(|l| self.lease_ok(l));
    if let Err(e) = ww_sd::check_access(&cmd, lease_ok) {
        return Ready(Err(e));
    }
    // hand the command to the driver
}
```

Who may call the lease commands at all is the firmware's choice per link, made where it hands out the lease
(`ww_block` answers `claim` with `Common(Refused)`, see [Who may write](ww_block.md#who-may-write)); `ww_sd` has no
access rules of its own.

## Errors

`SdError` nests `CommonError` first (see [ww_error](ww_error.md)), then `NoCard`, `BadLease` (the command changes the
card and the call has no current lease) and `Restricted { index }`. Each call lists those it returns (`#[errors]`).

## Serving it

`ww api scaffold <crate>` prints the server struct and every handler stub. The device needs a raw-command hook in
its SD driver. `tests/block_sd` serves a mock card through `SdCard` and `BlockDevice` with one shared lease.
