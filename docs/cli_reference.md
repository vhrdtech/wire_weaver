---
level: advanced
---

# Command reference

Every `ww` command with its options. Start with the [command line tool](cli.md) page: installation, the first
session with a device, and the device selection flags all commands share. `ww help <command>` or
`ww <command> --help` prints the options of the installed version.

## `ww list`

Lists devices reporting a WireWeaver API id, without opening them: the information comes from USB string
descriptors (see [USB](transport/usb.md)), so it works even while another application is connected to the device.

```
$ ww list
LOCATION             PRODUCT             SERIAL                    API                 HASH              LABEL
usb 003-1 c0de:cafe  WireWeaver Generic  09000F000251313438333634  all_gpio_api@0.1.0  5ff989a95162d4d7
```

- `LOCATION`: transport, bus and port chain, VID:PID. The `003-1` part is what `--usb-path` expects.
- `API`: name and version of the API crate the firmware implements, `HASH`: its signature.
- `LABEL`: user label set by the firmware, if any.

Only devices matching the [device selection](cli.md#device-selection) are shown, so `ww list` is also the way to check
what a set of filters (or a `ww.toml`) selects. When some of the filters come from `ww.toml`, a note saying so is
printed to stderr.

| Option        | Description                                                               |
|---------------|---------------------------------------------------------------------------|
| `-a`, `--all` | Also list USB devices not reporting a WireWeaver API id                   |
| `--plain`     | One device per line with `key=value` fields, without table formatting     |

```
$ ww list --plain
usb 003-1 c0de:cafe "Vhrd.Tech" "WireWeaver Generic" serial=09000F000251313438333634 api=all_gpio_api@0.1.0 hash=5ff989a95162d4d7
```

## `ww introspect`

Connects to the selected device, downloads its introspection data (see [ww_self](std_library/ww_self.md)) and
prints the API as a resource tree: resource ids, methods with their signatures, properties with their access mode,
streams and sinks, nested traits, arrays of resources and doc comments. A summary follows: resource, trait and type
counts, size of the full API bundle and of the introspection data the device sent, referenced crates and the API
hash. Traits and types a device leaves out of its introspection data because they are known from
[snapshots](#ww-api) are put back first, any that could not be put back are counted as not included.

The tree looks the same as the one printed by [`ww api tree`](#ww-api) from source. Here with docs hidden:

```
$ ww introspect -d
trait AllGpioApi all_gpio_api@0.1.0
└─ 0 impl port[]: ww_gpio::Bank
   ├─ 0 impl pin[]: ww_gpio::Pin
   │  ├─ 0 fn set_output_level(level: Level)
   │  ├─ 1 fn toggle()
   │  ├─ 2 fn output_level() -> Level
   │  ├─ 3 fn input_level() -> Level
   │  ├─ 7 stream event: IoPinEvent
   │  ├─ 8 fn set_mode(mode: Mode, initial: Option<Level>) -> Result<(), Error>
   │  ├─ 9 fn mode() -> Mode
   │  ├─ 10 rw property pull: Pull, write error: Error
   │  ├─ 11 rw property speed: Speed, write error: Error
   │  └─ 12 fn configure_events(enabled: IoPinEnabledEvents) -> Result<(), Error>
   ├─ 8 fn capabilities() -> BankCapabilities
   ├─ 9 rw property reference_voltage: Volt, write error: Error
   └─ 10 fn name() -> String

15 resources, 2 traits, 11 types, 4362 bytes full, 72 bytes sent
crates: all_gpio_api@0.1.0, ww_gpio@0.1.0, ww_si@0.1.0, ww_numeric@0.2.0
api hash: 6e87e4113afd0168
```

`impl name[]: crate::Trait` is a nested trait (`[]` marking an array of them), properties show their access
(`const`, `ro`, `rw`, `wo`) and whether they are observable, `stream` is device-to-host and `sink` host-to-device.
The values of [`const` items](api/properties.md#const-items) outside of arrays of traits are read from the device
and shown after their type, `const property SERIAL: String = "E661A410"` (byte vectors as hex, struct and enum names
left out); `--no-values` skips the reads.

With `-t`, all types the API refers to are listed after the tree, by their index in the bundle: structs with their
fields, enums with their variants and discriminants, and each type's size kind (`unsized`, `final structure`,
`self-describing`, `sized`) and enum repr:

```
$ ww introspect -d -t
...
types
├─ 0 enum ww_gpio::Level sized, repr u1
│  ├─ 0 Low
│  └─ 1 High
├─ 1 enum ww_gpio::IoPinEvent sized, repr u2
│  ├─ 0 RisingEdge
│  ├─ 1 FallingEdge
│  └─ 2 Custom(u8)
...
```

| Option              | Description                                                                                     |
|---------------------|-------------------------------------------------------------------------------------------------|
| `-d`, `--skip-docs` | Do not print doc comments                                                                       |
| `-t`, `--types`     | Also print all types the API refers to                                                          |
| `--raw`             | Print raw introspection data (Rust debug format) instead of the tree                            |
| `--raw-as-sent`     | Same as `--raw`, but as the device sent it: traits and types known from snapshots are left out  |
| `--no-values`       | Do not read the values of `const` items                                                         |

Fails with an error if the device did not provide introspection data.

## `ww api`

Works on API crate sources instead of a device: the crate is parsed the same way `ww_codegen!` does it, so no
device and no build are needed.

`ww api tree <path>` prints the same resource tree as `ww introspect`, for the `#[ww_trait]` defined in the crate
at `<path>`:

```
$ ww api tree examples/blinky_api
trait BlinkyApi blinky_api@0.1.0
├─ 0 fn led_on()
└─ 1 fn led_off()
```

`ww api ast <path>` prints the parsed API bundle in [RON](https://github.com/ron-rs/ron) format, useful when
debugging codegen or introspection.

Both take `--name <Trait>` to pick the trait when the crate defines more than one, and `tree` also takes
`-d`/`--skip-docs` and `-t`/`--types`.

`ww api scaffold <path>` prints the server side of an API: a server struct, a stub for every handler with its
exact signature (methods and property getters/setters answer `Unimplemented`, stream and on-changed handlers do
nothing, no array index is valid) and the matching `ww_codegen!` call. The output compiles as is, so a new firmware
answers every request right away and the stubs are filled in one by one. Handler signatures depend on the codegen
options, so pass the same ones the `ww_codegen!` call uses:

| Option                     | Default         | Description                                                                 |
|----------------------------|-----------------|-----------------------------------------------------------------------------|
| `--name <Trait>`           |                 | The trait, when the crate defines more than one                             |
| `--server <Struct>`        | `Server`        | Name of the server struct                                                   |
| `--use-async`              |                 | Async handlers                                                              |
| `--handler-model <M>`      |                 | With `--use-async`: which handlers are `async fn`, the rest plain `fn`, see [sync and async handlers](api/handlers.md) |
| `--alloc`                  |                 | Handlers for `no_alloc = false`, taking and returning owned types (`String` instead of `&str`, see [the ww hub](transport/hub.md#servers-on-std-no_alloc-false)) |
| `--method-model <M>`       | `_=immediate`   | As `method_model` in `ww_codegen!`, see [methods](api/methods.md)           |
| `--property-model <M>`     | `_=get_set`     | As `property_model`, see [properties](api/properties.md)                    |
| `--const-model <M>`        |                 | `BOARD=fn` and the like: `const` items fixed at boot or link time, see [const items](api/properties.md#const-items) |
| `--medium <Type>`          | `()`            | Medium type handlers get in their `Context`                                 |
| `--omit <items>`           |                 | [Optional items](api/optional_items.md) to leave out, e.g. `i2c.pullups,reset` |
| `-o`, `--output <file>`    | stdout          | Write to a file; an existing file is not overwritten                        |

```sh
ww api scaffold examples/blinky_api --use-async -o firmware/src/server.rs
```

`ww api save <path>` saves every `#[ww_trait]`/`#[ww_api_root]` trait defined in the crate's `src/lib.rs`, and every
`#[derive_shrink_wrap]` type and unit marker struct (`pub struct Volt;`) defined there, in a `pub mod` at any depth,
or re-exported from a module with `pub use` (each known by its path and name, `units::Volt`) into `<path>/api_snapshots/<crate>_<major>_<minor>_<patch>.ron`, for example
`ww_stdlib/ww_gpio/api_snapshots/ww_gpio_0_1_0.ron`. Commit the snapshot and never change it afterward.
It is a regular API bundle in RON: the root is named after the crate and has one nested trait resource per trait,
definitions from this crate are included, and traits and types from other crates are only referenced by crate
version, path and name, and signature. Save a snapshot of each of those crates too.

A signature is a hash of the full definition with docs, including everything it refers to, calculated as if nothing
was skipped (`ww_self::signature`). A definition restored from another crate's snapshot can then be checked to be
exactly the one that was left out, which catches a crate changed without bumping its version. The client does the same
when it compares its API with a device's: a skipped trait or type with the same crate version but a different
signature makes the resources using it incompatible.

Snapshots of `ww_global` and the `ww_stdlib` crates are embedded into the `wire_weaver_snapshots` crate, used by
codegen and by `wire_weaver_client` (and so by `ww`). A device leaves traits and types known from them out of its
introspection data, referring to them by crate version, name and signature, which makes it much smaller (e.g.
`examples/all_gpio_api` goes from 4363 to 72 bytes). A definition is only left out if it is exactly the one in the
snapshot, e.g. an unpublished crate version that changed after its snapshot was saved stays in-line. The API hash is
calculated over the data as sent. Generated clients embed their own API in the same form. A client puts the left out
definitions back from its own snapshots after download or loading from cache, checking signatures; one it doesn't know (e.g. from a newer `ww_stdlib` crate) stays skipped, and resources using it
are only checked by crate version and signature. In this repo, `just save-snapshots` saves the snapshots and copies
them into `wire_weaver_snapshots/api_snapshots/`; a `wire_weaver_core` test fails if any of them is out of date.

```
$ ww api save ww_stdlib/ww_uart
saved ww_stdlib/ww_uart/api_snapshots/ww_uart_0_1_0.ron, 12 resources, 1 traits, 11 types (0 traits and 2 types not included)
```

Running it again does nothing if nothing changed. Any change is an error, including doc comments only:
bump the crate version (see [evolution rules](evolution/rules.md#doc-comments), doc-only changes bump the compatible
position, so that tools can tell that improved docs are available) and save a new snapshot. `--force` overwrites anyway, only use it for a
version that was never published. A snapshot of a new version is only saved if the version is bumped enough since the
previous snapshot, see below.

`ww api check <path>` is the [evolution checker](evolution/checker_tool.md): it compares the crate with its latest
snapshot and fails if the version is not bumped enough for what changed (breaking position for breaking changes,
compatible position for anything else, doc comments included). `ww api diff <path>` compares the same versions and
lists every change, doc comments included, see [listing all changes](evolution/checker_tool.md#listing-all-changes).

Current limitations, same as for codegen: traits are only found in `src/lib.rs`, a dependency reached through another
crate's re-export is only resolved for the ones `wire_weaver` re-exports (`ww_version`, `shrink_wrap`), and crates
with a workspace-inherited version can't be loaded.

## `ww observe`

Observes a trait level's `+observe` properties ([Observing properties](api/observing.md)) from introspection
alone, no generated code needed (TOOL-11, the same dynamic client [Python](python_advanced.md#observing) uses):

```
$ ww observe motor --rate 10
observing motor (3 properties): speed, current, position
budget: 20.0 kB/s
Ctrl+C to stop
{speed: 1500, current: 3, position: 1024}  rate 98.2/s  link 982/s
```

`<path>` is dotted/indexed (`motor`, `bank[0]`), empty for the API root. Without `--rate` every observable property
is asked for on-change; with it, all of them periodically at that rate (milliseconds). Prints each update's present
values, the measured update rate and the link bytes/s it costs, until Ctrl+C. See
[Observing properties](api/observing.md#the-host-side) for the host-side model this reuses.

`--samples MS` observes every sample of the level's `+batched` properties, in batches at most `MS` old
([Every sample](api/observing.md#every-sample-batched)): one line per sample with its device time in seconds, the
other properties as before.

```
$ ww observe foc --samples 5
  4294.966696 i_a: 1250 mA
  4294.966736 i_a: 1262 mA
  ..
```

## `ww explain`

Shows which bits of a buffer hold which value (TOOL-9): every field with its place in the buffer, its raw bits, its
type and its decoded value, the lengths, enum discriminants, `Option` flags and padding in between, and a hex map
coloured by field. Useful to check what a device really sends, to read a captured frame, or to learn the
[wire format](serdes/derive.md) on your own types.

```
$ ww explain --crate api 'sub_byte(I2cAddress::Ten(0x2A5), 100, -300)'
sub_byte arguments · 4 bytes

0000  d4 b9 2d 40 

bits             raw                  field                      type         value
0‥10                                  addr                       I2cAddress   Ten
0                1                      discriminant             u32          1
1‥10             1010 1001 01           0                        ub10         677  0x2a5
11‥17            1100 100             a                          ub7          100
18‥27            1011 0101 00         b                          ib10         -300  0x2d4

28 bits in values, 4 padding or unread
```

`<what>` is a type of the API (`Config`), or a resource path: a method's arguments (`motor.set_speed`, `--return` for
its return value), a property's value, a stream's item. The bytes come in hex (`0a1b2c`, `0a 1b 2c`, `0x0a,0x1b`),
from `--file`, or as a value written as a Rust expression, which is encoded first:

```
$ ww explain Config --value 'Config { mode: Mode::Fast, gains: [1.0, 2.5], name: "x", limit: None }'
$ ww explain 'motor.set_speed(1.5, Dir::Cw)'     # a call: its arguments
$ ww explain 'motor.limit = 3'                   # a property write
```

Literals take the type of their field: `5` is a `u16`, a `ub3` or an `f32` as the field says, and a value out of
range is an error naming the field (`argument a: 200 is out of range for ub7`). Struct literals (`Name { a: 1 }`,
`..Default::default()` for the rest), enum variants (`Mode::Fast` or `Fast`), `Some`/`None`, `Ok`/`Err`, tuples,
arrays (`[0; 4]`, `vec![..]`, `b"bytes"`, `0xa102` for bytes), ranges and negative numbers work. The values `ww`
prints (`ww introspect`, `ww observe`) are written the same way and can be pasted back; quantities go in `q!(..)`: `q!(3300 mV)`, `q!(1.2e-4 N*m/(rad/s))` (`*` for `·`, `u` for `µ`, a string for other symbols: `q!("20 °C")`), and a plain number is in the unit itself.

A whole message as it goes over the link, envelope and payload: `ww explain request HEX` for a request,
`ww explain event HEX` for an event (`--for PATH` names the method or property it answers; a stream's data names its
stream itself), and `--request` to see the request a call, a property write or a read is on the wire:

```
$ ww explain --crate api 'add(7, -2)' --request
Request Call add · 10 bytes

0000  01 01 00 07 00 00 00 fe  ff 01 

bits             raw                  field                      type         value
0‥79                                  ·                          Request      
0‥7              0000 0001              seq                      uvlq32       1
8‥15 ↩9l         0000 0001 ↩0001        path                     PathKind::Absolute [1]  (add)
16‥71                                   kind                     RequestKind  Call
16‥19            0000                     discriminant           nib          0
24‥71                                     args                   TailBytes    
24‥55            07000000                   a                    u32          7
56‥71            1111 1110 1111 1111        b                    i16          -2  0xfffe

72 bits in values, 8 padding or unread
```

The envelope (sequence number, path, request or event kind) is read with the `ww_client_server` types, the payload
inside it by the resource the path names. A path through a global trait id is shown but its payload is not
explained yet, nor are the multi requests and a level's property updates.

Front bits are counted from bit 7 of byte 0. Lengths and sizes are nibbles read from the back of the buffer (or of
the unsized value they are in), shown as `↩` with the byte and its half: `↩97h` is bits 7:4 of byte 97, `↩97l` bits
3:0. The API comes from the device, or without one from the crate defining it (`--crate PATH`, `--trait NAME` when
it has more than one) or a saved bundle (`--bundle FILE`, see [`ww api save`](#ww-api)). When the bytes don't decode,
the tree shows how far it got and where it stopped.

`ww explain FILE` with a capture file ([`ww capture`](#ww-capture)) prints the capture as [`ww monitor`](#ww-monitor)
printed it live, with the same `--layer`, `--bytes` and `--bits`; the capture carries the device's API, so no device
is needed. Replies are explained by the request their `seq` pairs them with.

## `ww monitor`

Prints what crosses the link as it happens, decoded ([The link tap](transport/tap.md), TOOL-14), from the link
setup on, until Ctrl+C or `--duration S`. `--layer` says how deep to look:

- `link` (default): every `ww_link` message (data, link setup, pings) with the frames it came in, and the link
  events; data messages with their request or event title, replies with the request they answer and its time.
- `frame`: every frame (USB packet, UDP datagram, RTT chunk) with its pieces as well.
- `api`: only requests, replies and events, with their values; `--bits` adds each message's bit map and field
  table, as [`ww explain`](#ww-explain) shows one.

```
$ ww monitor --layer api
   0.000000 L0 session Host Usb '3-1.2', U2 { checksum: None, tail: None, stream: false, prefix: None }, frames up to 64 B
   0.000055 L0 connected
   0.000280 L0 link ready
   0.065600 L0 → Request Call add  a: 5, b: 100 #3
   0.065955 L0 ← Event Value add  105 (reply to #3 in 355.0 µs)
```

The time is seconds since the first record, `L0` the link, `→` sent by the host and `←` received. `--bytes` adds the
bytes of every frame and message in hex. A frame line lists its pieces (`Full Data0 12 B`, `Start Data0 6/12 B`: 6 of
a 12 byte message's bytes), a message line the frames it is in (`in #3@58+6, #4@1+6`: 6 bytes at offset 58 of frame 3,
6 at offset 1 of frame 4). With a frame counter (UDP, USB with the `seq-prefix` diagnostic) a frame line shows its
prefix and what it says, `[15 in order]`, `[17 2 frames lost before]` or `[14 dropped: duplicate]`. Without a ww server, this is the command's own connection to the device: the link setup,
the reads it makes to connect, its pings.

### `--monitor` and `--capture` on any command

The tap of `ww monitor` and `ww capture` is also a pair of global flags of every command that opens a device
connection (`info`, `observe`, `stream`, `introspect`, `dfu`, `counters`, `report`, `usb-loopback`): they tap that
command's own connection, so the traffic of a real call is seen without a ww server.

- `--monitor[=frame|link|api]` (default `link`, the value needs `=`) prints to **stderr**, from the link setup on,
  as `ww monitor` does; the command's own output stays on stdout for scripts. `--monitor-bytes` and `--monitor-bits`
  are `ww monitor`'s `--bytes` and `--bits`.
- `--capture FILE` saves the same connection (frames included) to a new file, written as `FILE.partial` and renamed
  when the command ends, to read with [`ww explain FILE`](#ww-explain); it is refused when `FILE` exists
  (`ww capture --append` adds to one).

```
$ ww -l donguru-m1 --monitor=api info
...
   0.082878 L0 → Request Read ww_firmware_info::FirmwareInfo.FIRMWARE #3
   0.085630 L0 ← Event Value ww_firmware_info::FirmwareInfo.FIRMWARE  FirmwareSummary { name: "donguru", .. } (reply to #3 in 2.75 ms)
$ ww -l donguru-m1 --monitor=api observe meter
   0.120178 L0 → Request Observe meter  reading: on change; temperature: on change; totals: on change; others off #3
   0.123055 L0 ← Event Value meter  reading: on change (min 1 ms); temperature: on change (min 1 ms); totals: on change (min 1 ms); budget 2000 B/s (reply to #3 in 2.88 ms)
   0.124580 L0 ← Event PropertyUpdate meter  reading: MeterReading { vbus_in: 5228 mV, current: 7 mA }, temperature: 34000 m°C, ..
   0.134138 L0 ← Event PropertyUpdate meter  reading: MeterReading { vbus_in: 5229 mV, current: 6 mA }
```

Requests are named by the device's API, and calls through global traits (`ww info` reads `ww_firmware_info`,
`ww_board_info`, `ww_user_info`, ..) by the trait's crate and name, decoded from the snapshots `ww` carries, also when
the command never reads the device's own API. An `Observe` shows the property names with the mode asked and the one
granted, an update the properties it carries with their values. Ctrl+C and SIGTERM (`timeout`) end the command with the
capture finished and renamed.

Commands that don't connect (`list`, `explain`, `api`, `udev`, `elf`, `config`, `settings`) refuse the flags with an
error, and so do `ww monitor`, `ww capture` and `ww stats`, which are the tap themselves.

## `ww stats`

Framer and link statistics of the same records, every `--every S` seconds (1 by default) for the time since the last
print, until Ctrl+C or `--duration S`; `--file FILE` gives those of a whole capture instead:

```
$ ww stats --file link.wwtape
window 0.071 s, 5 blocks, 0 blocks lost, 0 records lost
link 0 tx:
  frames 7 (98.4/s), 136 bytes, fill 30.4%, 1.86 messages/frame
  overhead: prefixes 0 B, heads 26 B, checksums and tails 0 B, unused 312 B
  messages 13, 1 split over 2 frames
  message to frame: mean 926.0 µs, max 2.44 ms
  link messages: Data0 9, Disconnect 1, GetDeviceInfo 1, LinkSetup 1, Nop 1
link 0 rx:
  frames 10 (140.6/s), 418 bytes, fill 65.3%, 1.20 messages/frame
  overhead: prefixes 0 B, heads 25 B, checksums and tails 0 B, unused 222 B
  messages 12, 1 split over 5 frames
  link messages: Data0 9, DeviceInfo 1, LinkReady 1, Nop 1
link 0 api:
  Call add: 8 (112.5/s), reply mean 1.36 ms max 2.50 ms
  Call blob: 1 (14.1/s), reply mean 2.25 ms max 2.25 ms
```

Fill is the frame bytes used of what the frames could carry; trouble (dropped pieces, checksum failures, partial
messages dropped, frame gaps, framer mismatches) gets its own line when there is any. A link with a frame counter
(UDP, USB with the `seq-prefix` diagnostic) also gets a `frame prefix:` line: frames the medium lost, and those dropped
as duplicate, late, of the previous session, too short or resynced, and the new sessions it saw.

## `ww capture`

Saves what crosses the link into a file, until Ctrl+C or `--duration S`, with a spinner showing blocks and bytes:

```
$ ww capture -o link.wwtape --duration 10
captured 12 blocks, 48.2kB to link.wwtape
$ ww explain link.wwtape --layer api
```

The file is [tape blocks](transport/tap.md#capture-files) back to back, each written as it comes. A new capture is
written as `FILE.partial` and renamed to `FILE` when it ends; an existing `FILE` is refused unless `--append` adds to
it. `--sync` says when the data is synced to the disk, which bounds what a crash or a power cut can lose: an interval
(`1s`, the default, or `250ms`), `blocks:N` (every N blocks) or `none` (only at the end). A failed write or sync ends
the capture with the file as it is, never retried. The file carries the API bundles of the device and of the tap's own
records. `--layer link` or `api` leaves the frames out (smaller files).

A capture is read up to its first damaged block (a crash's cut last block, a flipped byte, a block out of order), and
where it was cut is said; zeros at the end (preallocated space) are the end. `--resync` on `ww explain FILE` and
`ww stats --file` reads on past damage to the next block instead. A `FILE.partial` left by a crash reads the same way.

## `ww usb-loopback`

Checks the link to the selected device in three stages, each with a progress bar, then prints a summary table:

1. `loopback`: one message at a time to the device and back, counting round trips per second, lost echoes (none
   within `--echo-timeout-ms`) and corrupted ones (each message carries data that checks itself).
2. `tx_speed`: host → device, the device discards the messages. An echoed marker every 32 messages (at most 2 in
   flight) keeps the host from running ahead of a slow device.
3. `rx_speed`: device → host, the device answers each request with 64 messages (at most 2 requests in
   flight); gaps in their numbering are lost.

The device's link layer answers (`Loopback` in [ww_link](transport/ww_link.md)), so this works with any device,
whatever its API, without downloading its introspection. The command exits with an error when anything was lost or
corrupted, or when the device stopped answering (3 timeouts in a row). The numbers above are defaults of the tuning
options in the table below (`--help` lists them under "Test tuning"), e.g. for a slow transport or a device with
small buffers.

| Option                 | Default | Description                                                                          |
|------------------------|---------|--------------------------------------------------------------------------------------|
| `--duration-sec <N>`   | `10`    | How long to run each stage                                                           |
| `--packet-size <N>`    | `max`   | Size of each link message (8 bytes of header and data), capped to what both sides accept |
| `--echo-timeout-ms <MS>` | `500` | An echo (round trip, marker or device → host reply) missing for this long is lost   |
| `--timeouts-in-a-row <N>` | `3`  | Give up after this many echo timeouts in a row                                       |
| `--marker-every <N>`   | `32`    | Host → device: messages between two echoed markers                                   |
| `--markers-in-flight <N>` | `2`  | Host → device: markers in flight at most, the host waits for their echoes beyond that |
| `--rx-batch <N>`       | `64`    | Device → host: messages the device sends for one request (at least 2)                |
| `--batches-in-flight <N>` | `2`  | Device → host: requests in flight at most                                            |

Lost or corrupted packets usually point at the hardware rather than the software: a bad cable, a malfunctioning
hub, a powered hub used without power, or power problems on an externally powered device.

## `ww dfu`

Firmware update steps through the [`ww_dfu`](std_library/ww_dfu.md) traits. These work with any device that
implements the trait somewhere in its API, whatever the API and its version: `ww dfu` connects without the user API
check and calls the trait through a [global trait path](api/addressing.md), which the device resolves itself. So a
`ww` built today can still reboot a device whose firmware is years older.

```
$ ww dfu reboot --serial 4B333720
blinky_api@0.1.0 reboots into the updater
```

| Command / option             | Default   | Description                                                                 |
|------------------------------|-----------|-----------------------------------------------------------------------------|
| `reboot`                     |           | `ww_dfu::BootControl::reboot`, then exits without waiting for the device    |
| `reboot --into <TARGET>`     | `updater` | `updater` (bootloader with a transport or recovery app) or `app` (normal reset, applies a staged update) |
| `flash <FILE>`               |           | A whole update of a [chunky](https://github.com/romixlab/chunky_mcu_updater) device: delta, trial, confirm (below) |
| `status`                     |           | `UpdateNode::node`: role, profile, product, layout, version, floor, seal, boot state, last update; `Stage` limits |
| `confirm`                    |           | `BootControl::confirm`: keep the trial image (the firmware normally does this itself after its self-test) |
| `revert`                     |           | `BootControl::revert`: give up the trial image (the old one runs after the next reset), or withdraw a staged one |

When a [ww hub](transport/hub.md) serves the device to other programs too, `reboot`, `flash`, `confirm` and `revert`
take it whole while they run ([exclusive access](transport/hub.md#the-whole-device-exclusive-access-and-lending-srv-32)):
other programs' calls and writes are refused naming `ww dfu`, and the hub doesn't steer the device's clock. `flash`
keeps it across the device's reset into the trial.

A device without `ww_dfu::BootControl`, or with an incompatible version of `ww_dfu`, answers that it doesn't implement
the trait. On an RP2350 whose updater is the boot ROM, [`ww bootsel flash --wait --follow`](#ww-bootsel) then
writes the new image and waits for the app to come back; a single command that follows any device through its
updater is not done yet (CLT-21). An A/B device whose app takes updates itself has no updater of its own: `reboot
--into updater` there goes to the chip's boot ROM (the B135's STM32 ROM DFU), and `ww dfu reboot` says so.

### `ww dfu flash`

Updates a device whose app serves `ww_dfu` with chunky's `chunky_ww` (scenario `2C-APP-STEP`), with the host library
`chunky_ww_host`: reads what the device holds (`Inventory::chunks`), plans a delta against it (chunks the device has
are copied on the device, only the rest goes over the wire), sends the steps with as many in flight as the device
takes and a progress bar, has the device verify and stage the image, reboots it into a trial, follows it by serial
number and waits until the new firmware confirms itself, or the device reverts to the old image.

```
$ ww dfu flash target/thumbv7em-none-eabihf/release/b135_fw -s 0B135A01   # abridged
note no --fw-version: sealed as 66052 (1.2.4), the running version + 1
plan 61 steps: 124 chunks copied on the device, 3 sent, 1 kept, 2 blocks erased
✔ staged in 1.9s: 7 KB over the wire for a 256 KB image
back running 66052 (1.2.4), Trial
✔ 66052 (1.2.4) runs and confirmed itself, 14.2s in all
```

The file is an ELF (detected by its magic bytes; its load segments must lie in the device's ACTIVE slot), an image
already sealed for the device (as long as ACTIVE, with its descriptor), or a raw binary payload. An ELF or a binary
is sealed for the device first: its product id and its seal scheme from `UpdateNode::node`, an image for another
product or scheme is refused before anything is sent.

| Option                 | Default                  | Description                                                                 |
|------------------------|--------------------------|-----------------------------------------------------------------------------|
| `--fw-version <V>`     | the running version + 1  | Release version, a number or `x.y.z` (packed `major << 16 \| minor << 8 \| patch`) |
| `--product <ID>`       | the device's             | Product id; another one than the device's is refused                         |
| `--counter <N>`        | the device's floor       | Anti-rollback counter                                                        |
| `--key <FILE>`         |                          | Release key (`chunky keygen`) for a device that checks Ed25519 signatures    |
| `--unsigned`           | for an `unsigned` device | Seal without a signature; a device that checks signatures refuses it         |
| `--chip <NAME>`        | by the device's layout id | chunky chip preset, for a layout this `ww` doesn't know                     |
| `--in-flight <N>`      | the device's             | Steps outstanding at most                                                    |
| `--no-reboot`          |                          | Stop after staging; `ww dfu reboot --into app` starts the trial              |

Each ERASE step waits for the slowest erase of one block of the part (an STM32H7 128 KB sector: up to 4 s, while a
single-bank part doesn't answer), the reboot for the bootloader's swap. When the device finds DFU changed under the
plan (`Error::Custom(1)`, chunky's `CUSTOM_NOT_BLANK`), the host reads it again and plans again; a lost reply is
resumed from `Stage::progress`.

## `ww counters`

The `cnt` counters of any device implementing [`ww_counters::Counters`](std_library/ww_counters.md) anywhere in its
API (a global trait, like `ww dfu`), decoded with the firmware's ELF found by the build id the device reports, and
shown with the same views as `cnt` over a debug probe.

```
$ ww counters --serial 4B333720
Using ELF target/thumbv8m.main-none-eabihf/release/usb_io_fw
📍 RAM counters at 0x20000000, 64 B
INFO  boots         3
...
```

| Command / option     | Default | Description                                                                         |
|----------------------|---------|-------------------------------------------------------------------------------------|
| `read`               | yes     | All counters once                                                                   |
| `watch [--interval MS]` | 100  | All counters, then each change until `q` or Ctrl+C (`reset` / `decrease` marked)   |
| `tui`                |         | Terminal UI, a table per buffer; `r` / `R` reset RAM / backup counters              |
| `reset [--bkp]`      |         | Zero the RAM counters, with `--bkp` the backup ones too                             |
| `info`               |         | Buffers, `cnt` format, build id, and the ELF that matches                           |
| `--elf PATH`         |         | The ELF to use; refused if its build id differs from the device's                   |
| `--force`            |         | Use the ELF anyway, or the newest under `target/` when no ELF matches               |
| `--format FORMAT`    | `text`  | `json` (one document) or `jsonl` (one object per counter, `watch` events)           |

Without `--elf`, the ELF comes from `WW_ELF_PATH`, the ELF directory `~/.cache/ww/elf` (filled by `ww elf add` or `cnt elf add`)
or `target/` of the cargo project in the current directory. See [ww_counters](std_library/ww_counters.md#on-the-host).

## `ww sync`

Keeps several devices' clocks on the host's UTC at once (TOOL-16), with the `ww_clock_sync` crate: each device's
`ww_timebase::Clock::now()` is read a few times a second, fitted against the host's monotonic clock (offset and
rate, from the readings with the shortest round trips) and steered with `Clock::steer`, ten times a second while the
device is acquiring and once a second once it says locked, see
[host sync](std_library/ww_timebase.md#host-sync-std-49) and its [states](std_library/ww_timebase.md#states). Works
with any device serving `Clock` with the `host_sync` group anywhere in its API; a device locked to a sync line only
takes note (source `line`). Devices with a shared clock align their animations to UTC, so their LEDs breathe in
phase ([`ww_indication`](std_library/ww_indication.md#animations-in-phase-std-50)). What it looks like (the values are
for illustration, not a measurement):

```
$ ww sync --mode normal
✓ B135 USB-FDCAN 3445 converged after 1.2 s: locked, residual -31 µs (within 2.00 ms)
✓ rePort bench-2 converged after 2.4 s: locked, residual +84 µs (within 2.00 ms)
✓ Donguru A1B2C3 converged after 6.3 s: locked, residual -620 µs (within 2.00 ms)
✓ all 3 within 2.00 ms after 6.3 s, LED phase 704 µs apart (±700 µs)
DEVICE                STATE   SOURCE  STEERED BY      RESIDUAL          ERROR    RATE            SLEW     RTT     SAMPLES  NOTE
B135 USB-FDCAN 3445   locked  host    ww pid 81234    -31 µs ±75 µs     95 µs    +4.612 ppm      +0 µs    150 µs  240
rePort bench-2        locked  host    ww pid 81234    +84 µs ±400 µs    480 µs   +12.880 ppm     +3 µs    800 µs  240
Donguru A1B2C3        locked  host    ww pid 81234    -620 µs ±700 µs   1.10 ms  -2003.204 ppm   -41 µs   1.40 ms 240
LED phase: 704 µs apart across 3 devices (±700 µs), 0.023 % of a 3 s breath
```

On a terminal the table is redrawn every second, with a line above it each time a device converges or falls out and
when all of them are within; into a pipe only those lines are printed (no colour), and the table once at the end.
Until Ctrl+C or `--duration`; then the devices go on in holdover on their last steer point.

With a [ww hub](transport/hub.md#clock-sync-in-the-hub-srv-31) running, the hub is the steerer: `ww sync` opens each
device through the hub (which then steers it), shows the hub's steering from its `sync().stats()` stream, and never
steers itself, so there is never a second steerer on the PC. `--poll-ms` and `--steer-ms` are then the hub's own
pace; after Ctrl+C the hub keeps steering the devices for its sync linger (30 min). Without a hub, or with
`WW_HUB=off`, `ww sync` steers directly as above. Either way it starts from the
[rate cache](std_library/ww_timebase.md#the-rate-cache), so a device this PC has seen before is steered after its
first reading.

| Column     | What it is                                                                                          |
|------------|-----------------------------------------------------------------------------------------------------|
| `STATE`    | the device's own `sync_status`: `acquiring` until it is within its lock tolerance, then `locked`     |
| `RESIDUAL` | the device's shared time minus the host's UTC at the middle of a `shared_now()` round trip, ± half that round trip: what the sync achieves, and the device's LED lead on UTC |
| `ERROR`    | the device's own error bound: the host's error estimate in the last point, plus slew left and holdover |
| `RATE`     | how much faster shared time runs than the device's clock (an HSI running 1 % fast is about -9900 ppm) |
| `SLEW`     | offset the device still slews in (5 % of its clock while acquiring, 1 % once locked)                |
| `RTT`      | the shortest round trip of the fit                                                                   |
| `STEERED BY` | whose steer points the device follows, by its own word (program, pid, and host when another PC); `NOTE` names a second steerer it only notes |

A device has converged once it says `locked` and its residual is within `--within-us`; all have converged once every
device that serves a clock has. A check whose half round trip is beyond `--within-us` (a USB hiccup) can't tell
either way and changes nothing. The LED phase is the spread of the residuals: every device starts its LED cycles on
UTC multiples of the period, so two devices' breaths are apart by the difference of their residuals.

| Option / argument | Default | Description                                                                          |
|-------------------|---------|--------------------------------------------------------------------------------------|
| `DEVICE...`       | all     | Serial numbers or user labels, or parts of them, each matching exactly one device    |
| `--mode MODE`     |         | Also write the indication mode: `all-off`, `only-essential`, `normal`               |
| `--duration S`    | Ctrl+C  | Stop after this many seconds                                                         |
| `--poll-ms MS`    | `200`   | Time between two readings of each device's clock once locked (100 ms before)        |
| `--steer-ms MS`   | `1000`  | Time between two steer points once locked (100 ms before)                            |
| `--within-us US`  | `2000`  | Residual within which a locked device counts as converged                            |

The device selection flags (`--product`, `--api`, ...) narrow which devices `DEVICE...` is matched against.

## `ww hub`

Looks at and steers the per-user [ww hub](transport/hub.md#the-ww-hub-command-tool-17) (TOOL-17): the process that
holds devices and serves every program, and the question "who has my device?" for any device, hub or not.

```
$ ww hub status
ww hub 0.5.0 running
built with wire_weaver 0.5.0
pid        4711
exe        /home/me/.cargo/bin/ww
socket     /run/user/1000/ww/hub.sock
started by dg (pid 4690), up 12m 31s
clients    2 programs, 2 connections
devices    3 known, 1 open through the hub
idle exit  after 5m 00s without connections, counted from the last program leaving

KEY                             PRODUCT         STATE        CONNECTIONS
usb:c0de:cafe:18AC50C211FFB7C1  b200_usb_io     open (hub)   1
usb:c0de:cafe:204B319B5431      donguru         free         0
usb:c0de:cafe:344533413431      B135 USB-FDCAN  held by dg (pid 4690, direct)  0
```

| Command                 | Description                                                                                          |
|-------------------------|------------------------------------------------------------------------------------------------------|
| `serve [--idle-exit S]` | Run the hub in the foreground (its log on stderr) until idle or stopped, for watching or debugging; exits at once if a hub runs already |
| `status`                | Version, process, who started it, uptime, programs, devices, idle timers; exit code 1 when no hub runs |
| `stop`                  | Stop the hub and wait until it is gone; says so when none runs                                       |
| `who [DEVICE]`          | Every device and who holds it: the hub, a ww program using it directly (from its lock file), or on Linux any other program (from `/proc`); `DEVICE` is a key, serial, label or product. With a hub running, the programs connected to it follow |
| `keep [DURATION]`       | Keep the hub up without programs, for a bench: `30m`, `2h`, `1d`, plain seconds, `forever` (the default) or `off` for the idle timeout again; needs a running hub |
| `lend DEVICE -- CMD`    | Lend the device to a program that isn't a WireWeaver one (probe-rs, dfu-util): the hub closes it, runs `CMD`, opens it again after; Ctrl+C reaches `CMD`, its exit code is `ww`'s. Without a hub `CMD` just runs ([more](transport/hub.md#the-whole-device-exclusive-access-and-lending-srv-32)) |

`ww list` asks a running hub instead of USB: the same columns plus `STATE` (`free`, `open (hub)`, `held by <program>
(pid ..)`), so a device somebody holds shows up too. `--direct` enumerates USB anyway; device selection flags and
`--all` always do.

```
$ ww hub lend 0B135A01 -- probe-rs reset --chip STM32H725
ww hub usb:c0de:cafe:0B135A01 lent to `probe-rs reset --chip STM32H725`, the hub opens it again after
...
ww hub usb:c0de:cafe:0B135A01 returned to the hub
```

Colour is off when the output isn't a terminal or `NO_COLOR` is set. Shell completion gives the subcommands, the
`keep` durations and, for `who`, the keys of the plugged-in devices, each with its product and label as help text.

## `ww info`

Firmware, board, ids, user label and last reset of any device, through the [device
contract](std_library/device_contract.md) traits (global trait paths, like `ww dfu`: whatever the device's API and
version, no ELF needed).

```
$ ww info --serial 4B333720
API        usb_io_api@0.5.0
Firmware   b200_usb_io 0.5.0 (a1b2c3d-dirty, built 2026-10-03 18:20), release
Build id   28a4e144e867138da2cccccd14e3e6baa2c78fbc
Board      rePort rev B (strap 2), BOM std, serial RP-0042
Ids        chip e6613871235b1c2d (internal, primary)
Label      "bench 2" (kept over power cycles)
Last reset Panic, crash data 2 boots old, boot 3 since cleared, ran 2.5 s
```

| Option            | Default | Description                                                                          |
|-------------------|---------|--------------------------------------------------------------------------------------|
| `--format FORMAT` | `text`  | `json` for scripts; each part says `value`, `not_implemented` or `error`             |

A part the device doesn't implement says so, the rest is still shown.

## `ww report`

`ww info` plus the counters, the device's stored `defmt` log and the last reset's fault and stack addresses named
with the ELF, as one text a tester sends with a bug report. The ELF is found by the build id the device reports
([where](std_library/device_contract.md#where-ww-looks-for-the-elf)); a part whose ELF isn't found says so and
names the places searched.

```sh
ww report --serial 4B333720 -o report.md
```

| Option             | Default | Description                                                                         |
|--------------------|---------|-------------------------------------------------------------------------------------|
| `-o`, `--output F` | stdout  | Write the report to a file                                                          |
| `--elf PATH`       |         | The ELF to use for everything; refused if its build id differs from the device's    |
| `--force`          |         | Use the ELF anyway                                                                  |
| `--format FORMAT`  | `text`  | `json` for scripts                                                                  |

Reading the log does not empty the device's store.

## `ww pins`

Pin tests on any device serving [`ww_pin_diag::PinDiag`](std_library/ww_pin_diag.md), wherever in its API: found
through the device's introspection data (`--at diag.pins` picks one when there are several). `ww pins list` shows
the points and what each can do, `ww pins test` tests them:

```
$ ww pins test GPIO0..=GPIO2,DET,LED --plan b200a.toml
ID  NAME   GROUP  PULLS                    GND          SUPPLY       SHORTED TO  NOTES
P0  GPIO0  mcu    floating ✗ want pull-up  ok           ok           GPIO1
P1  GPIO1  mcu    floating                 ok           ok           GPIO0
P2  GPIO2  mcu    held low                 SHORT        ok           ok
P3  DET    mcu    not checked              not checked  not checked  ok          no internal pulls; can't drive high; can't drive low
P4  LED    mcu    refused                  refused      refused      refused     in use by indication
5 points: 1 short(s) to GND or the supply, 1 short(s) between points, 1 expectation(s) not met
```

Per point, only with what its caps allow: **pulls** (input with the pull-up, then the pull-down, each after driving
the opposite level: external pull-up, pull-down, floating, or held high / low when the point also is shorted to that
rail), **GND** (drive high weakly, read back: low is a short), **supply** (drive low weakly, read back: high is a
short), then **shorted to**: each point driven low alone with the others on pull-up, then high alone with the others
on pull-down; who follows is connected to it. Outputs that can't be read (shift register outputs) are sources only,
so `ww pins test 'chain.*,mcu.*'` shows which MCU pins each output reaches. A check a point can't do says *not
checked*, a refusal of the firmware (`not capable`, `in use by ...`, `refused: <rule>`) says *refused*, with the
reason in NOTES; neither stops the test. Every point is given back with `release_all` at the end, also on Ctrl-C and
errors. The exit code is 0 when nothing is shorted and, with a plan, every expectation is met, 1 otherwise.

| Command / option | Description                                                                                       |
|------------------|---------------------------------------------------------------------------------------------------|
| `list [--format json]` | Points with id, name, group and caps (`read`, `pull_up`, `pull_down`, `drive_low`, `drive_high`, `weak_drive`) |
| `test POINTS`    | Points to test, comma separated: names (`GPIO0`), ids (`P8`), ranges (`GPIO2..=GPIO7`, `GPIO2..GPIO8` without the end, `P2..=P7`), globs on names (`GPIO1*`) and on groups with names (`'chain.*'`). A name wins over an id (a point named `P8` is that point); a part that matches nothing is an error. Without points: the plan's, without a plan all |
| `--watch POINTS` | Points only read, never set: their level, and whether they follow the tested points             |
| `--plan FILE`    | Expectations, narrower allow lists and nets, see below; its `target` is the default for `--at`   |
| `--save FILE`    | Write the pull class each tested point shows into `expect` of FILE (created if missing)          |
| `--at PATH`      | Where `PinDiag` is in the device's API, by item names (`diag.pins`), when it has more than one   |

A plan file lives in the firmware repo, one per board revision:

```toml
target = "diag.pins"

[[point]]
names = ["GPIO3..=GPIO7"]      # names, ids, ranges and globs, as on the command line
expect = "pull_up"             # pull_up, pull_down, floating, held_high, held_low
[[point]]
name = "GPIO2"
expect = "floating"
[[point]]
name = "GPIO0"
allow = ["read"]               # read, pull_up, pull_down, drive_low, drive_high: only narrows

[shorts]
among = ["GPIO0..=GPIO8"]      # the points tested against each other (default: all tested points)
same_net = [["GPIO26", "GPIO34"]]  # one net: connected is right, not connected is a fault
```

An entry naming a point exactly wins over a range or glob for `expect`, a later entry over an earlier one; the
`allow` lists of all matching entries apply. `--save` on a known good board fills `expect` (an existing file keeps
its comments and order; a point is taken out of a `names` list whose `expect` differs, into its own entry), after
which `ww pins test --plan FILE` is the pass/fail test. Connections between points are not saved: put real nets into
`same_net` by hand.

Point names complete live from the device (selected by env variables and `ww.toml`). Names a firmware sends as
string ids ([`ww_interned`](std_library/ww_pin_diag.md#ww_interned)) show as `#17` and are selected that way; looking
them up in the firmware's ELF is pending (STD-48).

## `ww elf`

The ELF directory (`~/.cache/ww/elf`, or the ones in `WW_ELF_PATH`), where firmware ELFs are found by build id for
`ww counters`, `ww report` and `cnt`.

```sh
ww elf add usb_io_fw-0.5.0-elf.tar.zst   # every ELF of a release, stored as <build id>.elf
ww elf add target/release/usb_io_fw      # one ELF
ww elf id target/release/usb_io_fw       # its build id, and whether it is in the directory
```

`--dir DIR` adds to another directory. A file without a build id is listed as skipped. See the [release
archive](std_library/device_contract.md#the-release-archive).

## `ww settings`

`ww settings dump` decodes a `ww_settings::Store` flash dump: no device needed, just the raw bytes of the store's
flash range (read off the chip, from a `.bin` or a debug probe dump). The word and page size aren't in the dump
itself, so they are given on the command line (completion suggests the sizes `Store` supports, with a chip example
each):

```
$ ww settings dump settings.bin --word-size 8 --page-size 2048 --base 0x08078000
2 pages, word 8 bytes, page 2048 bytes, base 0x08078000
page 0 @ 0x08078000: page_seq 1
  0x08078010 +48     valid, stale key 0 seq 1 len 30
  ...
  0x080787f0 +16     erased
page 1 @ 0x08078800: page_seq 2 (current)
  0x08078810 +24     valid, stale key 1 seq 28 len 10
  ...
  0x08078b40 +48     valid, current key 0 seq 40 len 30
  0x08078b70 +24     valid, current key 1 seq 40 len 10
  0x08078b88 +1144   erased

current record per key:
  key 0 @ 0x08078b40 (page 1), seq 40, 30 bytes: 27272727...
  key 1 @ 0x08078b70 (page 1), seq 40, 10 bytes: d8d8d8d8...
```

Every item slot is shown, valid or not (`cut`: a write a power loss interrupted; `garbage`: neither a valid item
nor erased), including on a page whose header is itself invalid, which a device's own `Store` never looks at. The
"current record per key" section is cross-checked against a real `Store` run over the same bytes, so it is exactly
what the device would read; a mismatch is a bug in the decoder, not a sign of a bad dump, and is reported as an
error. `--format json` gives the same information as JSON instead.

## `ww bootsel`

RP2040 and RP2350 boot ROMs in BOOTSEL mode, over PICOBOOT: no WireWeaver on the device, no picotool or libusb on
the host. Used to flash an RP2350 whose app rebooted itself into the boot ROM (`ww_dfu::BootControl::reboot`, see
[`ww dfu`](#ww-dfu)), and to follow it back into the new firmware.

```sh
ww bootsel list                                   # boot ROMs in BOOTSEL mode with their serial numbers
ww bootsel info -s E661                           # chip id (RP2350)
ww bootsel flash app.uf2 -s E661                  # erase, write, verify, reboot into the new image
ww bootsel flash target/thumbv8m.main-none-eabihf/release/app -s E661  # an ELF works too, no objcopy needed
ww bootsel flash app.uf2 -s E661640843522C2F --wait 10 --follow
ww bootsel reboot -s E661 [--bootsel]             # reboot (RP2350), into BOOTSEL again with --bootsel
```

| `flash` option        | Default      | Description                                                                 |
|-----------------------|--------------|-----------------------------------------------------------------------------|
| `--addr <ADDR>`       | `0x10000000` | Flash address of a raw binary (any file that isn't `.uf2` or an ELF)        |
| `--family <F>`        | `rp2350`     | UF2 family to take blocks of: `rp2350`, `rp2040` or `any`                   |
| `--wait <SECONDS>`    |              | Wait this long for the BOOTSEL device with the `--serial` to appear         |
| `--follow`            |              | After the reboot, wait for a WireWeaver device with the same serial and show it |
| `--no-reboot`         |              | Don't reboot after writing                                                  |

The file is a UF2 (by extension), an ELF (by its magic bytes, whatever the extension — `target/.../release/app`
works straight from `cargo build`, no `objcopy`/`elf2uf2`), or otherwise a raw binary written at `--addr`. An ELF
is taken by its `PT_LOAD` segments' physical (flash) address; a segment that isn't in flash (a RAM-only image) is
an error.

`--serial` selects the device as for other commands; part of it is enough when only one matches, `--wait` and
`--follow` need the full one. See [following a reboot](transport/follow_reboot.md).

## `ww udev`

Linux only. Meant to print a udev rule giving your user access to WireWeaver USB devices
(`ww udev | sudo tee /etc/udev/rules.d/70-ww_device.rules`), but not implemented yet: the command stops with a
`todo!()` panic. Until then write the rule by hand, see [USB](transport/usb.md).

## `ww config`

Manages the [project config file](cli.md#project-config-wwtoml):

```sh
ww config save --api blinky_api@^0.1 --serial 4B333720  # write flags into ww.toml (comments are kept)
ww config show                                         # resolved selection and where each setting comes from
ww config unset serial                                 # remove settings from ww.toml
```

`ww config save` writes into the `--config` file if given, otherwise into the `ww.toml` found in the current
directory or its closest parent, otherwise creates `ww.toml` in the current directory. Only flags from the command
line are saved, never environment variables, so a `WW_*` variable exported in a shell profile does not end up in
project files.

