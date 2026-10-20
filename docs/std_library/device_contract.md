---
level: basic
---

# Device contract

Six `ww_stdlib` crates that every device implements, whatever it does. Together they answer what a host, a tester's
bug report or a production line asks of any unit: *what firmware is this and where is its ELF, what board, which
unit, what has it counted, what did it log, and why did it last reset.* One host tool, [`ww info`](../cli_reference.md#ww-info)
and [`ww report`](../cli_reference.md#ww-report), asks the same questions of every device, because all of them implement the
same global traits.

| Question | Crate | Trait | What `ww` shows |
|---|---|---|---|
| What firmware, which build? | [`ww_firmware_info`](ww_firmware_info.md) | `FirmwareInfo` | name, version, git, build time, profile, **build id** |
| Why did it last reset? | [`ww_firmware_info`](ww_firmware_info.md) | `LastReset` | cause, boot count, fault registers, stack, panic message |
| Which board? | [`ww_board_info`](ww_board_info.md) | `BoardInfo` | name, revision and where it came from, variant, BOM, serial, optional list of reworks |
| Which unit? | [`ww_uid`](ww_uid.md) | `Uid` | chip id, MAC, ..., the first is the primary id |
| What did the user call it? | [`ww_user_info`](ww_user_info.md) | `UserInfo` | label and notes, whether they survive a power cycle |
| What has it counted? | [`ww_counters`](ww_counters.md) | `Counters` | the `cnt` counters, decoded with the ELF |
| What did it log? | [`ww_log`](ww_log.md) | `LogDefmt` | the stored `defmt` log, decoded with the ELF |

## Where to put them

Implement each trait once, at any depth of the API tree. The conventional place is one `Device` subtree of the API
root:

```rust
#[ww_trait]
pub trait Device {
    ww_impl!(firmware: ww_firmware_info::FirmwareInfo);
    ww_impl!(last_reset: ww_firmware_info::LastReset);
    ww_impl!(board: ww_board_info::BoardInfo);
    ww_impl!(uid: ww_uid::Uid);
    ww_impl!(label: ww_user_info::UserInfo);
    ww_impl!(counters: ww_counters::Counters);
    ww_impl!(log: ww_log::LogDefmt);
}

#[ww_api_root]
pub trait MyDevice {
    ww_impl!(device: Device);
    // ... what the device does
}
```

Where they sit doesn't matter to `ww`: its calls carry a *global trait path* (crate, version, trait name) that the
device resolves itself ([addressing](../api/addressing.md)), so `ww info` works with a device whatever its own API and
whatever version of it, also one flashed years ago. A device that doesn't implement a trait, or implements an
incompatible version of it, is reported as such for that part; the rest is still shown.

Every item of the first four crates is a `const`: the device serves it from a value, without a handler
([const items](../api/properties.md#const-items)), and the host reads it once per connection.

## Linking a firmware to its ELF: the build id

Three of the traits need the firmware's **ELF** on the host: `defmt` logs (the format strings are in the ELF, not on the
wire), `cnt` counters (names, types and units are in non-loaded sections of the ELF), and a crash record (addresses
to name). The ELF is found by the firmware's **GNU build id**: the linker hashes the linked output, so it changes with
every byte of code, and the device has it in flash while the ELF has it in a note.

Three lines make a firmware report it:

1. The linker writes the note. In `.cargo/config.toml` (or the build's `RUSTFLAGS`):

    ```toml
    [target.thumbv8m.main-none-eabihf]
    rustflags = ["-C", "link-arg=--build-id=sha1"]
    ```

2. `memory.x` keeps the note in flash and names its bounds:

    ```text
    SECTIONS {
        .note.gnu.build-id : ALIGN(4)
        {
            __build_id_start = .;
            KEEP(*(.note.gnu.build-id));
            __build_id_end = .;
        } > FLASH
    } INSERT AFTER .rodata;
    ```

3. The firmware reads it where the traits ask for it, `ww_firmware_info::build_id()` (feature `build-id`). The same
   value goes into `FirmwareInfo::BUILD_ID`, `DefmtInfo::build_id` and `CountersInfo::build_id`, so each trait is
   usable alone. `build.rs` of the firmware calls `ww_firmware_info::build::emit()` for the rest of
   `FirmwareInfo::FIRMWARE` (see [`ww_firmware_info`](ww_firmware_info.md)).

Without the note the build id is empty. `ww` then can't tell which ELF belongs to the device: it uses only an ELF
passed with `--elf`, and says it couldn't check it.

## Where `ww` looks for the ELF

For each build id the device reports, in this order:

1. `--elf PATH`: used, and **refused** if its build id differs from the device's (`--force` to use it anyway).
2. The ELF directory: `<build id>.elf` in each directory of `WW_ELF_PATH`, then `~/.cache/ww/elf`.
3. The ELFs of the cargo project in the current directory (`target/`), the one with that build id.

Nothing is found by guessing: when no ELF has the device's build id, the parts that need one say so and name the
places searched. `--force` takes the newest binary under `target/` instead, with a warning.

The ELF directory is filled with `ww elf add`:

```sh
ww elf add target/thumbv8m.main-none-eabihf/release/usb_io_fw   # one ELF, stored as <build id>.elf
ww elf add usb_io_fw-0.5.0-elf.tar.zst                          # a whole release
ww elf id usb_io_fw                                             # its build id, and whether it is in the directory
```

`ww elf add` and `cnt elf add` are the same code (`cnt_core::elf`), they fill one directory.

## The release archive

A firmware release ships **one archive of all its ELFs**, not loose files: `<firmware>-<version>-elf.tar.zst`, with
every board variant and profile of the release. A tester who got a bug report from a unit runs `ww elf add` on the
archive of the version printed in the report, and `ww report` finds the right ELF by build id from then on.

```sh
# at release time, in the firmware's repo: every ELF of the release, and a plain list of build ids to read
for elf in release/*.elf; do echo "$(ww elf id $elf) $(basename $elf)"; done > index.txt
tar --zstd -cf usb_io_fw-0.5.0-elf.tar.zst index.txt release/*.elf
```

`ww elf add` reads each ELF's build id from the ELF itself, so a file in the archive that is not an ELF (the index,
notes) is ignored and an ELF without a build id is listed as skipped. The version a unit runs is in its report
(`FirmwareInfo::FIRMWARE`), which says which archive to fetch.

## `ww info`

```text
$ ww info --serial 4B333720
API        usb_io_api@0.5.0
Firmware   b200_usb_io 0.5.0 (a1b2c3d-dirty, built 2026-10-03 18:20), release
Build id   28a4e144e867138da2cccccd14e3e6baa2c78fbc
Board      rePort rev B (strap 2), BOM std, serial RP-0042
Ids        chip e6613871235b1c2d (internal, primary)
Label      "bench 2", notes "DUT of the CAN test" (kept over power cycles)
Last reset Panic, boot 3 since cleared, ran 2.5 s
            message: index out of bounds
```

The same data as JSON with `--format json` (each part is `{"status": "value" | "not_implemented" | "error", ...}`).
It needs no ELF and no user API.

## `ww report`

`ww report` is `ww info` plus the counters, the stored log and the last reset's addresses named with the ELF, in one
file a tester sends along with a bug report:

```sh
ww report --serial 4B333720 -o report.md
```

It has the sections of the output above, then:

* **Last reset**: the fault's `pc` and `lr` and the stack words named by function (`fn+0x1c`) from the ELF of the
  firmware that *crashed* (the record carries its build id: it may be older than an update since).
* **Counters**: every `cnt` counter of RAM and backup with its value and unit, decoded with the ELF.
* **Log**: what the device's log store holds, from the first message since boot, decoded with `defmt`; gaps are
  shown where the device dropped messages. Reading it does not empty the store.

Each part that needs an ELF it couldn't get says why and the rest of the report is still complete, so a tester
without the archive still sends something useful. `--elf`, `--force` and `--format json` behave as for
[`ww counters`](ww_counters.md#on-the-host).

The log is a stream, which is opened on the trait's absolute path in the device's API (the host reads the device's
introspection data for it, it needs no API of its own): that is the one part where `ww report` is more than global
trait calls.

## On the host (Rust)

`ww_report` is the library behind both commands:

```rust
let device = DynClient::from_config(config).connect().await?;
let contract = ww_report::clients::Contract::new(&device);   // keep it: dropping it disconnects the device
let report = ww_report::report::collect_report(&contract, &ReportOptions::default()).await;
println!("{}", ww_report::report::render_report(&report));
```

`tests/device_report` runs it against a mock device implementing every trait of the contract.
