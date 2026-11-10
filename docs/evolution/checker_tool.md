---
level: basic
---

# Evolution checker

`ww api check <path>` compares the traits and types of a crate with the latest [snapshot](../cli_reference.md#ww-api) saved in
`<path>/api_snapshots/`, and fails if the crate version or the [wire version](rules.md#the-wire-version) is not
bumped enough for what changed, following the [evolution rules](rules.md). The crate is parsed from source, same as for `ww api save`, so nothing has to be built.

```
$ ww api check ww_stdlib/ww_uart
ww_uart 0.1.0 (ww_stdlib/ww_uart/api_snapshots/ww_uart_0_1_0.ron) -> 0.1.1 (source): compatible changes
  needs: wire w1.0 -> w1.1 or later (a new wire revision), crate 0.1.0 -> 0.1.1 or later (compatible position (patch before 1.0, minor after))
  compatible: wire version w1.0 -> w1.1
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
  needs: no wire version declared (a new wire line if it were), crate 0.1.0 -> 0.2.0 or later (breaking position (minor before 1.0, major after))
  breaking: BlinkyApi::led_off removed
      rule: traits, resources and types are never removed, clients and code using them break (docs/evolution/rules.md#api)
Error: blinky_api 0.1.0 -> 0.1.0 has breaking changes, which requires a bump of the breaking position (minor before 1.0, major after): 0.2.0 or later.
blinky_api 0.1.0 has a snapshot (examples/blinky_api/api_snapshots/blinky_api_0_1_0.ron), so it is released and never changes: a change needs a new version.
Bump `version` in examples/blinky_api/Cargo.toml to 0.2.0 or later (docs/evolution/rules.md#your-apis-version); once that version is released, record it with `ww api save examples/blinky_api` and commit api_snapshots/.
```

`blinky_api`'s snapshot of 0.1.0 was saved before wire versions existed, so only the crate version is checked
against it; the next section shows both.

## Two bumps: the wire and the crate

The wire version (`[package.metadata.wire_weaver] wire = "w1.0"` in `Cargo.toml`) and the crate version are bumped apart: the wire version for what devices and hosts of
the two versions can still say to each other, the crate version for Rust code. One comparison gives both, on the
`needs:` line, and the check fails when either is not bumped enough, naming what to change for each. A trailing
argument with a default is a wire-compatible addition, a new wire revision, but a breaking change of the Rust
signatures, a breaking crate bump:

```
$ ww api check my_device_api
my_device_api 0.1.0 (my_device_api/api_snapshots/my_device_api_0_1_0.ron) -> 0.1.1 (source): breaking changes
  needs: wire w1.0 -> w1.1 or later (a new wire revision), crate 0.1.0 -> 0.2.0 or later (breaking position (minor before 1.0, major after))
  breaking (Rust only, the wire is compatible): MyDevice::led_on: takes 0 argument(s) on the old version, 1 on the new version: the same bytes on the wire, but other parameters for handlers and client calls
      rule: arguments added at the end with a default, or promoted to a #[flatten] struct, keep the wire, but change the Rust signatures of handlers and client calls (docs/evolution/rules.md#arguments)
Error: my_device_api 0.1.0 -> 0.1.1 has breaking changes, which requires a bump of the breaking position (minor before 1.0, major after): 0.2.0 or later.
Bump `version` in my_device_api/Cargo.toml to 0.2.0 or later (docs/evolution/rules.md#your-apis-version); once that version is released, record it with `ww api save my_device_api` and commit api_snapshots/.
my_device_api wire w1.0 -> w1.0 has MyDevice::led_on: arguments added or removed at the end, which needs a new wire revision: w1.1 or later.
Bump the wire version in `[package.metadata.wire_weaver] wire` of my_device_api/Cargo.toml to w1.1 or later (docs/evolution/rules.md#the-wire-version).
```

Which change needs which bump is in [the rules](rules.md#which-bump-a-change-needs). In short:

* A new wire line (`w1.0` -> `w2.0`): anything that breaks the wire, also through a dependency. A `ww_stdlib` crate
  on a new line of its own is one cause, with the breaks it brings named under it: `shared_api on a new wire line,
  w1.0 -> w2.0 (2 wire breaks because type shared_api::Shared changed)`.
* A new wire revision (`w1.0` -> `w1.1`): a wire-compatible addition (a method, property, stream, trait, type, field,
  variant or trailing argument), a raised max length, or a dependency at a newer revision of its line.
* No wire bump: doc comments, and changes that only break Rust code (a rename, a `#[flatten]` promotion, a changed
  catch-all). Doc comments still need the compatible crate bump: they are in introspection data and snapshots, so
  tools show the new docs ([doc comments](rules.md#doc-comments)).

A bigger bump than needed is always accepted, so a line bumped by hand for a change of meaning the checker can't see
passes. A wire version going back, or no longer declared, fails, and so does a `#[since]` newer than the crate's wire
version. `ww api save` refuses to save in each of these cases, as for a crate version not bumped enough. A crate that
declares no wire version on the old side has nothing to compare with: the `needs:` line still says what it would
need. From code, `Report::wire_bump`, `Report::minimal_wire()` and `Report::check_wire()` give the same.

## Several broken items, one cause (EVO-21)

A dependency (a `ww_stdlib` crate, or any other crate used through it) can break more than one trait, resource or
type of the checked crate at once: every item that reaches the one type, trait or marker the dependency changed
breaks with it. The checker groups such items under the change that actually broke them, instead of listing them as
unrelated breaks, whenever it can name that cause (a crate and a name, from the same checks that already explain a
single break: [a wire-incompatible dependency version](#what-is-checked), or the same crate version with a different
definition):

```
$ ww api check my_board_api
my_board_api 0.4.0 (my_board_api/api_snapshots/my_board_api_0_4_0.ron) -> 0.4.0 (source): breaking changes
  6 broken because type ww_board_info::Rework changed:
    breaking: BoardInfo::info: ...
        rule: a type written by either version is read by the other one (docs/evolution/rules.md#data-types)
    breaking: Indication::state: ...
        rule: ...
    breaking: Clock::now: ...
        rule: ...
    breaking: Can::rate: ...
        rule: ...
    breaking: LastReset::reason: ...
        rule: ...
    breaking: Counters::read: ...
        rule: ...
Error: my_board_api 0.4.0 -> 0.4.0 has breaking changes, which requires a bump of the breaking position (minor before 1.0, major after): 0.5.0 or later.
```

A group forms only once a cause reaches two or more items; a single broken item still reads as an ordinary breaking
line, with its own rule and bump. Grouping is naming, not a verdict of its own: every grouped item still carries its
own [`Rule`](#what-is-checked) and counts toward the same required bump as it would alone.

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

## Several API roots in one crate

A crate that holds several wire lines, one `#[ww_api_root]` per module with its wire version in
`[package.metadata.wire_weaver.roots]` ([several lines in one crate](rules.md#several-lines-in-one-crate)), has a
snapshot per root, named after the crate version that saved it and the root's wire version,
`<crate>_<major>_<minor>_<patch>.<module path>.<Trait>@w<line>.<revision>.ron`. Crates with one root, or none
(`ww_stdlib`), keep their whole-crate files:

```
$ ww api save tests/multi_root_api
wire_v1::Thermo: saved tests/multi_root_api/api_snapshots/multi_root_api_0_1_0.wire_v1.Thermo@w1.2.ron, 5 resources, 2 traits, 1 types
wire_v2::Thermo: saved tests/multi_root_api/api_snapshots/multi_root_api_0_1_0.wire_v2.Thermo@w2.1.ron, 5 resources, 2 traits, 1 types
```

* A root's snapshot holds the root and the traits and types it reaches. A type in `common` that no root uses is in
  none of them, and another line's types are not in this one's.
* `#[since = "w1.2, w2.1"]` on a shared item is recorded as the revision of the snapshot's own line.
* Each root is checked against its own previous snapshot before it is saved, as a crate is: the version and the
  entry in the `roots` table must be bumped enough. Nothing is written when any root is refused.
* Only the current crate version's files are written, as for a whole crate, so a released snapshot never changes and
  the pre-commit hook [above](#a-released-snapshot-never-changes) works unchanged.
* A root that didn't change since its previous snapshot gets no new file: that one, saved by an older crate version,
  stays its latest. A root that changed, a doc comment too, gets a file of the new crate version.
* A crate that had one root and now has several still has its older whole-crate snapshots
  (`thermo_api_1_2_0.ron`). Each counts as an earlier snapshot of the root on the same line with the same trait
  name, cut down to that root.
* A root with no earlier snapshot is saved unchecked. When the crate has snapshots of older versions, `ww api save`
  warns, naming what it looked for and what is there: a root renamed, or moved to another module or line, matches
  none of them by name (EVO-40).
* A root on w0 gets no snapshot (below).

### Checking each root

`ww api check` and `Check::run` check such a crate root by root (EVO-39): each root from source against its own
latest snapshot, under its name and wire version. Here `Code` in `common` went from `u16` to `u32`:

```
$ ww api check tests/multi_root_api
API root `wire_v1::Thermo` (w1.2)
multi_root_api 0.1.0 (api_snapshots/multi_root_api_0_1_0.wire_v1.Thermo@w1.2.ron) -> 0.1.0 (source): breaking changes
  needs: wire w1.2 -> w2.0 or later (a new wire line), crate 0.1.0 -> 0.2.0 or later (breaking position ...)
  breaking: Channel::read: return type: struct Code: field 0: u32 on the new version, u16 on the old version
  ...
API root `wire_v2::Thermo` (w2.1)
multi_root_api 0.1.0 (api_snapshots/multi_root_api_0_1_0.wire_v2.Thermo@w2.1.ron) -> 0.1.0 (source): breaking changes
  needs: wire w2.1 -> w3.0 or later (a new wire line), crate 0.1.0 -> 0.2.0 or later (breaking position ...)
  ...
note: shared `Code` changed on `wire_v1::Thermo` (w1.2), `wire_v2::Thermo` (w2.1)
crate: 0.2.0 or later, the largest its roots need
```

* Each root's wire version is checked on its own line, and a wire bump names the root's entry in the `roots` table.
  `#[since = "w1.2, w2.1"]` on a shared item is checked against each line's revision.
* A type or trait of the crate that more than one root reaches and that changed is listed once more at the end, with
  every root and line it changed on.
* The crate version is one for all roots, so it has to be bumped for the root that needs the most.
* A root that didn't change keeps an older crate version's snapshot as its latest. Once the crate version has
  snapshots of other roots, that older snapshot stands for the current version: a change to the root afterwards needs
  a new version, or a save again while the version isn't released, as for a whole crate.
* With `require_current` (the evolution test), a root that changed since its latest snapshot needs a file of the
  current version. A root unchanged since an older version's snapshot needs none.
* A root the `roots` table no longer lists while it has snapshots is a line leaving the crate, a breaking crate bump
  ([several lines in one crate](rules.md#several-lines-in-one-crate)). `ww api save` records the removal in
  `<crate>_<major>_<minor>_<patch>.<module path>.<Trait>@w<line>.<revision>.removed`, a one-line text file, and refuses
  it without the bump. Later versions are not asked again, and the evolution test fails until the removal is recorded.
* `ww api check --against <file>` takes one root's snapshot file and checks that root. A root's snapshot file checked
  by itself is compared with the previous snapshot of the same root.
* `ww api diff` lists each root's differences under its name.
* All roots are loaded with one cache of the crates read (`wire_weaver_core::CrateCache`), so the dependencies'
  source is walked once for the whole check.
* A root on w0 gets no snapshot and needs none: its check is on the design line ([below](#the-design-line-w0)).

## The design line, w0

An API on `w0.x` is still being designed ([w0, the design line](rules.md#w0-the-design-line)): `ww api save` keeps no
snapshot of it, and `ww api check` passes, also with `--strict` and in the evolution test with `require_current`.

```
$ ww api save my_device_api
not saved: w0.2 is the design line, which keeps no snapshots (docs/evolution/rules.md#w0-the-design-line); the first is saved once the API moves to w1.0 at its first release
```

* Against a snapshot that exists (`--against`, or one kept from before), `ww api check` prints the report as usual,
  `needs:` saying `wire w0.2 (the design line: no wire bump)`, and passes: what differs is information.
* A crate that can be published (no `publish = false`) with a root on w0 gets a warning from both commands.
* Moving the API to `w1.0` makes the next `ww api save` keep its first snapshot, and every check from there on is
  the one above.

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
* a `ww_si::Quantity` becomes a `ww_si::Fixed` or back: other bytes; the finding says it is the intended move, for a
  major version,
* a [bound](rules.md#bounds) is narrowed or widened: values one side writes are refused by the other (rule `Bound`),
* a trait or type from another crate is replaced by one from a wire-incompatible version of that crate (e.g.
  `ww_si 0.1` -> `0.2`), or one from the same crate version with a different definition.

It also warns about additions without `#[since]`, or with a `#[since]` older than the new wire version (and about a
wire revision that wasn't bumped for them), and fails on any resource, field or variant whose `#[since]` is newer
than the API's own wire version (a host built with it would refuse that item on every device of this version, see
[`#[since]`](rules.md#since)). A change of the root's wire version is listed too; whether it is bumped enough is
[checked on its own](#two-bumps-the-wire-and-the-crate).

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
    #[since = "w1.1"]
    pub g: u7, // added in w1.1, reads as 0 from old data
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
    #[since = "w1.1"]
    pub g: u3, // added in w1.1, reads as 0 from old data
    pub b: u8,
}
```

Without `a`, the padding before `b` is 7 bits if `S` starts at a byte boundary, but none if it starts at bit offset 7,
so `g` can't be added. Padding after the last field is never free: whatever follows the type in its parent starts
right there. Enum variants with data are not padded to the same size, so there are no free bits after a shorter
variant either.

All of this was checked against real bytes: old and new versions read each other's data, with the new field
reading as zero from old data. The client uses the same rules when comparing its API with a device's.

## Unit-suffix hints (TOOL-22)

`ww api check` also notes a plain integer field, method argument or property whose name ends in a known unit
suffix (`_us`, `_ms`, `_ns`, `_s`, `_hz`, `_khz`, `_ppb`, `_ppm`, `_bytes`, `_bits`, `_kib`), with the
[`exact_units::Fixed<U, P, T>`](../std_library/exact_units.md) it could become instead: a note, never an error,
printed after the verdict and never affecting it, even when there is no snapshot to compare with yet:

```
$ ww api check my_device_api
my_device_api 0.1.0 (my_device_api/api_snapshots/my_device_api_0_1_0.ron) -> 0.1.0 (source): no changes
no changes
  note: shared_us: u64 could be exact_units::Fixed<Second, Micro, u64> (exact_units::Micros<u64>), renamed to
  `shared`; silence with #[no_unit_hint]
```

A field that really is a plain number (a count of nothing in particular, an index, a value whose unit isn't fixed)
silences the note with `#[no_unit_hint]` right on it:

```rust
struct SteerPoint {
    #[no_unit_hint]
    retry_count_s: u8, // not a duration: s is the slot letter here, not seconds
}
```

On a trait method the same bare attribute silences every argument, or names only the ones to leave out:

```rust
#[ww_trait]
trait Device {
    #[no_unit_hint(poll_hz)]
    fn configure(&self, timeout_ms: u32, poll_hz: u32); // timeout_ms still gets the note
}
```

and on a `property!`/`stream!`/`sink!`/`ww_impl!` item it silences the resource's own name:

```rust
#[no_unit_hint]
property!(ro, rate_hz: u32); // a raw register value here, not a frequency
```

`ww api scaffold` shows the same notes on stderr, next to the generated handler stubs, so a new API crate's
handlers are written against `Fixed` from the start where the note applies.

## A missing-test warning (TOOL-31)

`ww api check` and `ww api save` also warn, before anything else, when the crate directory has no
`tests/evolution.rs` calling `wire_weaver_evolution::Check` (the [test above](#checking-at-every-commit)):

```
$ ww api check my_device_api
warning: no my_device_api/tests/evolution.rs calling wire_weaver_evolution::Check: add the wire_weaver_evolution
  dev-dependency and this test (docs/evolution/checker_tool.md), so a wire break is caught by `cargo test`, not
  only by `ww api check` run by hand
my_device_api 0.1.0 (my_device_api/api_snapshots/my_device_api_0_1_0.ron) -> 0.1.0 (source): no changes
no changes
```

A crate without the API crate's own test only gets checked when someone remembers to run `ww api check` by hand:
`cargo test`, CI and a pre-commit hook never see it. The warning never affects the verdict, like the TOOL-22 hints,
and it only looks at the file's text (it doesn't need the crate to build), so it also fires on a crate the checker
can't otherwise parse. `wire_weaver_evolution::missing_test_warning(path)` is the library call behind it, for other
tools that want the same check.

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
* [Grouping several breaks under one cause](#several-broken-items-one-cause-evo-21) only works when the cause is a
  dependency's own trait, type or marker, caught by a crate version or a definition mismatch: a plain field or
  argument type changed in the checked crate's own code is not grouped (each item it reaches is already named by
  itself), and a redefinition the checker doesn't yet name a cause for (e.g. a unit or scale of the crate's own
  redefined under the same name, EVO-17) still lists every item it reaches apart.
