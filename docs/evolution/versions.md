---
level: advanced
---

# Versions and wire lines

A device and a host meet in link setup. Before either sends a single request, they have to decide whether they can
understand each other at all. This page is about how WireWeaver makes that decision, and about the versions you
write so that it can. [Evolving an API](overview.md) has the short version; [what can change](rules.md) lists which
change is compatible.

## Your API's version

An API has two versions, for two jobs:

* The **crate version** is Cargo's: SemVer for the Rust code built against the API crate. A Rust-only break (a
  trailing argument with a default, a `#[flatten]` promotion, see [arguments](rules.md#arguments)) bumps its breaking
  position, a wire-compatible addition its compatible one (patch before 1.0, minor after).
* The **wire version** is what devices and hosts decide on: link setup connects a device and a host of the same API
  on the same wire line, whatever their crate versions, and refuses another line.

The two move apart on purpose. A new trailing argument with a default keeps every byte, so old and new talk, yet
every handler and client call changes its signature. A field widened from `u8` to `u16` compiles everywhere, yet
breaks the wire. One version could not say both.

Name and version of the user API crate are still sent during link setup, for display (`DeviceApiInfo`, logs,
`PeerVersion` on the device), and the crate name tells APIs apart: a host whose client was generated from another API
crate is refused. Nothing decides on the crate version's numbers.

### The wire version

The API crate declares its wire version in its `Cargo.toml`, next to the crate version. It is said "wire 2.3": line
2, revision 3.

```toml
[package]
name = "blinky_api"
version = "0.4.1"

[package.metadata.wire_weaver]
wire = "w2.3"
```

* A wire-compatible addition bumps the revision (`w2.3` -> `w2.4`), a wire break the line (`w3.0`).
* A new API starts at `w0.0`, the [design line](#w0-the-design-line), and its first release at `w1.0`.
* The `w` is part of the notation everywhere, so a crate version can't be taken for one: `2.3`, `w2.3.0` and `0.2.0`
  are refused.
* The `#[ww_api_root]` trait gets a `*_WIRE_VERSION: ww_version::WireVersion` const next to its `*_FULL_GID`.
* Every crate whose traits or types cross the wire declares its wire version the same way: `ww_stdlib` crates too,
  and the foundation crates below them ([crate layers](crate_layers.md)).

A test crate (`publish = false`) may give the wire version in the attribute instead, `#[ww_api_root(wire = "w2.3")]`:
several roots in one crate standing for different lines, as in a test of two lines talking (EVO-31). The attribute is
refused in a crate that can be published, and when the crate's `Cargo.toml` declares a different one.

A full client needs the wire version of its root: `ww_codegen!` stops with an error naming the fix when the root
trait is a plain `#[ww_trait]`.

### What link setup decides

Link setup decides on the line (EVO-25): `DeviceInfo` carries the device's wire version, `LinkSetup` the host's.

* The same line connects, whatever the revisions: a `w2.4` host talks to a `w2.3` device and the other way round.
  Items newer than the device's revision are refused by the host, see [`#[since]`](#since).
* Another line is refused on both sides: the host with `Error::IncompatibleDeviceProtocol`, which names both lines and
  who should update, the device with `Disconnect(IncompatibleVersion)`.
* A host without generated code (the dynamic client, `ww`, Python) works from the device's introspection data and
  connects to any line.
* A device or host that doesn't say its wire version (built before it existed) is refused by a generated peer.

A host that has to talk to devices of several lines bundles one client per line: see
[a host for several wire lines](multi_line_host.md).

## `#[since]`

Inside a line, a newer host meets older devices all the time. `#[since = "w2.4"]` is the wire revision that added a
resource, field or variant, so the host can tell what an older device has:

```rust
#[ww_api_root]
pub trait BlinkyApi {
    fn led_on();
    #[since = "w2.4"]
    fn led_toggle();
}
```

A host refuses to call a resource marked newer than the connected device's wire version, with
`Error::OlderProtocol`, before sending anything (see `examples/blinky_api_evolved`). A crate version there
(`#[since = "0.2.0"]`, the form before wire versions) is refused with a hint.

Wire versions are compared by line, then revision: the device has the item when its wire version is at least the
marker's.

* On one line, that is the revision that added the item: a `w2.5` device has an item marked `w2.4` and not one marked
  `w2.6`.
* A new line (`w2.5` -> `w3.0`) counts every older marker as present: an item marked `w2.4` is callable on a `w3.0`
  device. Keep the markers as they are on a new line, don't rewrite them. Whether an item survived the break is not
  the marker's to say: an item the new line removed is reported from the device's introspection data
  (`Error::NotImplementedByDevice`).

The host knows the device's wire version from link setup (`DeviceInfo`, EVO-25), and from its introspection data when
the device says nothing there; only a device from before wire versions leaves `#[since]` unchecked.

A marker names a revision of the crate that declares the item, not of the API root. A trait from a `ww_stdlib` crate
(`ww_impl!(board: ww_board_info::BoardInfo)`) carries that crate's markers, compared with the device's wire version of
that crate (`[package.metadata.wire_weaver] wire = ..` in its `Cargo.toml`, see
[crate layers](crate_layers.md#layer-2-ww_stdlib)). They need the device's introspection data; link setup carries only
the root's wire version.

A marker newer than the API's own wire version is a mistake (the item would be refused on every device of this
version); the [evolution checker](checker_tool.md) fails on it.

## Which bump a change needs

The wire version and the crate version are bumped apart, each for what it describes: the wire version for what
devices and hosts of the two versions can still say to each other, the crate version for Rust code (Cargo SemVer).
The [evolution checker](checker_tool.md#two-bumps-the-wire-and-the-crate) derives both from one comparison and fails
when either is not bumped enough:

| Change                                                          | Wire version        | Crate version     |
|-----------------------------------------------------------------|---------------------|-------------------|
| [doc comments](#doc-comments) only                              | none                | compatible        |
| a new method, property, stream, trait, type, field or variant   | `w2.3` -> `w2.4`    | compatible        |
| a trailing argument with a default                              | `w2.3` -> `w2.4`    | breaking          |
| a rename, a `#[flatten]` promotion, a catch-all changed         | none                | breaking          |
| a wire break (removed, moved, a type changed, a bound narrowed) | `w2.3` -> `w3.0`    | breaking          |
| a dependency at a newer revision of its wire line               | `w2.3` -> `w2.4`    | as its changes    |
| a dependency on a new wire line                                 | `w2.3` -> `w3.0`    | breaking          |

"Compatible" is the crate's patch before 1.0 and minor after, "breaking" minor before 1.0 and major after. A bigger
bump than needed is always accepted, so a new line by hand for a change of meaning the checker can't see stays
allowed. A wire version may not go back, and a crate that declared one keeps declaring it.

The API crate's dependencies are part of this contract: upgrading a `ww_stdlib` crate or `shrink_wrap` to a
wire-incompatible version, or [the API model](#api-model), is a breaking change of the user API too.

### Doc comments

Doc comments on traits, resources and types are not part of the wire format, but they carry meaning the code can't
express (what a value means, its limits, when to use what). They are shipped in introspection data and in
[crate snapshots](../cli_reference.md#ww-api), so a GUI or CLI can show them. A released version's snapshot never
changes, so a doc-only change is still a new version: bump the compatible position (patch before 1.0, minor after),
as for a wire-compatible addition. Devices with the old version keep connecting, and tools can tell that improved
docs are available. `ww api save` refuses to overwrite a snapshot with changed docs, and the
[evolution checker](checker_tool.md) requires the compatible bump.

This is about an API's own doc comments. A trait or type of another crate that the API uses is named in its snapshot
and in a device's introspection data by its shape signature, which leaves docs out (EVO-69,
[shape and docs](inside_the_checker.md#doc-comments-of-a-dependency-evo-69)). When only that dependency's docs
change, the API is the same, and the checker says so in a `note:` line, never an error or a bump. A device rebuilt
with other docs and the same API connects as the same API, and the client logs "documentation updated".

### API model

The API model (`ww_client_server`, the request and event encoding every generated server and client speaks) is part
of the compatibility equation too: the API can't move to a new model version without a breaking change of its own.

E.g., if `user_device_api` 0.1.0 depends on `ww_client_server` 0.4.0 and a new major version of `ww_client_server`
comes out (0.5.0), then `user_device_api` must be bumped to 0.2.0 to use the newer API model. This should only happen
to add new features though, and if the previous version does all it is supposed to, there might not be a need to
upgrade.

## w0, the design line

While an API is still being designed, every snapshot and every rule above would only get in the way. Line 0 is for
that (EVO-41): breaks are accepted without a new line, and nothing is kept that would hold the API to its earlier
shape. A first release moves it to `w1.0`, and from there every rule above applies.

* `ww api save` keeps no snapshot of a crate or root on `w0.x` and says why. Moving to `w1.0` saves the first one.
* `ww api check` and the crate's evolution test pass on w0, with `strict` and `require_current` too. Against a
  snapshot that exists (one kept from before, or `--against`) they list what differs as information and ask for no
  bump.
* `#[since]` on w0 is a compile error: revisions mean nothing there. A type shared with released lines of one crate
  ([several lines](#several-lines-in-one-crate)) keeps their revisions; one naming line 0 is refused.
* Link setup takes a device of any w0 revision. A client then needs the device's introspection data whenever the API
  hashes differ: it compares every resource, as on any line, and refuses per call what differs. Without that data it
  refuses every call into its API with `Error::DesignLineUnchecked`, since a revision tells nothing on w0. Equal
  hashes need nothing more: the hash is over the device's own introspection bytes.
* `ww api check` and `ww api save` warn when a crate that can be published (no `publish = false`) has a root on w0:
  others would build on an API that may still break. Move it to `w1.0` before publishing.

`ww api scaffold` suggests `w0.0` for an API that declares no wire version. Keep devices with a w0 API to the bench:
any build of it may break any other.

## Several lines in one crate

A break starts a new line, but the devices on the old line are still out there. Rather than a crate per line, one
API crate can hold every wire line still in use, one `#[ww_api_root]` per module, with the types and sub-traits the
lines share unchanged in a module of their own (EVO-37, `tests/multi_root_api`):

```text
src/lib.rs        pub mod common; pub mod wire_v1; pub mod wire_v2;
src/common.rs     types and #[ww_trait]s every line uses as they are
src/wire_v1.rs    #[ww_api_root] pub trait Thermo { .. }   // line 1
src/wire_v2.rs    #[ww_api_root] pub trait Thermo { .. }   // line 2
```

Each root's wire version is an entry of the `roots` table, by its module path:

```toml
[package.metadata.wire_weaver.roots]
"wire_v1::Thermo" = "w1.2"
"wire_v2::Thermo" = "w2.1"
```

* A shared type is one Rust type on every line: a host of several lines converts nothing between them.
* The crate version is for Rust only, so one crate version serves firmware of every line it holds. A line leaves the
  crate when no device runs it any more, a breaking crate bump.
* `ww_codegen!` names a root by its module path, `ww_codegen!(thermo_api::wire_v2::Thermo for Client, ..)`; with
  several roots, a bare `Thermo` or no name is an error listing them. Two roots of one name differ by their line; two
  traits of one name in one root's tree are refused, since the API model names a trait by crate and name.
* A line's module names the shared types with `use super::common::Code;` and the shared sub-traits by path,
  `ww_impl!(channel: super::common::Channel)`.
* An item of a shared type or trait was added in a revision of each line it is on: `#[since = "w1.2, w2.1"]` names
  one revision per line, and each root takes its own line's.
* A host gets one client for a shared sub-trait for all lines, generated once with `client = "sub_level"` and named by
  each line's client with `sub_levels = [..]` ([shared sub-traits](multi_line_host.md#shared-sub-traits), CLT-68).
* Every root needs an entry once the table exists; a crate-level `wire = ..` stays for a crate with one root and for
  crates whose traits are reached by global paths (`ww_stdlib`).
* `ww api save` saves a snapshot per root, and `ww api check` checks each root against its own latest one
  ([several API roots](checker_tool.md#several-api-roots-in-one-crate),
  [checking each root](checker_tool.md#checking-each-root)): the crate version is bumped for the root that needs the
  most, and a root removed from the table is a breaking crate bump, recorded by `ww api save`.

`ww api line new` (TOOL-33) adds a line to such a crate, and [a host for several wire lines](multi_line_host.md)
shows the whole shape on a worked example.

## Below your API

Your API's wire version covers your traits and types. Below them, the bytes are shaped by WireWeaver's own layers,
and those have versions of their own:

* **The foundation layers.** `shrink_wrap`, `ww_framer`, `ww_link` and `ww_client_server` declare a wire line in
  their `Cargo.toml` the same way your API does. Its line is the layer's **generation**, a byte in the
  [frozen hello](../transport/ww_link.md#the-frozen-hello) that opens every link (EVO-71). Peers on different
  generations of a layer fail with `Error::IncompatibleGeneration` naming the layer, before anything evolvable is
  read. See [crate layers](crate_layers.md#generations-from-the-wire-line).
* **`ww_stdlib` crates** have wire lines of their own, and a dependency on a new line of one is a new line of your
  API ([the table above](#which-bump-a-change-needs)).
* **Choices of a firmware image** that are not part of its API travel apart, in `ImageFacts`
  ([choices of one firmware image](rules.md#choices-of-one-firmware-image)).

### The ww hub and version skew

The [ww hub](../transport/hub.md#version-skew-and-handover-srv-30) (SRV-30) sits between programs built with
different WireWeaver releases and the devices, so it adds rules of its own:

* **The preamble is frozen.** Its bytes never change; new fields are new trailer entries (a new tag), which a reader
  that doesn't know them skips. That is what lets any client and any hub exchange their versions and a clear refusal.
* **What the hub serves.** A client says its generations in the preamble (EVO-23, the bytes of its
  [link hello](../transport/ww_link.md#the-frozen-hello)); the hub serves every client whose link, shrink_wrap and API
  model generations are its own. A wire-compatible change of any of them is invisible to the hub. A breaking one bumps
  that layer's generation and means: the hub keeps the previous generation's code for a while and lists it in its
  window, the way a device keeps an old protocol, so programs built before the break keep working through a newer hub
  until they are rebuilt. A client outside the window is refused (status 4) with its versions and the fix, never
  served half-way. A client from before the generations is judged by its `ww_link` and `ww_client_server` versions by
  Cargo's SemVer rule (the same major, 0.x: the same minor).
* **The user API is end to end.** The hub reads the envelope only, never a user API: a user API change is between the
  client and the device as without a hub, and the hub doesn't need rebuilding for it.
* **`ww_hub_api`** evolves like any API crate: additions are wire-compatible (an older hub answers `Unimplemented`),
  `Hub::info` and `Hub::handover` are frozen.
* **Newer replaces older.** A program that can host a newer hub than the running one has it hand over; only a strictly
  newer `ww_hub` version replaces an older one, so a PC converges on its newest hub.
