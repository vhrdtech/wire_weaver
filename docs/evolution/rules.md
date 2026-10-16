---
level: basic
---

# Evolution rules

Two different things can break when a WireWeaver crate changes, and they are tracked separately:

* **Wire compatibility** - whether bytes written by one side can still be read by the other. Firmware lives in the
  field for years, hosts are updated on their own schedule, and shrink_wrap blobs may be stored on disk. A wire break
  means deployed devices stop talking to new hosts (or the other way around), which no `cargo update` can fix.
* **Rust (SemVer) compatibility** - whether code still compiles against the new crate version. A break here is
  annoying, but it is found by the compiler and fixed by the developer, before anything ships.

A change can break one without the other: renaming a `BufWriter` method breaks Rust code but not a single byte on the
wire; changing a field's `u8` to `u16` in an API type compiles fine and silently breaks the wire. The rules below say
what you can change in your API crate without breaking either, and which version to bump when you do.
For changing WireWeaver itself or a `ww_stdlib` crate, see [crate layers and version bumps](crate_layers.md).

## In short

A device runs `blinky_api 0.1.0`. The next version adds a method and a field, and keeps everything else:

```rust
#[ww_api_root]
pub trait BlinkyApi {
    fn led_on();
    fn led_off();
    #[since = "0.1.1"]
    fn led_toggle();
}

#[derive_shrink_wrap]
struct Status {
    on: bool,
    #[since = "0.1.1"]
    #[default = None]
    brightness: Option<u8>, // old data reads as None
}
```

Both are wire-compatible additions, so the crate goes from `0.1.0` to `0.1.1`: an old device and a new host still
connect, the host gets a clean error when it calls `led_toggle` on the old device, and reads `None` for the
brightness it doesn't send. The rest of this page lists which changes are compatible like these. When in doubt,
run the [evolution checker](checker_tool.md), it compares your crate with its last release.

## Your API's version

Name and version of the user API crate are sent during link setup, and both device and host refuse to connect if
they are not compatible (`ww_version::Version::is_protocol_compatible`):

* Before 1.0: major and minor must match, so `0.2.0` and `0.2.7` connect, `0.2.0` and `0.3.0` don't.
* After 1.0: major must match.

So the compatible position (patch before 1.0, minor after) is for wire-compatible additions: new methods,
properties, streams, new fields with defaults, etc. Mark them with `#[since = "0.2.3"]`, so that a newer host can
report a clean error when calling a resource that an older device doesn't have (see `examples/blinky_api_evolved`).
Everything else bumps the breaking position.

