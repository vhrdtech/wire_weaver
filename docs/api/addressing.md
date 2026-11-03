---
level: nomicon
---

# Resources addressing

Every resource (method, property, stream, sink or trait) is reached by its **path**: an array of indices from the API
root, each a `UNib32` (4 bits for `0..=7`). `[0, 1, 2]` takes 2 bytes, its length included. As a user you never
write these indices: they are assigned in declaration order, and generated code uses the names.

```rust
#[ww_api_root]
trait ApiRoot {
    fn turn_on();               // [0]
    fn turn_off();              // [1]
    ww_impl!(motor: Motor);     // [2, ..]
    ww_impl!(port[]: Port);     // [3, <index>, ..]
}
```

A nested trait adds one index for its own items, an array one more for the selected index; with several arrays on
the way, each puts its index at its place in the path, and the server's handler gets them all as its index chain.

A path only means something for one API and version, so it needs the host to know the device's API. When that is not
possible or not wanted (calling a reusable trait on any device, or on many devices at once), a request names the
trait instead.

## Global trait addressing (FullVersion)

A request can call a reusable trait without knowing where it sits: put a device to sleep through its power management
trait, or read the firmware version of any device with `ww_firmware_info`. Instead of an absolute path, it carries
the trait's crate name and version (`FullVersion`). That takes 8 to 16 bytes or more, depending on the crate name,
where a path takes a few: costly on a constrained link, or when many calls share one packet.

The request carries the trait's crate name and version (`FullVersion`), the trait's name in that crate (a crate can
define several, `ww_dfu` has five) and the path from the trait's root. The trait is found wherever it sits in the
device's API tree and the request continues from there: the trait's crate must be the requested one in a protocol
compatible version (major and, before 1.0, minor have to match) and its name the requested one. When a trait is
implemented several times, the first one in the tree (depth first, in resource id order) answers. Who finds it depends
on whether the device has introspection, see [Who resolves a global path](#who-resolves-a-global-path).

Global paths don't depend on the user API or its version, which makes them the way to talk to a device whose API the
host doesn't know: a host built against a new API version can still reboot a device running an old one into its
updater (`ww_dfu::BootControl`) or read its info traits. On the host, connect with `traits_only` and call through a
trait client:

```rust
// any device, whatever its API and version: the user API check of link setup is skipped
let device = DynClient::traits_only(|c| c.usb().serial_eq(serial)).connect().await?;
// a client generated with `client = "trait_client"` for ww_dfu::BootControl, see tests/cross_version
let mut boot = BootControl::new(device.global_attachment(ww_dfu::BOOT_CONTROL_FULL_GID, "BootControl"));
boot.reboot(BootTarget::Updater).call().await?;
```

A `traits_only` connection refuses absolute paths (`Error::AbsolutePathInTraitsOnly`): without the device's API they
could mean anything. `ww dfu reboot` in the [CLI](../cli.md) does exactly this.

## Global trait addressing (CompactVersion)

`CompactVersion` is the short form: the trait's id in the
[ww_global registry](https://github.com/vhrdtech/wire_weaver/tree/master/ww_global) and its crate's version numbers,
all `UNib32`. A `ww_user_info::UserInfo` read takes 6 bytes this way and 24 with `FullVersion`.

Every public trait in `ww_stdlib` has an id, one per trait, keyed `crate::Trait`, in the same number space as the few
crates that have one (`ww_log` = 0, `wire_weaver_usb_link` = 512, `ww_client_server` = 513). Small numbers are
shorter on the wire, so the ranges go by encoded size:

| Ids     | Size     | For                                                                                     |
|---------|----------|-----------------------------------------------------------------------------------------|
| 0-7     | 1 nibble | `ww_log` (crate) and `ww_log::LogDefmt` only, 2-7 kept free                              |
| 8-63    | 1 byte   | the device contract and common IO: firmware and board info, identity, counters, firmware update, clock, indication, GPIO, UART, I2C, SPI, CAN, ADC, PWM |
| 64-511  | 1.5 byte | all other reusable traits                                                               |
| 512+    | 2 bytes  | link and protocol crates                                                                |

Ids are assigned by hand in `ww_global/wire_weaver_gid.json` and nowhere else: code never carries the number. The
codegen looks a trait's id up by its crate and trait name (`<TRAIT>_COMPACT_GID`, `ApiLevel::compact_id` in the API
model), so moving a trait to another module, reordering traits or adding a sibling trait changes nothing. A rename or
a move to another crate keeps the id (the old name goes into `aliases`), ids are never reused (a removed trait is
marked `retired`), and `assigned_ids.lock` keeps every id ever assigned, checked by a test. `ww api check` reports a
trait whose id changed or disappeared as a breaking change.

A `GlobalCompact` path is resolved like a `GlobalFull` one: the trait is found by its id wherever it sits in the
API tree, the requested version must be protocol compatible with the device's (same major, and same minor before
1.0), and an id the device doesn't know, a crate's id or an incompatible version is answered with
`ErrorKind::TraitNotFound`. A server serving it by itself spends 64 bytes of flash and about 170 instructions more
than for an absolute path on a Cortex-M3, see `examples_mcu/mcu_qemu/src/bin/global_paths.rs`.

