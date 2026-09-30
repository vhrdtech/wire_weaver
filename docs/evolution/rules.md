# Evolution rules

Two different things can break when a WireWeaver crate changes, and they are tracked separately:

* **Wire compatibility** - whether bytes written by one side can still be read by the other. Firmware lives in the
  field for years, hosts are updated on their own schedule, and shrink_wrap blobs may be stored on disk. A wire break
  means deployed devices stop talking to new hosts (or the other way around), which no `cargo update` can fix.
* **Rust (SemVer) compatibility** - whether code still compiles against the new crate version. A break here is
  annoying, but it is found by the compiler and fixed by the developer, before anything ships.

A change can break one without the other: renaming a `BufWriter` method breaks Rust code but not a single byte on the
wire; changing a field's `u8` to `u16` in an API type compiles fine and silently breaks the wire. The rules below say
which versions to bump for each, depending on where in the crate stack the change is.

## Crate layers

| Layer                 | Crates                                                                                                                                                              | Versioning                                                                  |
|-----------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------|
| 1. Wire format        | `shrink_wrap`, `shrink_wrap_derive`                                                                                                                                 | own version, changes rarely                                                 |
| 2. Shared API & types | `ww_stdlib/*`: base types (`ww_numeric`, `ww_si`, `ww_date_time`, `ww_version`, `ww_global`, `ww_self`) and traits (`ww_gpio`, `ww_i2c`, `ww_client_server`, ...) | own version per crate, the version is the API identity                      |
| 3. Framework          | `wire_weaver`, `wire_weaver_core`, `wire_weaver_derive`, `wire_weaver_client`, `wire_weaver_cli`, `ww_device`, `ww_link`, `ww_framer`, `wire_weaver_udp_link`, ...  | shared `[workspace.package] version`, free to break Rust API                |
| 4. User API           | `my_device_api` and friends (see [folder structure](../api/folder_structure.md))                                                                                    | own version, the version is the API identity, checked on every connection |

Everything above layer 1 implements shrink_wrap's traits. Base type crates depend on `shrink_wrap` directly, trait
crates and user API crates depend on `wire_weaver`, which re-exports `shrink_wrap`. Generated code refers to
`shrink_wrap::...` through that re-export, so a single `shrink_wrap` version must be shared by the whole dependency
graph of an application.

### Layer 1: shrink_wrap

`shrink_wrap` is the foundation that every other crate and every user type is built on, so it is kept stable and
changes to it are rare and deliberate.

**Wire format changes** - anything that changes the bytes produced for an existing type (alignment, length
encoding, discriminant or flag layout, the meaning of an existing `derive_shrink_wrap` directive) breaks every
device, every host and every stored blob at once, regardless of any API version. Post 1.0 this is not allowed at all.
Before 1.0 it is allowed only with a strong reason, batched with other such changes, and listed under `### ⚠️
Breaking` in the changelog starting with `Wire format:`, so that it can't be mistaken for a Rust-only break. The
byte-level claims in [shrink_wrap](../serdes/shrink_wrap.md), [derive](../serdes/derive.md) and
[showcase](../serdes/showcase.md) must be re-verified in the same change.

**Rust API breaks** (`0.2` -> `0.3`) do not touch the wire, but they still ripple through the whole ecosystem: to
Cargo, `shrink_wrap 0.2` and `0.3` are unrelated crates with unrelated `SerializeShrinkWrap` traits. A type
from a `ww_stdlib` crate still built on 0.2 does not implement 0.3 traits, and can't be used inside a type or an API
built on 0.3. So every `ww_stdlib` crate, the framework and every user API crate has to be re-released together.
Therefore:

* Prefer additive changes (new types, new methods, new directives, new features). They do not break anything and
  are released as a patch version (`0.2.0` -> `0.2.1`), so the rest of the ecosystem picks them up without
  re-releasing. This is the one exception to the "every change bumps minor" rule.
* Collect breaking Rust API changes and release them together, rarely. When it happens, bump all `ww_stdlib` crates
  in the same release cycle.
* `shrink_wrap_derive` is versioned together with `shrink_wrap`, as the generated code is tightly coupled to it.

### Layer 2: ww_stdlib

