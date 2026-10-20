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
define several, `ww_dfu` has five) and the path from the trait's root. The server finds the trait wherever it sits in
its API tree and continues from there. It answers `ErrorKind::TraitNotFound` when it doesn't implement the trait, only
implements an incompatible version of its crate (major and, before 1.0, minor have to match), or only implements it
under a resource array (`ww_impl!(port[]: ww_gpio::Bank)`: a global path can't select an index). When a trait is
implemented several times, the first one in the tree (depth first, in resource id order) answers.

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

A server serves a `GlobalCompact` path like a `GlobalFull` one: it finds the trait by its id wherever it sits in the
API tree, checks that the requested version is protocol compatible with its own (same major, and same minor before
1.0), and answers `ErrorKind::TraitNotFound` otherwise, also for an id it doesn't know, a crate's id or a trait under
a resource array. Serving it costs 64 bytes of flash and about 170 instructions more than an absolute path on a
Cortex-M3 (`GlobalFull`: about 790), see `examples_mcu/mcu_qemu/src/bin/global_paths.rs`.

The host prefers the compact form: a global call to a trait the registry knows goes out with its id, whether the
client asked for `GlobalFull` or not. A device built before the trait had an id answers `TraitNotFound`; the client
then sends the same request again with `GlobalFull` and keeps using it for that trait for the rest of the
connection. The tap decoder, `ww --monitor=api` and `ww explain` name compact calls through the registry and decode
them from the trait's snapshot.

## API report

TODO: export a file describing all levels of API with IDs (DOC-5).
