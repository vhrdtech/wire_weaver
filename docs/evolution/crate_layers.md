---
level: advanced
---

# Crate layers and version bumps

For changing WireWeaver itself or a `ww_stdlib` crate. An application only needs the
[evolution rules](rules.md) for its own API crate.

Every crate in the stack has its own versioning rules, because a change travels differently depending on where it
is made: a `shrink_wrap` break cascades to every other crate, a framework crate is free to break its Rust API, and
some framework code defines bytes on the wire although it lives in a "free to break" crate.

## The layers

| Layer                 | Crates                                                                                                                                                              | Versioning                                                                  |
|-----------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------|
| 1. Wire format        | `shrink_wrap`, `shrink_wrap_derive`                                                                                                                                 | own version, changes rarely                                                 |
| 2. Shared API & types | `exact_units` (repo root, the units and `Fixed` under `ww_si`); `ww_stdlib/*`: base types (`ww_numeric`, `ww_si`, `ww_date_time`, `ww_version`) and traits (`ww_gpio`, `ww_i2c`, `ww_client_server`, ...); framework base types `ww_global` and `ww_self` (repo root) | own version per crate, the version is the API identity                      |
| 3. Framework          | `wire_weaver`, `wire_weaver_core`, `wire_weaver_derive`, `wire_weaver_client`, `wire_weaver_cli`, `ww_device`, `ww_link`, `ww_framer`, ...  | shared `[workspace.package] version`, free to break Rust API                |
| 4. User API           | `my_device_api` and friends (see [folder structure](../api/folder_structure.md))                                                                                    | own version, the version is the API identity, checked on every connection |

Everything above layer 1 implements ShrinkWrap's traits. Base type crates depend on `shrink_wrap` directly, trait
crates and user API crates depend on `wire_weaver`, which re-exports `shrink_wrap`. Generated code refers to
`shrink_wrap::...` through that re-export, so a single `shrink_wrap` version must be shared by the whole dependency
graph of an application.

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

## Layer 1: shrink_wrap

`shrink_wrap` is the foundation that every other crate and every user type is built on, so it is kept stable and
changes to it are rare and deliberate.

**Wire format changes** - anything that changes the bytes produced for an existing type (alignment, length
encoding, discriminant or flag layout, the meaning of an existing `derive_shrink_wrap` directive) breaks every
device, every host and every stored blob at once, regardless of any API version. Post 1.0 this is not allowed at all.
Before 1.0 it is allowed only with a strong reason, batched with other such changes, and listed under `### ⚠️
Breaking` in the changelog starting with `Wire format:`, so that it can't be mistaken for a Rust-only break. The
byte-level claims in the [SerDes](../serdes/shrink_wrap.md) pages ([wire format internals](../serdes/wire_format.md),
[showcase](../serdes/showcase.md), ...) must be re-verified in the same change.

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

## Layer 2: ww_stdlib

`ww_stdlib` crates are meant to be shared between unrelated projects, so their types end up inside many
different APIs. Their versions travel on the wire (as `FullVersion` / `CompactVersion` with a
[ww_global](https://github.com/vhrdtech/wire_weaver/tree/master/ww_global) id) for trait-based requests
and introspection, so the version is not just Cargo metadata, it's a compatibility statement, same as for user API
crates (layer 4):

* Wire-compatible change (following the [data type](rules.md#data-types) and [API](rules.md#api) rules) -> bump the
  compatible position: patch before 1.0 (`0.1.0` -> `0.1.1`), minor after.
* Wire-incompatible change -> bump the breaking position: minor before 1.0 (`0.1.0` -> `0.2.0`), major after. Every
  crate and user API that exposes the changed type or trait over the wire is then wire-incompatible too, and has to
  bump its own breaking position when it upgrades.
* A Rust-only break (e.g. renaming a helper method, changing a `std` convenience impl) is still a Cargo breaking
  change and bumps the breaking position, even though the wire is unaffected. Avoid mixing such changes with
  releases that are supposed to be wire-compatible additions; prefer deprecating and adding.

`exact_units` is the bottom of this layer: the unit markers, `Dim`, `Magnitude`, `SIExpr` and `Fixed`, which `ww_si`
re-exports. A unit's identity on the wire includes its crate (a counted base is `exact_units` + its marker name), so
moving a marker to another crate is a wire change, and a break in `exact_units` is a break in `ww_si` and everything
built on it.

Base type crates (`exact_units`, `ww_numeric`, `ww_si`, ...) are used by other `ww_stdlib` crates as well, so their breaking
changes cascade the same way `shrink_wrap` ones do, just over a smaller set of crates - check reverse dependencies in
the root `Cargo.toml` before breaking one. `ww_version` and `ww_global` are special: `FullVersion`,
`CompactVersion` and `ApiHashPair` are exchanged during link setup, before any version could be checked, so they are
`final_structure` and must not change on the wire at all.

`ww_client_server` is the API model: it defines the request/event encoding that every generated server and client
speaks. Its version is sent to the host in `DeviceInfo::api_model_version`, and a wire break there is a wire break for
every user API, so it is treated with almost the same care as `shrink_wrap` - see [API model](rules.md#api-model).

## Layer 3: framework

The framework crates share one version and are free to break their Rust API: bump the minor version once per
release cycle and describe what users must change in the changelog. A user API crate built against `wire_weaver 0.5`
keeps working with a host or firmware built against `wire_weaver 0.6`, as long as both still use the same
`shrink_wrap` major version and the same wire protocol.

The exception is code that defines what goes on the wire:

* `ww_framer` (frame layout, heads, checksums, frame prefixes) and `ww_link` (message kinds, `DeviceInfo`,
  `LinkSetup`, each medium's `Framing`) - changing these breaks every device from every host. The host checks the
  device's `DeviceInfo::dev_link_version` against its own `LINK_VERSION` and the setup token
  ([link setup](../transport/ww_link.md#setup-token)), and fails with `Error::IncompatibleLinkVersion` instead of a
  setup timeout. Only add new message kinds that old peers ignore; never change existing ones. The last such break
  was TR-15 (0.5): the setup token in `GetDeviceInfo` / `DeviceInfo` / `LinkSetup` / `LinkReady`, USB packets
  without the split-message CRC-16, UDP datagrams with a frame counter and a CRC-32 on every message. A device and a
  host on both sides of it don't connect over USB or UDP; RTT and WebSocket only miss the token, which is refused as
  well, so every firmware and host is rebuilt together.
* Server and client codegen in `wire_weaver_core` - how resources are indexed, how arguments and return values are
  wrapped. Changing it changes the wire for every user API without any user version changing, so it must stay
  byte-identical for the same API definition.
* The settings record format and store layout of `ww_settings` (see
  [settings stored on a device](rules.md#settings-stored-on-a-device)).

These are wire changes even though they live in "free to break" crates, so they follow the layer 1 rules: avoid,
batch, and mark as `Wire format:` in the changelog.

## Layer 4: user API

See [your API's version](rules.md#your-apis-version) in the evolution rules.
