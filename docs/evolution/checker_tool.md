# Evolution checker

`ww api check <path>` compares the traits and types of a crate with the latest [snapshot](../cli.md#ww-api) saved in
`<path>/api_snapshots/`, and fails if the crate version is not bumped enough for what changed, following the
[evolution rules](rules.md). The crate is parsed from source, same as for `ww api save`, so nothing has to be built.

```
$ ww api check ww_stdlib/ww_uart
ww_uart 0.1.0 (ww_stdlib/ww_uart/api_snapshots/ww_uart_0_1_0.ron) -> 0.1.1 (source): compatible changes
  compatible: Uart::flush added
version is bumped enough
```

The snapshot used is the newest one of the same crate with a version not newer than the one in `Cargo.toml`, so it is
the snapshot of the current version if it was already saved, and of the previous version otherwise. `--against <file>`
compares with a specific snapshot instead. Nothing is checked if there is no snapshot yet.

`<path>` can also be a snapshot file, which is then compared with the previous snapshot in the same directory, e.g. to
check snapshots that were saved with `--force`.

`ww api save` runs the same check before saving a snapshot of a new version, and refuses to save it if the version
is not bumped enough (`--force` saves anyway). So in a crate that has snapshots of all of its published versions,
every new snapshot is checked against the previous one.

## Listing all changes

`ww api diff <path>` compares the same versions as `ww api check` (and also takes `--against` or a snapshot file), and
lists every difference, doc comments included, followed by the verdict. It never fails, and doesn't tell why a change
is breaking, `ww api check` does:

```
$ ww api diff ww_stdlib/ww_uart
ww_uart 0.1.0 (ww_stdlib/ww_uart/api_snapshots/ww_uart_0_1_0.ron) -> 0.1.0 (source)
  ~ Uart::rx: docs
      /// Receive stream
    + /// of chunks
  ~ Uart::prevent_back_feed: docs
      /// Detect RX low for more than 1 byte interval and set TX low to avoid powering device in sleep mode through it.
    - /// Optional, check [Capabilities] if supported.
    + /// Check [Capabilities] if supported.
  + Uart::flush: fn flush()
  ~ RxFlags: field `parity_error`: docs
    + /// Parity error
  + RxFlags: field `framing_error`: framing_error: bool
breaking changes, version has to be 0.2.0 or later (run `ww api check` for details)
```

Each line is an added (`+`), removed (`-`) or changed (`~`) trait, resource, type, struct field, enum variant or
dependency crate version. Changes are: a resource's signature (kind, arguments, return type, property access, stream
direction, array or not), a resource's or variant's name, a type's kind, size kind or repr, a field's position, type
or `#[default]` value, `#[since]` of any of them, and doc comments, shown as a line diff. Items are matched as
described below, enum variants by discriminant, fields by name and unnamed ones by position. A renamed type is
removed and added.

`evolution::diff()` in `wire_weaver_client` returns the same list, for use from other tools.

## What is checked

Changes are sorted into four kinds, and each kind requires a bump of a version position:

| Change            | Examples                                                                                                    | Bump                                               |
|-------------------|-------------------------------------------------------------------------------------------------------------|----------------------------------------------------|
| None              | exactly the same traits and types, doc comments included                                                    | none                                               |
| Doc comments only | a doc comment on a trait, resource, type, field or variant is changed                                       | compatible position (patch before 1.0, minor after) |
| Compatible        | new trait, new resource at the end of a trait, new field with `#[default]` at the end of an `Unsized` type, new field in unused padding bits of any type, new type, `#[since]` or `#[default]` value changed | compatible position (patch before 1.0, minor after) |
| Breaking          | see below                                                                                                   | breaking position (minor before 1.0, major after)  |

Doc comments are not on the wire, but a released version's snapshot never changes, so they need a new version as well
(see [doc comments](rules.md#doc-comments)). A bigger bump than required is always fine.

Resources are matched by their id inside each trait (their position, as the device sees them), traits and types by
name. A change is breaking if:

* a trait, resource or type is removed, or a resource moved to another id (e.g. a new one inserted before it: add new
  resources at the end),
* a resource, struct field or enum variant is renamed, or struct fields change between named and unnamed: this does
  not change the wire, but breaks Rust code using them,
* a resource of the old version can't be used with the new one, in either direction (old client with a new device, and
  new client with an old device), compared the same way as a client checks its API against a device's: resource kind,
  number and types of arguments, return type, property access, stream direction, array or not,
* a type can't be written by one version and read by the other: e.g. field type changed, field added without
  `#[default]` at the end of an `Unsized` type, field added in between old ones outside of unused padding bits (see
  below), field added at the end of a `final_structure`, `self_describing` or `sized` type, enum variant added or
  removed, `ww_repr` or size kind changed,
* a trait or type from another crate is replaced by one from a wire-incompatible version of that crate (e.g.
  `ww_si 0.1` -> `0.2`), or one from the same crate version with a different definition.

It also warns about additions without `#[since]`, or with a `#[since]` that is not the new version.

## New fields in padding bits

Padding inside a type, before a field aligned to a nibble or a byte, is always written as zero bits and skipped when
reading. A new field of any type, `Unsized` or not, can take such bits, anywhere between the old fields, if:

* none of the old fields moves and the size doesn't change: the new field only takes bits that were padding,
* all zero bits are a valid value of it, as that is what it reads from old data: `bool`, numbers, arrays and
  non-`Unsized` structs of them, or an enum without data that has a variant with discriminant 0,
* it's fixed size (not `UNib32`, `Option`, an enum with data, an `Unsized` type, ...).

Old fields are then matched by name. Where padding is depends on the bit offset the type starts at. An `Unsized` type
always starts at a byte boundary, so its padding is always in the same place:

```rust
#[derive_shrink_wrap]
pub struct U {
    pub f: bool,
    // 7 bits of padding before `b`
    #[since = "0.1.1"]
    pub g: u7, // added in 0.1.1, reads as 0 from old data
    pub b: u8,
    pub s: String,
}
```

An `Unsized` type can gain fields with `#[default]` at the end as well, in the same version. Bits after its last field
are not free though: lengths of variable size fields are written there.

`final_structure`, `self_describing` and `sized` types are packed tightly into their parent, so they can't grow at the
end, and they can start at any bit inside the parent, so the checker requires that the new field fits for every start
offset:

```rust
#[derive_shrink_wrap(sized)]
pub struct S {
    pub a: u8,
    pub f: bool,
    // 7 bits of padding before `b`, wherever S starts, as `a` is byte-aligned
    #[since = "0.1.1"]
    pub g: u3, // added in 0.1.1, reads as 0 from old data
    pub b: u8,
}
```

Without `a`, the padding before `b` is 7 bits if `S` starts at a byte boundary, but none if it starts at bit offset 7,
so `g` can't be added. Padding after the last field is never free: whatever follows the type in its parent starts
right there. Enum variants with data are not padded to the same size, so there are no free bits after a shorter
variant either.

All of this was checked against real bytes: old and new versions read each other's data, with the new field
reading as zero from old data. The client uses the same rules when comparing its API with a device's.

## Limitations

* Snapshots of all published versions have to be kept in `api_snapshots/`, the checker can only see what is saved
  there. A crate without snapshots is not checked.
* Only traits and types that `ww api save` finds are checked (see [ww api](../cli.md#ww-api)).
* Semantic changes that keep the same bytes, like changing units or the meaning of a value, can't be detected.
* Padding bits are only reused when that works for every bit offset a type can start at, even if in all of its actual
  uses it's always at the same offset.