The host prefers the compact form: a global call to a trait the registry knows goes out with its id, whether the
client asked for `GlobalFull` or not. A device built before the trait had an id answers `TraitNotFound`; the client
then sends the same request again with `GlobalFull` and keeps using it for that trait for the rest of the
connection. The tap decoder, `ww --monitor=api` and `ww explain` name compact calls through the registry and decode
them from the trait's snapshot.

## Who resolves a global path

A device with introspection resolves no global path itself, the client does (SRV-53). Its introspection data already
names every trait with its crate and version, so the client finds the trait there and sends the request with the
trait's absolute path. The bytes a lookup would take on the device go to the introspection data instead, docs
included, which is far more useful to a user than a second way to address traits:

- When the client knows the device's API (a dynamic or generated client gets it at connect, from the local cache or
  the device), a global call goes out with the absolute path right away. A capture then names it by the device's API
  (`user.name` instead of `ww_user_info::UserInfo.name`).
- On a `traits_only` connection the client doesn't know it: the call goes out with its global path, the device
  answers `TraitNotFound` (to the compact path, then to the full one), and the client gets the introspection data
  once (from the local cache, else from the device), finds the trait and sends the call again with the absolute path.
  Later calls on that connection are resolved at once. A trait the data doesn't have is answered with
  `TraitNotFound`, as before.
- A trait under a resource array becomes reachable this way, at index 0 (`[port, 0, ..]`), which the device's own
  lookup can't do.

Trait clients, `DynClient::global_attachment`, the `ww` commands and the hub work the same either way.

A device without introspection resolves global paths itself, without names in its flash: it has a `const` table of
the reachable traits, each the 32-bit FNV-1a hash of its crate and trait name (`ww_global::trait_hash`), its crate's
major and minor version and its absolute path. It hashes the crate and trait name of a `GlobalFull` request (one
multiply per byte, no 64-bit arithmetic on a Cortex-M0), finds the entry and checks the version after the hash
matched. Two traits of one API with the same hash are a compile error of `ww_codegen!`; rename one, or serve compact
ids only. A trait under a resource array is not in the table: a global path can't select an index. `GlobalCompact`
paths are served by the traits' `ww_global` ids, as above.

`ww_codegen!(.., global_paths = "..")` chooses differently from these defaults:

| `global_paths` | `GlobalFull`    | `GlobalCompact`   |
|----------------|-----------------|-------------------|
| `"full"`       | by hash         | by `ww_global` id |
| `"compact"`    | `TraitNotFound` | by `ww_global` id |
| `"none"`       | `TraitNotFound` | `TraitNotFound`   |

Not given, it is `"full"` without introspection and `"none"` with it.

Per trait reachable this way, a server without introspection spends 80 bytes at `fw-z` on a Cortex-M0+ (its level
included), 40 bytes less than matching the names did; one with introspection carries no lookup at all, 120-156 bytes
less. See [What a server costs in flash](../articles/flash_budget.md#global-trait-paths-by-hash-srv-53).

## API report

TODO: export a file describing all levels of API with IDs (DOC-5).