The API crate's dependencies are part of this contract: upgrading a `ww_stdlib` crate or `shrink_wrap` to a
wire-incompatible version, or [the API model](#api-model), is a breaking change of the user API too.

### Doc comments

Doc comments on traits, resources and types are not part of the wire format, but they are shipped in introspection
data and in [crate snapshots](../cli_reference.md#ww-api), so a GUI or CLI can show them. A released version's snapshot never
changes, so a doc-only change is still a new version: bump the compatible position (patch before 1.0, minor after),
same as for a wire-compatible addition. Devices with the old version keep connecting, and tools can tell that
improved docs are available. `ww api save` refuses to overwrite a snapshot with changed docs, and the
[evolution checker](checker_tool.md) requires the compatible bump.

## Data types

WireWeaver considers two root types for evolution and evolution rule checks - `struct` and `enum` (plain and with data
variants).
Size of a type is marked as one of: `Unsized`, `FinalStructure`, `SelfDescribing` or `Sized`.

### Unsized types

By default, user defined struct or enum is `Unsized`. Both can contain variable-size types - vectors,
strings or other structs and enums. Unsized types support all the evolution options. It is recommended to stick with
unsized types, unless extreme space-saving is required.

When serializing, size of such objects is calculated and written to the resulting byte array. Which is the only
overhead, giving all the nice backwards and forwards compatibility benefits.

* New fields with `#[default = ..]` can be added to the end of structs and enum struct variants. Old data reads as
  the default (see [derive](../serdes/derive.md#evolving-types)), `T` can be anything:
    * `Option<T>` - `#[default = None]`,
    * `Vec<T>`, `String` - `#[default = Vec::new()]`, `#[default = String::new()]`,
    * a bit-sized field (`bool`, nibble, `Option` / `Result` flag) can land in padding bits of the old data's last
      byte, which read as 0: give it its zero value as the default.
* New enum variants can be added at the end. Readers of older versions fail on them unless the enum has a
  [`#[future_version]` variant](../serdes/derive.md#enum-variants-from-a-newer-version); with one, they get its
  discriminant. Every `Unsized` enum has one unless it says `no_future_version` (SW-47). In an `Unsized` enum new
  variants may carry data, in a `sized` or `self_describing` one only new unit variants can be read past by older
  readers. Adding or removing `no_future_version` changes the Rust type, not the wire: a Rust API break only.
* New fixed size fields can be added in between old ones, into previously unused padding bits, without moving any
  other field (see [the checker](checker_tool.md#new-fields-in-padding-bits) for exact conditions).
* TODO: clarify: `T` -> struct containing `T`
* Struct fields and enum variants can be renamed (but their position must NOT change).

A `TailSize<N>` field (see [wire format](../serdes/special_types.md#tailsizen-the-size-of-the-rest-of-a-value)) is part
of the layout like a `Sized` field: adding, removing or moving it, or changing `N`, breaks compatibility; fields
after it evolve as usual, and old readers skip what newer writers append.

A `Delta<T>`, `DeltaOfDelta<T>` or `XorFloat<T>` field (see
[wire format](../serdes/special_types.md#delta-deltaofdelta-and-xorfloat-compressed-sequences)) is like a `Vec<T>`
field: it can be appended with `#[default = Default::default()]` (old data reads as an empty sequence), but
swapping it for a `Vec<T>` or another of the three, or changing `T`, changes the bits and breaks compatibility.

### Maximum lengths: `MaxLen<T, N>`

A [`MaxLen<T, N>`](../types.md#bounded-lists-and-strings-maxlent-n) has the same bytes as `T`, so a bound never
changes the wire, only which values the reader accepts (a longer one is an error, never truncated). Whether a change
is compatible depends on what the old version writes:

* **Raising a bound or removing it** (`MaxLen<T, 64>` -> `MaxLen<T, 256>` or `T`): compatible. Everything the old
  version writes is still read. The checker lists it as `max length raised`, because values the new version writes
  beyond the old bound are rejected by the old version (with an error): a new client sending a long piece to an old
  device gets an error reply, the same as any argument the device refuses.
* **Lowering a bound or adding one** (`T` -> `MaxLen<T, 64>`, `MaxLen<T, 256>` -> `MaxLen<T, 64>`): breaking. Values
  the old version is allowed to write no longer fit, and nothing on the wire tells the old version so.
* Changing `T` inside the bound follows the usual rules for `T`.

At connect time the client does not refuse a device whose bounds differ from its own: a longer value is rejected when
it is sent, by whichever side has the smaller bound.

### Compressed sequences: `Delta`, `DeltaOfDelta`, `XorFloat`

Each has bytes of its own, so `Vec<T>` -> `Delta<T>`, `Delta<T>` -> `DeltaOfDelta<T>` or `Delta<u32>` ->
`Delta<u64>` is breaking (`ww api check` reports it), unlike the borrowed / owned form (`Delta<'i, T>` /
`DeltaOwned<T>`), which is the same type.

### Sparse structs

A [`sparse` struct](../serdes/layouts.md#sparse-a-struct-of-optional-fields) (DER-13) has a slot per field, all of
them `Option<T>`:

* New fields can be appended after the last slot, without `#[default]`: older data reads them as `None`, older
  readers ignore them.
* A field can't be inserted before the last slot, also not into a gap left by `#[slot = N]`: a reader that doesn't
  know the slot can't step over a present value in it and fails with `Error::OptionAnyVecUnknownSlot`. Removing a
  field breaks too: data from an older writer can still have it present.
* A field's slot and its `T` can't change (`T` evolves by its own rules); fields can be renamed.
* Turning an ordinary struct into a `sparse` one or back changes every byte: breaking.

### FinalStructure, SelfDescribing and Sized types

* `Sized` can become `SelfDescribing` and back (`sized` <-> `self_describing`): neither gets a size slot from its
  parent, the bytes are the same, and the checker reports it as compatible. A `sized` enum must have variants of one
  size (see [derive](../serdes/size_assumptions.md)).
* New fixed size fields can be added in between old ones, into previously unused padding bits, without moving any
  other field (see [the checker](checker_tool.md#new-fields-in-padding-bits) for exact conditions).
* Struct fields and enum variants can be renamed (but their position must NOT change).

## API

Data types used in API are a part of SemVer guarantee and are subject to the rules above. Meaning that it's not allowed
to break compatibility on any of the data types used directly or indirectly without also bumping the breaking position
of the API version as well (minor before 1.0, major after, see [your API's version](#your-apis-version)).

* Adding argument
* `T` to struct of `T` in return position
* `T` to `Vec<T>` in return position?

Streams (API-30, API-31, [stream reliability](../api/stream_reliability.md)): the delivery kind of a stream is part
of the API. Turning a plain `stream!` into a `#[counted]` or `#[reliable]` one (or back, or one into the other), or
giving a stream arguments or a result (`#[stream(T)] fn name(args) -> Result<A, E>`), changes its events and how it is
opened, so it is a break of the API crate: bump its breaking version position, as for a plain sink becoming a
`#[credit]` one. A plain `#[stream(T)] fn name()` without arguments and result is the same as `stream!(name: T)`, the
same bytes in introspection.

### API model

API model (like `ww_client_server`) is part of the compatibility equation, it is not allowed to update the model
version without breaking compatibility.

E.g., if user_device_api v0.1.0 depends on ww_client_server v0.4.0 and a new major version of ww_client_server comes
out (v0.5.0), then user_device_api must be bumped to v0.2.0 to use the newer API model. This should only happen to add
new features though, and if previous version is doing all that it is supposed to, there might not be a need to
upgrade.

## Settings stored on a device

A [settings store](../api/settings.md) (`ww_settings`, STD-32) keeps a record per group in the device's flash, and
the record outlives the firmware that wrote it: an update reads the old firmware's record, a downgrade the new one's.
Each field has a slot, each stored field is a byte blob of its own, so every firmware can step over a slot it
doesn't know. That makes the rules looser than for a [sparse struct](#sparse-structs):

* **Adding a field**: any slot that was never used, also one in a gap between older slots. Older records read it as
  its default. An older firmware keeps it unchanged when it writes its record.
* **Removing a field**: put its slot into `#[settings(retired(..))]`; the derive then refuses to give it to another
  field. A record that still holds it is read without it, and the slot is dropped on the next write, so a downgrade
  to a firmware that still has the field finds it at its default.
* **Changing a field's type**: only the way the type itself may evolve (a struct gets fields with `#[default]`
  appended, an enum gets variants). An older firmware reads what it knows of the newer value (a struct's first
  fields, an enum variant it doesn't know as `Unknown(n)`), and as long as it doesn't change the field, it writes the
  newer bytes back unchanged; if it changes the field, the parts it doesn't know are gone. Bytes it can't decode at all
  (an enum with `no_future_version`) are kept while the field stays at its default. Any other type change, or a
  change of what the value means (units, scale): a new slot, and the old one retired.
* **Changing a default**: a stored record holds only the fields that differ from their default, so every device that
  never set the field takes the new default, and so does one that set it to exactly the old default. When that is
  wrong, the field needs a new slot.
* **Renaming a field** is fine; the slot is what is stored. Moving a field to another group (another `Key`) is a
  new field there and a retired one here; copying the value over is the firmware's job on load.
* The **record format** (`ww_settings::record::FORMAT`) and the **store layout** (the page and item headers in
  `ww_settings::store`) are defined by `ww_settings` itself: a change to either is a breaking change of
  `ww_settings`, as for `ww_framer` in [layer 3](crate_layers.md#layer-3-framework). A firmware that finds a record in a format it
  doesn't know leaves it alone (`is_read_only`) instead of overwriting it.
