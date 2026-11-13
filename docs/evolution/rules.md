---
level: advanced
---

# What can change

Every rule on this page answers the same two questions about a change:

1. Can each version still read what the other one writes? If not, the change breaks the **wire**: a new
   [wire line](versions.md#the-wire-version).
2. Does Rust code written against the old crate still compile? If not, the change breaks the **crate**: its breaking
   version position.

A change that passes both is an addition: a new wire revision, the crate's compatible position. Which bump each
outcome needs is in [which bump a change needs](versions.md#which-bump-a-change-needs), and the
[evolution checker](checker_tool.md) applies every rule here for you. This page is for knowing why it says what it
says, and for planning a change before making it.

The short version: add at the end, with a default; don't remove, reorder or retype what is out there; a change of
meaning is a break even when the bytes stay. For changing WireWeaver itself or a `ww_stdlib` crate, see
[crate layers](crate_layers.md).

## Data types

WireWeaver considers two root types for evolution - `struct` and `enum` (plain and with data variants). How a type
may change depends on its [size assumption](../serdes/size_assumptions.md): `Unsized` (the default),
`FinalStructure`, `SelfDescribing` or `Sized`. The first can grow, the others trade that away for bytes.

### Unsized types

By default, a struct or enum is `Unsized`. It can contain variable-size types: vectors, strings, other structs and
enums. When serializing, its size is written next to it, and that is the only overhead: it buys all the backwards and
forwards compatibility below. Stick with unsized types unless extreme space-saving is required.

**Fields can be appended.** New fields with `#[default = ..]` can be added to the end of structs and enum struct
variants. Old data reads as the default (see [derive](../serdes/derive.md#evolving-types)), and old readers skip the
new field. `T` can be anything:

* `Option<T>` - `#[default = None]`,
* `Vec<T>`, `String` - `#[default = Vec::new()]`, `#[default = String::new()]`,
* a bit-sized field (`bool`, nibble, `Option` / `Result` flag) can land in padding bits of the old data's last byte,
  which read as 0: give it its zero value as the default.

**Fields can fill padding.** New fixed-size fields can be added in between old ones, into previously unused padding
bits, without moving any other field (the exact conditions are in
[inside the checker](inside_the_checker.md#new-fields-in-padding-bits)).

**Variants can be appended.** New enum variants can be added at the end. Readers of older versions fail on them
unless the enum has a [`#[future_version]` variant](../serdes/derive.md#enum-variants-from-a-newer-version), the
catch-all; with one, they get its discriminant. Every `Unsized` enum has one unless it says `no_future_version`
(SW-47).

* In an `Unsized` enum new variants may carry data. In a `sized` or `self_describing` one only new unit variants can
  be read past by older readers.
* Adding or removing `no_future_version` changes the Rust type, not the wire: a Rust API break only.
* The checker knows the catch-all (EVO-10): a variant appended to an enum with one is a compatible change. One
  inserted before others, or a reordering, moves a variant to another discriminant and stays breaking, as does any
  new variant of an enum without the catch-all.
* The API model records the catch-all since EVO-10, so a snapshot saved before it counts as one without: save it
  again (`ww api save`) for appended variants to pass.

**Names can change, positions can't.** Struct fields and enum variants can be renamed, but their position must not
change. Python and `ww` scripts that name them change with them.

**Variants are never removed.** Rust code naming one breaks, so the checker asks for a breaking crate bump. With a
catch-all the wire keeps (a reader of the new version takes an older writer's value of it as the catch-all), so the
wire version needs no bump of its own; without one it is a wire break too.

**Tuple and unit structs** follow the same rules. A tuple struct (`struct Pair(u8, u16);`) goes by position: new
fields with `#[default = ..]` only at the end. Naming the fields of a tuple struct, or the other way round, keeps
every byte, but the API model then describes a different type (named instead of unnamed fields), which the checker
reports as breaking. A unit struct (`struct Marker;`) can get fields with defaults later, named or not.

**Some fields are part of the layout.** A `TailSize<N>` field (see
[wire format](../serdes/special_types.md#tailsizen-the-size-of-the-rest-of-a-value)) is part of the layout like a
`Sized` field: adding, removing or moving it, or changing `N`, breaks compatibility; fields after it evolve as usual,
and old readers skip what newer writers append.

A [`Series<T>`](../serdes/series.md) field is like a `Vec<T>` field: it can be appended with
`#[default = Default::default()]` (old data reads as an empty series), but swapping it for a `Vec<T>`, or changing
`T` other than as `T`'s own rules allow, changes the bits and breaks compatibility.

### FinalStructure, SelfDescribing and Sized types

These types are packed tightly into their parent, with no size of their own, so they can't grow at the end. What is
left:

* `Sized` can become `SelfDescribing` and back (`sized` <-> `self_describing`): neither gets a size slot from its
  parent, the bytes are the same, and the checker reports it as compatible. A `sized` enum must have variants of one
  size (see [size assumptions](../serdes/size_assumptions.md)).
* New fixed-size fields can be added in between old ones, into previously unused padding bits, without moving any
  other field (see [inside the checker](inside_the_checker.md#new-fields-in-padding-bits) for the exact conditions).
* Struct fields and enum variants can be renamed (but their position must not change).

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

### Transparent wrappers, units and scales

The trickiest changes keep every byte and still break. A [`transparent`](../serdes/derive_reference.md#transparent)
struct (DER-18) is exactly its one field on the wire, and `ww_si::Fixed<U, P, T, B>` (STD-55) is one: a `T` with its
unit `U`, prefix or scale `P` and bound `B` in the type. The checker compares it by what it means, not only by its
bytes:

* **`T` -> a wrapper of `T`, or back** (`shared_us: u64` -> `shared: Fixed<Second, Micro, u64>`): compatible, the
  bytes are the same. The checker lists it as `u64 -> Fixed<Second, Micro, u64, Unbounded>, same bytes`. Renaming the
  field with it (dropping the `_us`) is compatible as well (fields are matched by position), but Python and `ww`
  scripts that name the field change with it.
* **Another unit, prefix or scale** (`Fixed<Second, Micro, u64>` -> `Fixed<Second, Milli, u64>`): breaking, although
  the bytes are the same: a reader of the old version would take a value in ms for µs. Units and scales are compared
  by value, so another name for the same one is compatible (`Milli` -> `Pow10<-3>`, `Fixed<Volt, Milli, ..>` ->
  `Fixed<MilliVolt, One, ..>`), and lbf·s -> N·s is breaking with the factor in the finding (EVO-17, see
  [units compared by meaning](units_by_meaning.md)). Origins of points are compared by where their zero lies. The
  same for one wrapper replaced by another wrapper of the same bytes (`Fixed<..>` -> a `Count<u64>` of your own).
* **Another `T`** (`u32` -> `u64`, inside the wrapper or across it): breaking, as for plain numbers.
* **`Quantity<U>` -> `Fixed<U, ..>`**: breaking, a `Quantity` sends its prefix and number type with every value, a
  `Fixed` only the number. It is the intended move for values whose prefix and type never change, so the checker says
  so in the finding: plan it for a major version.

`Fixed<Second, Micro, u64>` and `Fixed<Second, Micro, u64, Unbounded>` are the same type (the model always records
`B`), and so is an alias of either (`Micros<u64>`). `Fixed` in your API is a `ww_si` type the API model skips; the
checker reads its definition from the `ww_si` snapshot built into it, so a field changed from `u64` to `Fixed` is
checked the same way as a wrapper of your own. The same for a wrapper of any other crate with a built-in snapshot
(`ww_block::Lease`, a `u32`, as an argument of `ww_sd`). When that snapshot doesn't hold the wrapper (saved before it
was added, for a version not published yet), the check stops with an error naming it: save that crate's snapshot
again (`just save-snapshots` for `ww_stdlib`).

This holds for every generic type, not only `Fixed`: a type parameter that no field uses (one only in `PhantomData`,
which the model doesn't have) is a marker that gives the bytes a meaning. A marker with a definition in the model (a
unit, scale or origin, INT-13) is compared by that definition; any other marker by name, with its own arguments
(`Count<Apples>` -> `Count<Pears>`). A marker's definition is not compared field by field, it has no fields.

### Bounds

A bound (`B` of `Fixed`: `Unbounded`, `Range<MIN, MAX>`, `RangeU64<MIN, MAX>`, `RangeDec<MIN, MAX, EXP>`) doesn't
change the bytes, but a reader refuses a value outside its bound (on decode, before a handler runs). So a bound is
part of the contract, and any change of it breaks one side, depending on who writes the item:

* **Narrowed** (`Range<0, 300>` -> `Range<0, 100>`, `Unbounded` or plain `T` -> `Range<..>`): the new version refuses
  values the old one writes. For a method argument or a writable property that is an old host's call refused by a new
  device; for a property or stream the device writes, an old device's value refused by a new host.
* **Widened** (`Range<0, 100>` -> `Range<0, 300>`, `Range<..>` -> `Unbounded` or plain `T`): the old version refuses
  values the new one writes: a new host's argument refused by an old device, a new device's value refused by an old
  host.

Both are breaking, unlike a [max length](#maximum-lengths-maxlent-n) raised, and the checker says which side writes
and which refuses:

```text
Api::uptime: B of Fixed: bound 0..=600 on the new version, 0..=300 on the old version: values the new version
writes outside 0..=300 are refused by the old version
```

Bounds are compared by value: `RangeDec<-5, 25, -1>` and `RangeDec<-50, 250, -2>` are both `-0.5..=2.5`, the same
bound. They are recognised by name and arguments wherever they come from; a bound marker of your own with other names
is compared as any marker, by name.

In the place of a bound, `Saturating` is the same values as `Unbounded` (compatible), while `Wrapping` on one side
only is breaking: a wrapping number is a position modulo its storage, the reader would take a wrap for a jump (see
[units compared by meaning](units_by_meaning.md#bounds-wrapping-and-saturating)).

### Overflow policies of `Fixed`

The bounds marker `B` of an `exact_units::Fixed` that is `Unbounded`, `Wrapping` or `Saturating` (STD-63) has no
bytes of its own: changing one into another keeps the bytes and the value they mean, and changes what arithmetic on
the value does (a `Bam16` wraps at a turn, a `Q15` stops at its ends). Every reader still reads every value. Whether the
checker reports such a change, and how, is open: EVO-17 (today `Wrapping` on one side only is reported as breaking,
see [bounds](#bounds)). A change of the unit or the scale is a change of meaning, not of policy.

### Generic types

A generic struct or enum (`Timed<T>`, `Window<const N: usize>`, API-43) evolves like any other: its fields by the
rules for its size assumption, its arguments by their own rules (`Timed<Ampere>` changes when `Ampere` does). Its
parameters are compared by position and kind:

* Renaming a parameter (`Timed<T>` -> `Timed<V>`) changes nothing.
* Adding, removing or reordering parameters, turning a type parameter into a const one, or changing the type of a
  const parameter (`usize` -> `u8`) is breaking, also when a default keeps old code compiling: the uses on the wire
  carry one argument per parameter.
* Making a fixed length a parameter (`[u16; 4]` -> `[u16; N]`) or back is breaking for the definition; uses that end
  up with the same length (`Window<4>`) have the same bytes.
* A use with other arguments (`Window<4>` -> `Window<8>`, `Pair<u8, u16>` -> `Pair<u8, u32>`) is another type, as
  `[u16; 4]` -> `[u16; 8]` is.

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

### Compressed sequences: `Series<T>`

A `Series` has bytes of its own, so `Vec<T>` -> `Series<T>` or `Series<u32>` -> `Series<u64>` is breaking (`ww api
check` reports it), unlike the borrowed / owned form (`Series<'i, T>` / `SeriesOwned<T>`), which is the same type.
The codecs of its columns are not part of the type: each writer picks its own (`CodecPolicy`), every reader reads
them all. A struct or enum `T` evolves inside a `Series` as it does on its own (see [Series](../serdes/series.md)).

## API

Data types used in an API are part of its SemVer guarantee and follow the rules above: breaking compatibility of any
data type used directly or indirectly needs a bump of the API's breaking version position as well (minor before 1.0,
major after, see [your API's version](versions.md#your-apis-version)). The API itself adds rules for its resources.

### Moving a resource to another place

Within a wire line, a resource is its position in its trait: that is its id on the wire. A new one goes at the end,
and moving one (inserting another before it, or putting it into a sub-level) is a wire break that needs a new line.

Across a line break the bytes are incompatible anyway, so position means nothing there. The
[evolution checker](checker_tool.md#what-is-checked) matches resources by their path instead, the trait and the
name: `Thermo::read_once` on line 2 is `Thermo::read_once` on line 3, wherever it sits in the trait. A resource
replaced in place reads as one removed and one added (`set_rate` removed, `configure` added), not as a rename with
other arguments. A stream item or argument whose type was replaced by another type of the crate (`Sample` by
`Batch`) is reported as replaced, not compared field by field.

A resource that moved to another place on the new line is marked there with `#[moved_from]`, naming the wire version
and the place it had on the line before:

```rust
#[ww_api_root(wire = "w3.0")]
pub trait Thermo {
    fn configure(config: Config) -> Result<(), RateError>;
    ww_impl!(health: Health);
}

#[ww_trait]
pub trait Health {
    /// Was `Thermo::self_test` on line 2.
    #[moved_from(w2.1, Thermo::self_test)]
    fn self_test() -> Result<(), SelfTestError>;
}
```

* The place is the trait and the resource name, as the checker writes them (`Thermo::self_test`), and the version is
  a revision of the line it had that place on (also as a string, `#[moved_from("w2.1", ..)]`).
* The checker verifies that the resource is exactly the same: it works with the old one both ways, with the same Rust
  signature, max lengths and types (the crate's own by name). It then reports one move, `Thermo::self_test moved to
  Health::self_test`, instead of a removal and an addition. A move breaks Rust code using the old path, so the crate
  takes a breaking bump, as for any line break.
* A resource that also changed (`uptime` from seconds in a `u32` to milliseconds in a `u64`) is a new resource: the
  attribute is refused, and the old one is reported removed.
* A resource that moved over several breaks keeps one attribute per earlier line, so the older link needn't be
  deleted: `#[moved_from(w2.1, Thermo::self_test)]` and `#[moved_from(w1.0, Thermo::check)]`. Two naming one line are
  refused.
* Each attribute is checked against the snapshot of the line it names: by the comparison itself for the line compared
  with, against that line's latest snapshot for an older link of a chain and on every later revision of the new line.
  One that can't be checked, as its line has no snapshot, is a warning.
* An attribute that doesn't hold fails the check: one naming a place the line doesn't have, a revision newer than that
  line's latest snapshot or older than the resource's own `#[since]`, a version on the API's own line, its own place,
  a place another attribute names too, or a resource still there on the new line (a resource that stayed can't have
  moved too).
* A resource of the same name at another place is never taken for the same one by itself: its meaning may have
  changed, as `on()` of a LED and `on()` of a motor. When a line break is first checked, the checker lists each
  resource added with the name of one removed elsewhere as a proposal, suggesting `#[moved_from]` when the signature
  is the same. It never adds the attribute.
* In a crate with [several lines](versions.md#several-lines-in-one-crate), the new line's root has no snapshot of its
  own line yet. It is compared with the latest snapshot of the previous line's root of the same name, for its moves
  and proposals only.

`#[moved_from]` is read from the source and is not part of the API model: a device's introspection data doesn't carry
it, so the dynamic client and the facade helpers don't see it yet.

### Arguments

The arguments of a method or stream function are the fields of an `Unsized` args struct on the wire, so they follow
the rules for [unsized types](#unsized-types) (EVO-22, [methods](../api/methods.md#arguments-that-evolve)):

* **Adding an argument at the end with a default** is wire-compatible. A device of the older version skips it, a
  device of the newer version reads the default when an older client leaves it out. `Option<T>`, `Vec<T>` and
  `String` arguments at the end have their natural default (`None`, empty) without an attribute, any other type needs
  `#[default = expr]`. Removing a trailing argument that has a default is wire-compatible the same way.
* **Adding an argument without a default** (or before others) breaks the wire: a device of the newer version can't
  read a call from an older client. The checker names it: ``argument `brightness` is not sent by the old version and
  has no #[default]``.
* **Promoting arguments to a `#[flatten]` struct** (`fn f(a: u8, b: u16)` to `fn f(#[flatten] c: C)` with
  `struct C { a: u8, b: u16 }`) keeps every byte. The struct then grows by its own rules: fields appended with
  `#[default]`.
* Changing an argument's type follows the rules for that type. Renaming an argument keeps the wire, but Python and
  `ww` scripts that pass it by name change with it.

Each of the compatible changes still gives the handler and the client call other parameters: a break of the API
crate's Rust API. `ww api check` reports it with its own rule, `breaking (Rust only, the wire is compatible)`, and the
breaking version position is bumped, as for a rename. The wire version gets a new revision at most, so devices and
hosts of both versions keep talking: link setup decides on the wire line, not on the crate version.

### Streams and batched properties

The delivery kind of a stream is part of the API (API-30, API-31,
[stream reliability](../api/stream_reliability.md)). Turning a plain `stream!` into a `#[counted]` or `#[reliable]`
one (or back, or one into the other), or giving a stream arguments or a result
(`#[stream(T)] fn name(args) -> Result<A, E>`), changes its events and how it is opened, so it is a break of the API
crate: bump its breaking version position, as for a plain sink becoming a `#[credit]` one. A plain
`#[stream(T)] fn name()` without arguments and result is the same as `stream!(name: T)`, the same bytes in
introspection.

Batching a stream (API-41: `batched` / `batched(untimed)` in `#[stream(..)]`, `#[batched]` next to `stream!`), or
un-batching it, or switching between timed and untimed, changes its item type on the wire (one `Series<Timed<T>>` or
`Series<T>` per event instead of one `T`): a break of the API crate too. The batch size is the device's (it is not in
the API), so changing it is not a break.

Making a property `+batched` (API-39) or not changes its observe slot (a `Batched<T>` instead of a `T`): a break of
the API crate. The times of the samples travel in the `PropertyUpdate` event, not in a slot, so adding resources to a
level with `+batched` properties follows the usual rules.

### Optional items

`#[optional]` / `#[optional(group)]` on a method, property, stream or `ww_impl!` (API-16, API-54) is not on the wire:
an item keeps its id whether a device has it or not, and a device leaving it out answers `Unimplemented`, which a
client already has to handle for any call. But servers name optional items and groups in `omit = [..]` / `omit_if`,
so `ww api check` treats the marker by what it does to their Rust code:

* Making an item or a `ww_impl!` optional: compatible. Clients of the new version learn from a device's model that it
  may leave it out; clients of the old version get `Unimplemented` from a device that does.
* Putting an `#[optional]` item into a group (`#[optional]` to `#[optional(group)]`): compatible, `omit` by the item's
  name still works.
* Making an optional item required again, moving it to another group or out of its group: a break of the API crate.
  A server leaving it out no longer compiles (no handler), or silently keeps an item it meant to leave out.

### Metadata attributes

`#[claim(..)]`, `#[shares(..)]` and `#[label(..)]` (SRV-34, [metadata](../api/metadata.md)) are not on the wire and a
server doesn't use them: adding, changing or removing one is compatible, `ww api check` lists it. Only the
introspection data (and so the API hash) changes. Removing a `#[claim]` does change what the hub lets a program do: a
program that claims that resource is refused from then on, so say so in the changelog.

### Choices of one firmware image

Some choices belong to a firmware image and not to its API: `ww_codegen!(.., observe_samples = false)` is one. Two
images of one API have the same API hash, snapshot and introspection data, and a host that has the data cached by its
hash doesn't download it again, so these choices can't travel there. They are in `ww_version::ImageFacts`, a field of
the link's `DeviceInfo` outside its hash. A device sends it on every connection, also with introspection off, and a
generated server has it as `image_facts()`, the third argument of `LinkConfig::new`.

* A new choice is a field appended to `ImageFacts` with `#[default]`, its zero value the ordinary case. It is
  wire-compatible both ways: an old device reads as the ordinary case, an old host steps over the field.
* A fact only saves a host from asking. What a device answers still decides, for a host that doesn't read the fact.
* A choice that changes what a client may rely on is not a fact of the image: it belongs in the API.

### Not decided yet

Two changes of a return type are open questions, not rules yet: `T` -> a struct containing `T` (other than a
[transparent wrapper](#transparent-wrappers-units-and-scales) of it, which is compatible), and `T` -> `Vec<T>`.

## Settings stored on a device

A [settings store](../api/settings.md) (`ww_settings`, STD-32) keeps a record per group in the device's flash, and
the record outlives the firmware that wrote it: an update reads the old firmware's record, a downgrade the new one's.
Each field has a slot, each stored field is a byte blob of its own, so every firmware can step over a slot it doesn't
know. That makes the rules looser than for a [sparse struct](#sparse-structs):

* **Adding a field**: any slot that was never used, also one in a gap between older slots. Older records read it as
  its default. An older firmware keeps it unchanged when it writes its record.
* **Removing a field**: put its slot into `#[settings(retired(..))]`; the derive then refuses to give it to another
  field. A record that still holds it is read without it, and the slot is dropped on the next write, so a downgrade to
  a firmware that still has the field finds it at its default.
* **Changing a field's type**: only the way the type itself may evolve (a struct gets fields with `#[default]`
  appended, an enum gets variants). An older firmware reads what it knows of the newer value (a struct's first
  fields, an enum variant it doesn't know as `Unknown(n)`), and as long as it doesn't change the field, it writes the
  newer bytes back unchanged; if it changes the field, the parts it doesn't know are gone. Bytes it can't decode at
  all (an enum with `no_future_version`) are kept while the field stays at its default. Any other type change, or a
  change of what the value means (units, scale): a new slot, and the old one retired.
* **Changing a default**: a stored record holds only the fields that differ from their default, so every device that
  never set the field takes the new default, and so does one that set it to exactly the old default. When that is
  wrong, the field needs a new slot.
* **Renaming a field** is fine; the slot is what is stored. Moving a field to another group (another `Key`) is a new
  field there and a retired one here; copying the value over is the firmware's job on load.
* The **record format** (`ww_settings::record::FORMAT`) and the **store layout** (the page and item headers in
  `ww_settings::store`) are defined by `ww_settings` itself: a change to either is a breaking change of `ww_settings`,
  as for `ww_framer` in [layer 3](crate_layers.md#layer-3-framework). A firmware that finds a record in a format it
  doesn't know leaves it alone (`is_read_only`) instead of overwriting it.
