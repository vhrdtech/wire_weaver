---
level: basic
---

# ww_firmware_info

`ww_firmware_info` answers two questions about any device: **which firmware is this** (`FirmwareInfo`) and **why did
it last reset** (`LastReset`). It is `no_std` by default; `std` adds the owned types and every architecture's fault
record.

```toml
[dependencies]
ww_firmware_info = { version = "0.1", default-features = false, features = ["build-id", "cortex-m"] }

[build-dependencies]
ww_firmware_info = { version = "0.1", features = ["build"] }
```

## `FirmwareInfo`

```rust
#[ww_trait]
pub trait FirmwareInfo {
    const FIRMWARE: FirmwareSummary<'i>;
    const BUILD_ID: MaxLen<RefVec<'i, u8>, 32>;
    #[optional]
    const BUILD_RECORD: MaxLen<RefVec<'i, u8>, 512>;
}
```

* `FIRMWARE` - name, `ww_version::Version`, git commit (20 bytes) and dirty flag, build time and profile. A host
  prints `b200_usb_io 0.5.0 (a1b2c3d-dirty, built 3 Oct 2026 18:20)` from it, as our CLIs print `--version`.
* `BUILD_ID` - the **GNU build id** of the image. It is the firmware's identity: the linker hashes the linked output,
  so it changes with every byte of code, unlike a git commit (dirty builds, two profiles of one commit) or a version.
  A host finds the firmware's ELF by it, which defmt logs, `cnt` counters and crash records need. Empty when the
  firmware did not keep the note.
* `BUILD_RECORD` - the `bedrock_build_info` compact record as stored in flash (crate features, dependencies,
  compiler, target); the full one is in the ELF only. Optional: leave it out with `omit = ["firmware.BUILD_RECORD"]`.

All three are [`const` items](../api/properties.md#const-items): they do not change while the firmware runs, a server
provides them without handlers and a host reads each once per connection. `FIRMWARE` and `BUILD_RECORD` are Rust
consts, `BUILD_ID` is known at link time only, so it is a `fn` (`const_model = "BUILD_ID=fn"`); `ww api scaffold`
prints the impl with the exact names.

### Producing the summary

`build.rs` calls `ww_firmware_info::build::emit()` (feature `build`, std, no extra crates). It runs `git rev-parse
HEAD` and `git status --porcelain`, reads the package name and version from Cargo, uses `SOURCE_DATE_EPOCH` as the
build time when set (reproducible builds) and sets `WW_FW_*` variables with `cargo:rustc-env`. It asks Cargo to
rerun when `.git/HEAD` or `.git/index` changes. `firmware_summary!()` builds the value at compile time, for
`firmware: FirmwareInfo` in the API root `Device`:

```rust
impl api_impl::DeviceConsts for Server {
    const FIRMWARE_FIRMWARE: FirmwareSummary<'static> = ww_firmware_info::firmware_summary!();

    fn firmware_build_id(&self) -> MaxLen<RefVec<'_, u8>, BUILD_ID_MAX_LEN> {
        MaxLen::new_unchecked(RefVec::new_bytes(ww_firmware_info::build_id()))
    }

    const FIRMWARE_BUILD_RECORD: MaxLen<RefVec<'static, u8>, 512> = /* bedrock's COMPACT record */;
}
```

Outside git the commit is empty and `git` is `None`. Strings are cut at their bounds.

### Build id

Ask the linker for the note and keep it in flash:

```toml
# .cargo/config.toml
[target.thumbv8m.main-none-eabihf]
rustflags = ["-C", "link-arg=--build-id=sha1"]
```

```text
/* memory.x */
SECTIONS {
  .note.gnu.build-id : ALIGN(4) {
    __build_id_start = .;
    KEEP(*(.note.gnu.build-id))
    __build_id_end = .;
  } > FLASH
} INSERT AFTER .rodata;
```

With the `build-id` feature, `ww_firmware_info::build_id()` returns the bytes of the note between the two symbols
(`parse_build_id_note` does the parsing and is available everywhere, for hosts reading an ELF's section).

## `LastReset`

```rust
#[ww_trait]
pub trait LastReset {
    fn last_reset() -> ResetRecord<'i>;
    fn boot_count() -> u32;
    fn clear();
}
```

`ResetRecord` has the `cause` (`PowerOn`, `Brownout`, `Pin`, `Watchdog`, `Software`, `Lockup`, `Fault`, `Panic`,
`Updated`, `Undetermined`), the `build_id` of the firmware that crashed (the record may be older than an update),
`uptime` at the crash, the architecture's `fault` registers, up to 32 words of `stack`, a `message` of up to 128
bytes and `boots_since_crash` (0: the last reset was the crash; a crash kept across later normal resets shows a
`cause` other than `Fault` / `Panic` and a count above 0). `boot_count` with the cause tells a reset loop from a single crash; `clear` forgets both once the host saved
them. Capturing the record (a fault or panic handler writing into RAM that survives a reset) is firmware
infrastructure; this trait only reports it.

`Fault` has one variant per architecture behind a cargo feature: `CortexM(CortexMFault)` (`cortex-m`) and
`RiscV(RiscVFault)` (`riscv`); `std` enables all. A variant keeps its number whether it is compiled in or not
(see [`#[cfg]` on enum variants](../serdes/derive_reference.md#cfg-on-enum-variants)), so firmware enables the one it needs and a
host all of them. At least one must be enabled for `ResetRecord` to be usable.

## Evolution

Everything is evolvable: `FirmwareSummary`, `ResetRecord` and the enums can grow with `#[default]` fields and new
variants. Hosts print `Unknown` causes as numbers.
