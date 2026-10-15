# ww_uid

`ww_uid` lists the unique ids of a unit: a chip's factory id, a MAC address, an EUI-64. One trait implemented once
can carry several, which one-trait-per-id could not (a trait cannot be implemented twice on one server).

```rust
#[ww_trait]
pub trait Uid {
    const IDS: MaxLen<RefVec<'i, UniqueId<'i>>, 4>;
}
```

`IDS` is a `const` item (served by a `get_<path>_IDS` handler until API-36 lands). At most 4 ids.

## The primary id

The **first id is the primary id**. A device puts its hex (lower case, no separators) into the USB serial number
string and a host shows it in `ww list`. When the unit reboots into a ROM bootloader that reports the same id
(RP2350 BOOTSEL, STM32 DFU) the host finds it again by it; see [follow reboot](../transport/follow_reboot.md).

## `UniqueId`

* `kind: UidKind` - `ChipUnique`, `Mac48`, `Eui64`, `Uuid`, `Other(name)`.
* `source: UidSource` - `InternalIc`, `ExternalIc`, `Firmware`, `HardwareStraps`, `Other(name)`.
* `value` - up to 16 bytes.

Both enums are public and evolvable: a variant from a newer version reads as `Unknown(u32)`.
