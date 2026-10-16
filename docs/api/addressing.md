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

`CompactVersion` is the short form: a globally unique type id and the version numbers, all `UNib32`. Unique ids
can't come from crate names, so they are assigned by hand and tracked in git, in the
[ww_global registry](https://github.com/vhrdtech/wire_weaver/tree/master/ww_global).

Servers don't serve compact trait paths yet (`ErrorKind::TraitNotFound`): compact ids of traits are not in the API
model, and none is assigned so far (API-20).

## API report

TODO: export a file describing all levels of API with IDs (DOC-5).