`ww_stdlib` crates are meant to be shared between unrelated projects, so their types end up inside many
different APIs. Their versions travel on the wire (as `FullVersion` / `CompactVersion` with a
[ww_global](https://github.com/vhrdtech/wire_weaver/tree/master/ww_stdlib/ww_global) id) for trait-based requests
and introspection, so the version is not just Cargo metadata, it's a compatibility statement, same as for user API
crates (layer 4):

* Wire-compatible change (following the [data type](#data-types) and [API](#api) rules below) -> bump the compatible
  position: patch before 1.0 (`0.1.0` -> `0.1.1`), minor after.
* Wire-incompatible change -> bump the breaking position: minor before 1.0 (`0.1.0` -> `0.2.0`), major after. Every
  crate and user API that exposes the changed type or trait over the wire is then wire-incompatible too, and has to
  bump its own breaking position when it upgrades.
* A Rust-only break (e.g. renaming a helper method, changing a `std` convenience impl) is still a Cargo breaking
  change and bumps the breaking position, even though the wire is unaffected. Avoid mixing such changes with
  releases that are supposed to be wire-compatible additions; prefer deprecating and adding.

Base type crates (`ww_numeric`, `ww_si`, ...) are used by other `ww_stdlib` crates as well, so their breaking
changes cascade the same way `shrink_wrap` ones do, just over a smaller set of crates - check reverse dependencies in
the root `Cargo.toml` before breaking one. `ww_version` and `ww_global` are special: `FullVersion`,
`CompactVersion` and `ApiHashPair` are exchanged during link setup, before any version could be checked, so they are
`final_structure` and must not change on the wire at all.

`ww_client_server` is the API model: it defines the request/event encoding that every generated server and client
speaks. Its version is sent to the host in `DeviceInfo::api_model_version`, and a wire break there is a wire break for
every user API, so it is treated with almost the same care as `shrink_wrap` - see [API model](#api-model).

### Layer 3: framework

The framework crates share one version and are free to break their Rust API: bump the minor version once per
release cycle and describe what users must change in the changelog. A user API crate built against `wire_weaver 0.5`
keeps working with a host or firmware built against `wire_weaver 0.6`, as long as both still use the same
`shrink_wrap` major version and the same wire protocol.

The exception is code that defines what goes on the wire:

* `ww_framer` (frame layout, heads, checksums) and `ww_link` (message kinds, `DeviceInfo`, `LinkSetup`) - changing
  these breaks every device from every host. The link version is sent in `DeviceInfo::dev_link_version` but it is not
  checked yet, so a break there is not even detected cleanly. Only add new message kinds that old peers ignore;
  never change existing ones.
* Server and client codegen in `wire_weaver_core` - how resources are indexed, how arguments and return values are
  wrapped. Changing it changes the wire for every user API without any user version changing, so it must stay
  byte-identical for the same API definition.

These are wire changes even though they live in "free to break" crates, so they follow the layer 1 rules: avoid,
batch, and mark as `Wire format:` in the changelog.

### Layer 4: user API

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

## Checklist before breaking something

1. Is it a wire change or only a Rust change? If unsure, serialize a few values before and after and compare the bytes
   (`to_ww_bytes()`), and run the [evolution checker](checker_tool.md).
2. Which layer is it in? Wire changes in `shrink_wrap`, `ww_framer`, `ww_link`, `ww_client_server` or server/client
   codegen affect every device - avoid them, and if unavoidable, batch them and mark them `Wire format:` in the
   changelog.
3. For `shrink_wrap` (and base `ww_stdlib` types), list the reverse dependencies that need to be re-released.
4. Bump the right position: patch for compatible additions in `shrink_wrap` and API crates before 1.0, minor for
   everything else before 1.0.
5. Write the changelog entry, saying what users must change and whether deployed devices are affected.

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

* New fields with default capability can be added to the end of structs, enum struct and tuple variants.
    * `Option<T>` - None is read from old data,
    * `Vec<T>` - Empty vector is read from old data,
    * `String` - Empty string is read from old data,
    * `T` can be anything.
* New `Sized` fields can be added into previously unused padding bits.
* TODO: clarify: `T` -> struct containing `T`
* Struct fields and enum variants can be renamed (but their position must NOT change).

### FinalStructure, SelfDescribing and Sized types

* New `Sized` fields can be added into previously unused padding bits.
* Struct fields and enum variants can be renamed (but their position must NOT change).

## API

Data types used in API are a part of SemVer guarantee and are subject to the rules above. Meaning that it's not allowed
to break compatibility on any of the data types used directly or indirectly without also bumping the breaking position
of the API version as well (minor before 1.0, major after, see [layer 4](#layer-4-user-api)).

* Adding argument
* `T` to struct of `T` in return position
* `T` to `Vec<T>` in return position?

### API model

API model (like `ww_client_server`) is part of the compatibility equation, it is not allowed to update the model
version without breaking compatibility.

E.g., if user_device_api v0.1.0 depends on ww_client_server v0.4.0 and a new major version of ww_client_server comes
out (v0.5.0), then user_device_api must be bumped to v0.2.0 to use the newer API model. This should only happen to add
new features though, and if previous version is doing all that it is supposed to, there might not be a need to
upgrade.
