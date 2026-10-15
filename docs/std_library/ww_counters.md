# ww_counters

`ww_counters` carries a device's [`cnt`](https://crates.io/crates/cnt) counters over WireWeaver: the same counters
`cnt read` and `cnt tui` show through a debug probe, now over USB, UDP or any other link, with the same views
(`ww counters`).

`cnt` counts into two buffers of 32-bit words: RAM (zeroed at boot) and backup (in memory that survives a reset). A
`u64` counter is two words, low first. A counter's word index is assigned by the linker, and what each word is (name,
group, type, unit, severity, source location) is in symbol names of the firmware's ELF only: it exists after the link,
so it can't be compiled into the same image. So the device sends **raw words** and the host decodes them with the ELF
(`cnt_core`), exactly as over a probe. The ELF is found by the firmware's **build id**.

```rust
#[ww_trait]
pub trait Counters {
    const INFO: CountersInfo<'i>;
    #[errors(Common(InvalidArgument, Unsupported))]
    fn read(buffer: CounterBuffer, offset: u16) -> Result<MaxLen<RefVec<'i, UVlq32>, 64>, CountersError<'i>>;
    #[errors(Common(Unsupported))]
    fn reset(buffer: CounterBuffer) -> Result<(), CountersError<'i>>;
    #[optional]
    #[stream(CounterChanges<'i>, counted)]
    fn watch(period: Second) -> Result<Second, CountersError<'i>>;
}
```

* `INFO` - a [`const` item](../api/properties.md#const-items): the `cnt` format version (`cnt::SIGNATURE_VERSION`,
  2 today), `ram` and `backup` (`None` without one) as `BufferInfo { used_words, capacity_words }`, and the
  `build_id` (the same as [`FirmwareInfo::BUILD_ID`](ww_firmware_info.md#build-id), empty without the note). All are
  fixed at link time, so a server provides it as a `fn` (`const_model = "INFO=fn"`).
* `read(buffer, offset)` - words of `Ram` or `Backup` from word `offset`, at most 64, never past the used words
  (empty at the end). Words are `UVlq32`: a counter at 0 costs one byte, so a 64-word buffer of mostly small counters
  is one call of about 80 bytes. `Common(Unsupported)` for the backup buffer of a device without one,
  `Common(InvalidArgument)` for an offset past the used words.
* `reset(buffer)` - zero a buffer.
* `watch(period)` - optional and not served by anything yet: changed words every `period`, for slow links where
  polling `read` costs too much. Leave it out with `omit = ["counters.watch"]`.

## Serving the counters

The helpers in `ww_counters` serve `read` and `reset` from `cnt`'s buffers. `read_words` copies one page of atomics
into a 64-word scratch in the server struct (256 bytes) at once, so a page is one snapshot. For
`counters: ww_counters::Counters` in the API root `Device`:

```rust
pub struct Server {
    scratch: [UVlq32; ww_counters::MAX_WORDS],
    // ..
}

impl api_impl::DeviceConsts for Server {
    fn counters_info(&self) -> CountersInfo<'_> {
        CountersInfo {
            format: cnt::SIGNATURE_VERSION,
            ram: BufferInfo::new(cnt::ram_used_words(), cnt::counters_ram_buffer()),
            backup: None, // Some(BufferInfo::new(cnt::bkp_used_words(), cnt::counters_bkp_buffer()))
            build_id: MaxLen::new_unchecked(RefVec::new_bytes(ww_firmware_info::build_id())),
        }
    }
}

impl Server {
    fn counters_read(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        buffer: CounterBuffer,
        offset: u16,
    ) -> RpcResult<Result<MaxLen<RefVec<'_, UVlq32>, 64>, CountersError<'_>>> {
        RpcResult::Ready(match buffer {
            CounterBuffer::Ram => {
                read_words(cnt::counters_ram_buffer(), cnt::ram_used_words(), offset, &mut self.scratch)
            }
            CounterBuffer::Backup => Err(CountersError::unsupported()),
        })
    }

    fn counters_reset(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        buffer: CounterBuffer,
    ) -> RpcResult<Result<(), CountersError<'_>>> {
        match buffer {
            CounterBuffer::Ram => reset_words(cnt::counters_ram_buffer()),
            CounterBuffer::Backup => return RpcResult::Ready(Err(CountersError::unsupported())),
        }
        RpcResult::Ready(Ok(()))
    }
}

mod api_impl {
    wire_weaver::ww_codegen!(
        device_api :: Device for super::Server,
        server = true, no_alloc = true, use_async = false,
        const_model = "INFO=fn",
        omit = ["counters.watch"],
    );
}
```

`cnt::ram_used_words()`, `cnt::bkp_used_words()` and `cnt::SIGNATURE_VERSION` are new in `cnt` 0.5. The build id
needs the note in flash, see [build id](ww_firmware_info.md#build-id); `tests/counters` has a complete server.

## On the host

```
$ ww counters tui --serial 4B333720
Using ELF target/thumbv8m.main-none-eabihf/release/usb_io_fw
```

[`ww counters`](../cli.md#ww-counters) reads `INFO`, finds the ELF with the device's build id and shows the counters
with the views of `cnt` (crate `cnt_view`): `read` (default), `watch`, `tui`, `reset`, `info`, `--format json|jsonl`.
It works with any device implementing `Counters` anywhere in its API, through a
[global trait path](../api/addressing.md).

The ELF is looked up in this order (`cnt_core::elf`):

1. `--elf PATH`: used, and **refused if its build id differs** from the device's (`--force` to use it anyway);
2. `<build id>.elf` in the directories of `WW_ELF_PATH`, then in the ELF directory `~/.cache/ww/elf`;
3. the binaries under the cargo project's `target/` (when run inside the firmware's project).

`cnt elf add <ELF or release archive>` fills the ELF directory: a single ELF, or a firmware release's
`<firmware>-<version>-elf.tar.zst` with every ELF of the release, each stored under its build id. A device without
a build id gets the newest ELF under `target/`, with a warning that it can't be checked. `INFO`'s used words are
checked against the ELF as well: an ELF that needs more words than the device uses is other firmware.

A `u64` counter can tear between its two words (a carry between reading them). The host reads a `u64` counter again
when its high word changed or its value went down since the last read, until its high word reads the same twice.

`cnt` over a probe does the same check: it reads the build id note from the target's flash and refuses an ELF of
other firmware, so neither path decodes words with the wrong names any more.

From code, `ww_counters::host::WwSource` (feature `host`) is a `cnt_core::Source` over a `CountersDevice`, the two
calls on a generated client; `cnt_core::Counters::read` / `read_changes` decode, `cnt_view` shows.

## Evolution

`CounterBuffer` is `no_future_version` (`cnt` has these two buffers); `CountersInfo`, `BufferInfo` and
`CounterChanges` are evolvable structs, `CountersError` an evolvable enum with the `Common` errors. A new `cnt` buffer
layout bumps `cnt::SIGNATURE_VERSION`, which `INFO.format` carries: the host refuses a format it doesn't read.
