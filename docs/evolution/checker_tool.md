---
level: basic
---

# Evolution checker

`ww api check <path>` compares the traits and types of a crate with the latest [snapshot](../cli_reference.md#ww-api) saved in
`<path>/api_snapshots/`, and fails if the crate version is not bumped enough for what changed, following the
[evolution rules](rules.md). The crate is parsed from source, same as for `ww api save`, so nothing has to be built.

```
$ ww api check ww_stdlib/ww_uart
ww_uart 0.1.0 (ww_stdlib/ww_uart/api_snapshots/ww_uart_0_1_0.ron) -> 0.1.1 (source): compatible changes
  compatible: Uart::flush added
version is bumped enough
```

To start, save a snapshot of each released version and commit it, then check before every release:

```
$ ww api save my_device_api     # once per released version, commit api_snapshots/
$ ww api check my_device_api    # after changing the crate and bumping its version
```

The snapshot used is the newest one of the same crate with a version not newer than the one in `Cargo.toml`, so it is
the snapshot of the current version if it was already saved, and of the previous version otherwise. `--against <file>`
compares with a specific snapshot instead. Nothing is checked if there is no snapshot yet, unless `--strict` is given:
then a crate without a snapshot fails, naming the `ww api save` to run. `--require-current` requires the snapshot of
the version in `Cargo.toml` itself, see [below](#the-current-versions-snapshot).

`<path>` can also be a snapshot file, which is then compared with the previous snapshot in the same directory, e.g. to
check snapshots that were saved with `--force`.

`ww api save` only ever writes the snapshot of the version in `Cargo.toml`: it saves it, or refreshes it when the
source changed since, and never touches an older version's. It runs the same check against the previous version's
snapshot first, and refuses to save if the version is not bumped enough (`--force` saves anyway). So in a crate that
has snapshots of all of its published versions, every new snapshot is checked against the previous one. Refreshing is
for a version still being worked on: once a version is released, its snapshot never changes, bump the version instead
(the pre-commit hook [below](#a-released-snapshot-never-changes) refuses it).

A breaking change is listed with the rule it breaks and where that rule is explained, and a failed check says which
version to put into `Cargo.toml`:

```
$ ww api check examples/blinky_api
blinky_api 0.1.0 (examples/blinky_api/api_snapshots/blinky_api_0_1_0.ron) -> 0.1.0 (source): breaking changes
  breaking: BlinkyApi::led_off removed
      rule: traits, resources and types are never removed, clients and code using them break (docs/evolution/rules.md#api)
Error: blinky_api 0.1.0 -> 0.1.0 has breaking changes, which requires a bump of the breaking position (minor before 1.0, major after): 0.2.0 or later.
blinky_api 0.1.0 has a snapshot (examples/blinky_api/api_snapshots/blinky_api_0_1_0.ron), so it is released and never changes: a change needs a new version.
Bump `version` in examples/blinky_api/Cargo.toml to 0.2.0 or later (docs/evolution/rules.md#your-apis-version); once that version is released, record it with `ww api save examples/blinky_api` and commit api_snapshots/.
```

## Checking at every commit

The checker is a library, `wire_weaver_evolution`, which parses the crate from source like `ww api check` does and
needs neither the client nor a device. An API crate runs it from a test of its own, so the version of the checker is
the one its lock file pins, and `cargo test`, CI and a pre-commit hook all check the same way, with nothing to
install. `examples/blinky_api` is set up like this.

In the API crate, a dev-dependency and `tests/evolution.rs`:

```toml
[dev-dependencies]
wire_weaver_evolution = "0.5"
```

```rust
#[test]
fn api_evolution() {
    wire_weaver_evolution::Check::new(env!("CARGO_MANIFEST_DIR"))
        .require_current(true)
        .assert();
}
```

### The current version's snapshot

`require_current(true)` makes the test fail until the version in `Cargo.toml` has its own snapshot in
`api_snapshots/`, and whenever the source differs from that snapshot. The snapshot is then compared with the previous
version's, so the bump is checked as well. A version can't be released without its snapshot this way: with only the
previous version's snapshot required, a firmware once shipped API 0.2.0 with no `..._0_2_0.ron`, and nothing to tell
later what 0.2.0 was.

The snapshot is saved by the same test, with `WW_API_SAVE=1`:

```
$ WW_API_SAVE=1 cargo test -p my_device_api --test evolution
```

It writes the snapshot of the current version, or refreshes it when the source changed, and refuses (failing the
test) when the version is not bumped enough since the previous version's snapshot. As the test runs the checker its
lock file pins, saving needs no `ww` install either, and every clone saves the same bytes. `ww api save my_device_api`
does the same; `Check::save` is the library call behind both.

So the flow is: bump the version when starting on the next one, change the API, save, commit the snapshot with the
change. A failing test names what to do:

```
no snapshot of my_device_api 0.2.0 in my_device_api/api_snapshots (the previous one is my_device_api/api_snapshots/my_device_api_0_1_0.ron), and the current version's snapshot is required.
Save it with `just api-save` (`WW_API_SAVE=1 cargo test --test evolution` in the crate, or `ww api save my_device_api`) and commit api_snapshots/ (see docs/evolution/checker_tool.md).
```

Upgrading wire_weaver alone never makes this test fail or asks for a new snapshot. A newer wire_weaver may record
more about an API than the one that saved its snapshot (EVO-10 added each enum's catch-all, `future_version`): a field
of the API model that a snapshot file has nowhere was not recorded by its writer, and the checker compares both sides
without it, saying so in the report (`not compared: ItemEnum::future_version, which a snapshot saved by an older
wire_weaver doesn't have`). A snapshot is saved again only for a real change of the API, or when you run `ww api save`
to record the new fields too (an appended variant of an enum with the catch-all passes only once the snapshot records
the catch-all).

`strict(true)` instead of `require_current(true)` only requires some snapshot to compare with: the source is then
compared with the newest one not newer than its version, which may be the previous version's. That suits a crate that
saves a snapshot only when it releases.

Recipes in the project's `justfile`, `api-check` to run the test on its own, `api-save` to save the current version's
snapshot, `setup` to turn on the hook below:

```just
# compare the API with its snapshots: fails without the current version's snapshot, or on a change without the
# version bump it needs
api-check:
    cargo test -p my_device_api --test evolution

# save the snapshot of the current version of the API, commit api_snapshots/ with the change
api-save:
    WW_API_SAVE=1 cargo test -p my_device_api --test evolution -- --nocapture

setup:
    git config core.hooksPath .githooks
```

A step in CI, after the toolchain is set up (`--target` with the host's triple when `.cargo/config.toml` builds for
an MCU by default):

```yaml
      - name: API evolution check
        run: just api-check
```

And a tracked pre-commit hook, `.githooks/pre-commit` (executable), turned on once per clone by `just setup`. It only
runs when the commit touches the API crate:

```sh
#!/bin/sh
# API evolution check (docs/evolution/checker_tool.md), only when the commit touches the API crate
git diff --cached --quiet -- my_device_api && exit 0
exec just api-check
```

The hook checks the working tree, not only what is staged, the same as `cargo test` would. `git commit --no-verify`
skips it for a work-in-progress commit; CI still checks.

### A released snapshot never changes

The checker can't tell whether a version was released, so it lets the current version's snapshot be refreshed. What
was released is in git: a project tags its releases, and the hook refuses a commit that changes a snapshot already in
a tag. Before the `exec` line:

```sh
# a snapshot in a release tag never changes: bump the version instead
for f in $(git diff --cached --name-only --diff-filter=MD -- my_device_api/api_snapshots); do
    if [ -n "$(git tag --contains "$(git log -1 --format=%H -- "$f")")" ]; then
        echo "$f is released and never changes: bump the version in my_device_api/Cargo.toml" >&2
        exit 1
    fi
done
```

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

`diff()` in `wire_weaver_evolution` returns the same list, for use from other tools.

## What is checked

Changes are sorted into four kinds, and each kind requires a bump of a version position:

| Change            | Examples                                                                                                    | Bump                                               |
|-------------------|-------------------------------------------------------------------------------------------------------------|----------------------------------------------------|
| None              | exactly the same traits and types, doc comments included                                                    | none                                               |
| Doc comments only | a doc comment on a trait, resource, type, field or variant is changed                                       | compatible position (patch before 1.0, minor after) |
| Compatible        | new trait, new resource at the end of a trait, new field with `#[default]` at the end of an `Unsized` type, new field in unused padding bits of any type, new type, `#[since]` or `#[default]` value changed, max length raised or removed, `T` <-> a transparent wrapper of `T` | compatible position (patch before 1.0, minor after) |
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
* a [max length](rules.md#maximum-lengths-maxlent-n) is lowered or added (`T` -> `MaxLen<T, N>`): data the old
  version writes may no longer be read,
* a marker argument changes although the bytes stay: the unit or prefix of a
  [transparent wrapper](rules.md#transparent-wrappers-units-and-scales) (`Fixed<Second, Micro, u64>` -> `Fixed<Second,
  Milli, u64>`), any type argument no field uses, or one wrapper replaced by another of the same bytes (rule
  `Meaning`),
* a [bound](rules.md#bounds) is narrowed or widened: values one side writes are refused by the other (rule `Bound`),
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
  there. A crate without snapshots is not checked, except in strict mode, where it fails, and with `require_current`,
  where the current version's snapshot is required.
* Whether a version was released is not known to the checker: `ww api save` and `WW_API_SAVE=1` refresh the current
  version's snapshot whether it was released or not, a hook or review has to keep released ones unchanged.
* Only traits and types that `ww api save` finds are checked (see [ww api](../cli_reference.md#ww-api)).
* Semantic changes that keep the same bytes, like changing units or the meaning of a value, can't be detected.
* Padding bits are only reused when that works for every bit offset a type can start at, even if in all of its actual
  uses it's always at the same offset.
