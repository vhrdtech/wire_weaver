## Unreleased

### ⚠️ Breaking

- `--elf` is one global flag now (TOOL-20): `ww counters --elf X` and `ww report --elf X` work as before, `--elf` also goes before the subcommand, and with `--rtt` it gives the RTT control block. `WW_ELF` and the `elf` key in `ww.toml` are for the control block only; `counters` and `report` still look up the ELF by build id unless `--elf` is on the command line.

### 🚀 Features

- `ww api check --strict` fails when the crate has no snapshot to compare with, naming the `ww api save` to run, instead of passing unchecked (EVO-11). Each breaking change is followed by the rule it breaks and its docs section, and a failed check says which version to put in `Cargo.toml`. `ww api check`, `diff` and `save` run on `wire_weaver_evolution`, the same checker an API crate runs from its own test.
- `ww` reaches a device through a debug probe over RTT (TOOL-20, probe-rs through `wire_weaver_client`'s `rtt` feature, now enabled): `--rtt CHIP` (`WW_RTT`, `rtt` in `ww.toml`) connects every command (`info`, `counters`, `sync`, `pins`, `dfu reboot`, `settings`, `observe`, `report`, ...) through it instead of USB; `--probe SERIAL|VID:PID|VID:PID:SERIAL` picks one of several probes, `--elf ELF` takes the control block address from the firmware instead of scanning RAM, `--probe-speed-khz` sets the SWD clock. USB filters next to `--rtt` from the same or a stronger source are refused (they'd pick another device), from `ww.toml` under a `--rtt` flag they are left out and named; `--api` is checked on the device once connected; `--probe` or `--probe-speed-khz` without `--rtt` are an error. An `elf` path in `ww.toml` is relative to the file. `ww list --probes` (also `ww list` with `--rtt`) lists the probes and who holds each; `ww sync` syncs the device behind the probe (through the hub when one runs, key `rtt:<probe serial>:<chip>`), `ww dfu` takes it whole in the hub and reconnects through the probe after the reset. Shell completion offers probe-rs chip names from what is typed and the serials of connected probes. `docs/cli.md#through-a-debug-probe-rtt`.
- `ww dfu` holds every firmware update tool: its steps on a running device (`reboot`, `flash`, `status`, `confirm`, `revert`) and the chip boot ROMs as `ww dfu rp` (was `ww bootsel`) and `ww dfu stm32` (was `ww stm32dfu`), no longer at the root. The old names are gone (`unrecognized subcommand`); the firmware repos' recipes use the new ones. `DfuCommand` is now the group (`Device(DeviceDfu)`, `Rp`, `Stm32`).
- `ww sync` on a terminal is one live view from the start, with nothing printed above it: who steers the devices, the table, a status line (`2 of 3 within 2.00 ms, waiting for Donguru A1B2C3`, then `✓ all 3 within 2.00 ms for 4.1 s`), the phase spread and `q or Ctrl+C to stop`; the last view stays on screen after it stops. Into a pipe it prints lines as before.
- `ww sync`: `q` stops it like Ctrl+C (`util::QuitKey`: keys read without echo, the terminal restored after; termios through `rustix` on Unix, `crossterm` on Windows). It says who steers the devices: at the start when no ww hub runs (ww sync steers, nothing does after it), and at the end whether the hub keeps steering them or they go on in holdover. The LED wording is gone: the phase spread line reads `Phase: 704 µs apart across 3 devices (±700 µs)` without the share of a 3 s LED breath, and the help no longer says Klipper-style.
- Denser help: each subcommand shows only the global flags it uses: device selection and the link tap (`--monitor`, `--capture`) where it connects, device selection only for `list`, `sync`, `config`, `explain`, `monitor`, `stats`, `capture`, `bootsel`, `stm32dfu`, none for `api`, `elf`, `settings`, `hub`, `udev` (still accepted there, as before). A subcommand's `--help` sums them up in one line per group with a pointer to `-h` and `ww --help`, which keep their help: `ww info --help` is 25 lines instead of 95, `ww elf --help` 12 instead of 89.
- `just install` builds `ww` with the new `[profile.deploy]` and installs it through `tpm install` when `tpm` is on PATH (kept-releases stack, exactly one `ww`), else with `cargo install --locked --path wire_weaver_cli`; `just deploy` pulls main with `--ff-only`, refuses on local changes, runs the new `just deps-pull` (`../chunky_mcu_updater`), installs and checks `ww --version` against main's commit. `just install-cli` is now an alias of `install`; it no longer copies with `install -D`.
- `ww api save` writes the SHA-256 of the snapshot's shrink_wrap bytes into the RON header (`// shrink_wrap sha256: ..`), which the build of `wire_weaver_snapshots` checks; a snapshot without it is saved again.
- Every `ww` command holding a device directly steers its clock onto UTC while it runs (CLT-46, `WW_CLOCK_SYNC=off`
  to leave it alone); `ww dfu reboot | flash | confirm | revert` pause it while they change the device, `ww sync`
  steers with its own sync as before. `ww` no longer depends on `ww_clock_sync` (it is in `wire_weaver_client`).
- `ww hub lend DEVICE -- CMD` (SRV-32): the hub closes the device, `CMD` runs with the terminal (Ctrl+C reaches it), the device is given back after; `ww` exits with `CMD`'s code. Without a hub `CMD` just runs.
- `ww dfu reboot | flash | confirm | revert` take the device whole while a ww hub serves it to other programs too (SRV-32): their calls and writes are refused naming `ww dfu`, and `flash` keeps it across the reset into the trial. All of a `ww` run's connections share one `ClientMeta`.
- `ww sync` with a ww hub running shows the hub's clock steering (SRV-31) from its `sync().stats()` stream and opens each device through the hub, never steering itself; without a hub (or `WW_HUB=off`) it steers directly as before. New STEERED BY column (program, pid, host of the steerer the device follows) and a note naming a second steerer the device only notes. Direct steering starts from the per-device rate cache (`ww_clock_sync::RateCache`).
- `ww sync` shows how far each device is and when it got there (TOOL-16): per device the state (`acquiring` until
  it is within its lock tolerance, then `locked`), the residual against the host's UTC with ± half the check's
  round trip, the device's error bound, rate, the offset still to slew and the round trip; a line when a device
  converges (locked and its residual within `--within-us`, default 2000) or falls out, and `all N within X after
  N s` with the LED phase spread; the LED phase spread under the table. Steers ten times a second until a device is
  locked. On a terminal a live table, into a pipe only those lines (no colour) and the table at the end.
- `ww` reaches devices through the ww hub when it holds them or after repeated calls (CLT-39, `WW_HUB=off` to always open directly), starts the hub by itself, answers `ww --hub-version`, and `ww hub take DEVICE` (a key or a serial number) has the hub open a device and keep it.
- `ww hub status | stop | who | keep` (TOOL-17): the running hub's version, process, who started it, uptime, programs, devices and idle timers (exit code 1 when none runs); stop it; who holds each device (the hub, a ww program from its lock file, on Linux any other program from `/proc`; works without a hub); keep it up for a bench (`30m`, `2h`, `forever`, `off`). Completion gives the `keep` durations and, for `who`, device keys, each with a help text; colour is off without a TTY or with `NO_COLOR`. `lend` waits for SRV-32.
- `ww list` asks a running hub for the device list (the same columns plus `STATE`, so held devices show up); `--direct` enumerates USB anyway, as do `--all` and device selection flags. It no longer prints the client library's connection log lines.
- `ww hub serve` runs the per-user hub in the foreground (SRV-28), and `ww` can be started as the hub by a client (`WW_HUB_SERVE`, `ww_hub::main_if_hub`). The hub runs on a thread and runtime of its own (one thread, see `ww_hub::run_blocking`).

### 🐛 Fixes

- `ww hub stop` closes its own connection after asking the hub to stop: the hub waited up to 500 ms for it before letting go of `hub.lock` (0.5 s down to 0.1 s). Every `ww hub` command is about ten times quicker with the client fixes (TOOL-17).
- `ww hub status` and `ww list` show a device lent with `ww hub lend` as `lent by <program> (pid N): <purpose>`, not as free (SRV-32).
- Strings a device, or another program through the hub, hands over (USB product/manufacturer/serial, introspection
  docs and resource/type names, `UserStr` text, pin names, holder program/label/user/host) are escaped before
  printing (TOOL-19): a C0/C1 control character, DEL or a Unicode bidi override/isolate shows as a visible
  `\u{..}` escape instead of reaching the terminal as is, so a device can no longer move the cursor, start a colour
  or title-change escape sequence, or make a line read back out of order. New `wire_weaver_cli::untrusted` module
  (`Untrusted`, `escape`); `--json` output is unaffected.
- `ww api scaffold --alloc` prints handlers with owned types (`Vec`, `String`, `..Owned`) for return values, property values and errors, sink items and stream function results, as a `no_alloc = false` server calls them (API-21); it printed borrowed return types, which did not compile.
- `ww stm32dfu flash --wait` retries opening the bootloader for 3 s, as `--follow` does: right after it enumerates, udev may not have set its permissions yet ("opening device: permission denied", seen once in three updates of a B135 rev B) (CLT-34).
- `ww stm32dfu info` shows the ROM's version from its version byte (was `0x00 (V0.0)`, the first byte of "get commands"); `ww stm32dfu flash` notes "unplug and replug the board when you can" only for the ROMs that lock PWR_CR3 (V9.1, V9.2 on STM32H72x/73x, AN2606 Table 134; it said "power-cycle" for every V9.x, V9.3 too) and with `--follow` waits 120 s for the board then (CLT-34). Checked on a B135 rev B (H725, V9.3).
- Live `--monitor` / `ww monitor` never named paths (`Request Observe [2]`) while the same traffic from a `--capture` file did: the device's API reached the capture's describe blocks only with the next data block, after the messages that needed it, and was written in its resolved form (which a decoder could not always read). It is handed over at once, as the device sent it, and live and file print the same text (TOOL-14).
- SIGINT and SIGTERM (a `timeout`) end a command with `--capture`, and `ww monitor` / `ww capture` / `ww stats`, with the capture file synced and renamed, not left as `FILE.partial` (TOOL-14).
- `ww explain PATH/TO/FILE.wwtape` of a file that does not exist is an error naming it, not a connect to a device.

### ⚠️ Breaking

- The package is renamed from `ww` to `wire_weaver_cli` (the `ww` name on crates.io belongs to an unrelated crate), the
  binary is still `ww`. Install with `cargo install --git https://github.com/vhrdtech/wire_weaver wire_weaver_cli`.
- Versioned together with the other `wire_weaver_*` crates (0.5.0).

### 🚀 Features

- `ww api tree` shows `#[claim(..)]` (also when it comes from the trait), `#[shares(..)]` and `#[label(..)]` in front of an item (SRV-34).
- `ww sync [DEVICE...]` (TOOL-16): keeps several devices' clocks on the host's UTC with `ww_clock_sync` (each read a
  few times a second, fitted, steered once a second) and shows a live table per device: state, followed source,
  rate, round trip and the residual offset measured with `Clock::shared_now`. Devices by serial or label parts, or
  every device the selection matches; `--mode` writes the `ww_indication` mode on all, `--duration`, `--poll-ms`,
  `--steer-ms`. Shell completion offers serials and labels of connected devices.

- `ww dfu flash` updates a chunky device in one command (delta, trial, confirm), `ww dfu status`, `confirm`,
  `revert` (CLT-33); `ww dfu reboot --into updater` on an A/B app says it goes to the boot ROM.
- `ww stm32dfu read ADDR LEN [-o FILE]` (hexdump, or raw bytes into a file) and `ww stm32dfu erase ADDR | --sectors A..B` (CLT-34).
- `ww stm32dfu list | info | flash`: STM32 system bootloader in DFU mode over `ww_stm32_dfu`; `flash` takes an ELF or a binary (`--addr`), shows a progress bar, verifies by reading back, jumps to flash (`--leave-addr`, `--no-leave`) and with `--wait` / `--follow` follows a device that rebooted itself into the bootloader (CLT-34). Serial numbers of DFU devices complete for `--serial`. `info` shows the ROM bootloader version and id; `flash` notes after the jump that a V9.x bootloader needs a power cycle before the firmware can set up PWR.
- `--monitor=api`, `ww monitor` and `ww explain` show calls through a compact global trait id (API-20) by trait and resource name with their decoded values, as for `GlobalFull` paths; global trait calls of `ww info` and friends go out compact now (6 bytes for a `UserInfo` read).
- The tap shows what a call reads (TOOL-9, TOOL-14): `Observe` requests and their answers as property names with the modes asked and granted (`reading: on change; temperature: every 10 ms; budget 2000 B/s`), `PropertyUpdate` events as the properties they carry with typed values, batched updates included (`reading: MeterReading { vbus_in: 5231 mV, .. }, temperature: 33800 m°C`), calls through global traits as `ww_user_info::UserInfo.name` with their values (`ww info`, `ww report`, also when the command never introspects the device), values of calls with the units of their types.
- Global `--monitor[=frame|link|api]` (default `link`; `--monitor-bytes`, `--monitor-bits`) and `--capture FILE` on every command that opens a device connection (`info`, `observe`, `stream`, `introspect`, `dfu`, `counters`, `report`, `usb-loopback`) (TOOL-14): the command's own client is tapped from the link setup on, printed to stderr as `ww monitor` prints it (stdout stays the command's) and/or saved to a capture file `ww explain FILE` reads, e.g. `ww -l blinky --monitor=api info` shows its requests and the replies paired with them. Commands that don't connect, and `ww monitor` / `ww capture` / `ww stats`, refuse the flags. `ww monitor`, `ww capture` and `ww stats` are unchanged; `ww monitor --layer link|api` now prints a call's requests as they happen, in order with their replies (decoder fix in `wire_weaver_client`).

- `ww pins list` and `ww pins test` (TOOL-15): pin tests on any device serving `ww_pin_diag::PinDiag`, found through its introspection data (`--at` when it has several). `test POINTS` takes names, `P<n>` ids, ranges (`GPIO2..=GPIO7`) and globs (`'chain.*'`), checks pulls, shorts to GND, to the supply and between the points, prints a table with what wasn't checked or was refused, gives every point back at the end (also on Ctrl-C and errors) and exits 1 on a short. `--watch` points are only read, `--plan FILE` adds expectations, narrower allow lists and `same_net`, `--save FILE` writes what the board shows into the plan. Point names complete live from the device.
- `ww api scaffold --handler-model <M>` (SRV-21): `fn` or `async fn` per item as `handler_model` says, passed on to the printed `ww_codegen!` call.
- The link tap in the CLI (TOOL-14, on TR-14): `ww monitor [--layer frame|link|api] [--bytes] [--bits]` prints what crosses this command's own connection as it happens, from the link setup on: every frame with its pieces, every `ww_link` message with the frame bytes it came in, requests with their arguments and replies paired with their request by seq (explained by its method, no `--for` needed), link events. `ww stats [--every S]` prints framer and link statistics (fill, overhead, split messages, losses, reply times per method), `ww stats --file FILE` those of a capture. `ww capture -o FILE [--layer] [--duration S]` appends the tap's blocks to a file together with the device's API bundle; `ww explain FILE` prints a capture exactly as `ww monitor` printed it live, without the device. Ctrl+C ends them with the disconnect recorded. `ww capture` writes a new file as `FILE.partial` and renames it when done, refuses an existing one without `--append`, syncs by `--sync` (`1s` default, `250ms`, `blocks:N`, `none`) and stops for good at a failed write or sync; a capture is read up to its first damaged block, `--resync` reads on past damage.

- `ww api scaffold --observe-cells LEVEL` (API-50): a static `<Level>Cells` per level, its reader in
  `ObserveBatches::new` and `observe_cells = ".."` in the `ww_codegen!` call.

- `ww observe --samples MS` (API-39): every sample of the level's `+batched` properties, in batches at most `MS`
  old, one line per sample with its device time (the update's shared `sample_times`, API-49).

- `ww introspect` and `ww observe` show `ww_si::AnyQuantity` values (STD-37) with their units like typed quantities
  (`3300 mV`, `1013 mbar` for a unit the device sent).
- `ww introspect` (const values) and `ww observe` show `ww_si` quantities with their units: `3300 mV`,
  `0.00012 N·m/(rad/s)`, a unit marker `ww_si` doesn't know by its name (`1500 Rpm`); values are formatted by their
  types now (CLT-31).
- `ww api tree -t` shows generic definitions with their parameters (`struct Pair<A, B>`, `Window<const N: u64>`) and
  names them in fields; uses read `Pair<u8, u16>` (API-43). `ww api save` saves a snapshot again when only the ww_self
  format changed ("reformatted").

- `--link-setup-timeout-ms` (`WW_LINK_SETUP_TIMEOUT_MS`, `link_setup_timeout_ms` in ww.toml, CLT-30): how long to
  wait for the device to answer when connecting, instead of the transport's default.

- `ww observe <path> [--rate MS]` (TOOL-11): observes a trait level's `+observe` properties without generated
  code (`DynResource::observe`), printing each update's present values, the update rate and the link bytes/s it
  costs, until Ctrl+C. `<path>` is a dotted/indexed resource path (`motor`, `bank[0]`), empty for the API root;
  without `--rate` every property is observed on change, `--rate` asks for all of them periodically instead.
- `ww stream <path> [--batches]` (API-41): prints items of a plain, counted or reliable stream without generated
  code (`DynResource::stream` / `positioned_stream`), one per line, until Ctrl+C. `--batches` prints whole batches
  of a batched stream instead, each item with its own device time. A stream function with arguments isn't
  supported yet (no value-parsing convention in the CLI for method/stream arguments).
- `ww settings dump FILE --word-size --page-size [--base] [--format text|json]` (TOOL-12): decodes a
  `ww_settings::Store` flash dump, no device needed. Pages (header, sequence number, whether it's current), every
  item slot (valid, cut, garbage or erased, with offset, key, seq, length) and the current record of every key,
  cross-checked against a real `Store` over the same bytes (`ww_settings::dump`, which does the actual decoding).
- `ww usb-loopback` works again (TOOL-10), on the sans-IO event loop: progress bars per stage and a summary table
  (messages, per second, bytes per second, lost, corrupted); exits with an error on lost or corrupted messages or a
  failed test. Connects to any device without downloading its introspection, the link layer answers.
  `--packet-size` is the size of a link message (`max` by default: the largest both sides accept), with completion
  of `max` and a few sizes; a value that is not a number is an error instead of silently meaning `max`. The test's tunables are options under
  "Test tuning", defaults as before: `--echo-timeout-ms` (500), `--timeouts-in-a-row` (3), `--marker-every` (32),
  `--markers-in-flight` (2), `--rx-batch` (64), `--batches-in-flight` (2). Rates are always per second (no
  `/min` or `/d` for small ones); a stalled test hints at the tuning options instead of the cable.

- `ww info [--format json]` (CLT-25): firmware, board, ids, user label and last reset of any device, through the global
  `ww_firmware_info`, `ww_board_info`, `ww_uid` and `ww_user_info` traits, whatever the device's API and version; a
  trait the device doesn't implement is said so and the rest is shown.
- `ww report [-o FILE] [--elf PATH] [--force] [--format json]` (CLT-25): `ww info` plus the counters, the device's
  stored `defmt` log and the last reset's fault and stack addresses named with the ELF, decoded with the ELF found by
  the build id the device reports, in one file for a bug report. A part whose ELF isn't found says so.
- `ww elf add <elf|archive>` and `ww elf id` (CLT-25): the ELF directory `~/.cache/ww/elf`, filled from single ELFs
  or a release's `<firmware>-<version>-elf.tar.zst` (`cnt_core::elf`, shared with `cnt elf add`).
- `ww counters [read|watch|tui|reset|info]` (STD-25): the `cnt` counters of any device implementing
  `ww_counters::Counters`, decoded with the firmware's ELF found by the build id the device reports (`--elf` to
  give one, checked; `target/`, `WW_ELF_PATH`, `~/.cache/ww/elf` otherwise), a mismatch refused unless `--force`.
  The same list, watch, TUI and JSON views as `cnt` over a probe (`cnt_view`).
- `ww introspect` reads the `const` items (outside arrays of traits) and shows their values in the tree,
  `--no-values` skips that (API-36). `ww api scaffold --const-model "BOARD=fn"` prints the server's
  `<RootTrait>Consts` impl, a Rust const or a `fn` per item.
- `ww api tree` shows stream functions with their arguments and result and their delivery kind (API-31).
- `ww api tree` shows counted and reliable streams as `#[counted] stream` / `#[reliable] stream` (API-30).
- `ww api tree` and `ww introspect` show a method's `#[errors(..)]` (API-32), and with docs a `# Errors` section with
  each entry's doc line; `ww api check` reports changed lists as compatible.
- `ww api tree` and `ww introspect` also show the `#[errors(..)]` of stream functions and properties (API-32), with
  their `# Errors` sections.
- `ww api scaffold --omit i2c.pullups,reset` (API-16): no stubs for these optional items, `omit = [..]` in the
  generated `ww_codegen!`; stubs of the optional items it keeps name their omit path. `ww api tree` and
  `ww introspect` show `#[optional]` / `#[optional(group)]` and `(left out by this device)`.
- `ww api tree -t` and `ww introspect -t` mark `ww_error::CommonError` and the variants holding it (`Common(CommonError)
  common errors`, STD-16).
- `ww bootsel list | info | flash | reboot` (CLT-21): RP2040 / RP2350 boot ROMs in BOOTSEL mode over PICOBOOT
  (`ww_picoboot`), selected with `--serial` as other commands; `flash` writes a UF2 file or a raw binary (`--addr`),
  verifies it and reboots into it, with a progress bar; `--wait` waits for the boot ROM after the app rebooted into it
  and `--follow` for the app to come back. Shell completion of `--serial` lists BOOTSEL devices too.
- `ww dfu reboot [--into updater|app]`: reboots any device implementing `ww_dfu::BootControl`, whatever its API and
  version, through a global trait path (API-20, CLT-10).
- `ww --version` shows the git SHA (`-dirty` with uncommitted changes) and commit time.
- `ww api check` reports `MaxLen` bound changes: lowering or adding one is breaking, raising or removing one
  compatible (SW-19).
- `ww api tree` shows `#[credit]` sinks; `ww api scaffold` generates their `credit_<name>` handler (API-28).
- `ww api scaffold` generates handlers taking `_cx: &mut Context<..>`, `--medium <path>` sets the medium type.
- `ww api save <path>` saves all traits and types of a crate into `<path>/api_snapshots/<crate>_<version>.ron`, to be
  committed and kept unchanged. Re-running is a no-op, any change without a version bump is an error, doc-only
  changes included (they bump the compatible position), `--force` to overwrite.
- `ww api check <path>`, the evolution checker: compares a crate with its latest snapshot in `<path>/api_snapshots/`
  (or the one given with `--against`), lists breaking and compatible changes, and fails if the crate version is not
  bumped enough, doc-only changes included (compatible position).
- `ww api diff <path>` lists every change since the latest snapshot, doc comments included (as a line diff): added,
  removed and changed traits, resources, types, fields, variants and dependency versions, followed by the verdict of
  `ww api check`. Informational, never fails.
- `ww api check` and `ww api diff` also take a snapshot file as `<path>`, compared with the previous snapshot in the
  same directory.
- `ww api save` runs the same check before saving a snapshot of a new version, and refuses to save it if the version is
  not bumped enough since the previous snapshot (`--force` saves anyway).
- `ww introspect` prints the size of the full API and of the introspection data the device sent, which leaves out
  traits and types known from `ww_stdlib` snapshots.
- `ww introspect --raw-as-sent` prints the introspection data as the device sent it, before traits and types known
  from snapshots were put back.
- `ww introspect -t`/`--types` and `ww api tree -t`/`--types` also print all types the API refers to: structs with
  their fields, enums with their variants, size kind and repr.
- `ww api scaffold <path>` generates server side scaffold: a server struct, a stub returning `Unimplemented` for every
  method and property handler, stream and valid indices stubs, and the `ww_codegen!` invocation. Takes the same
  options as `ww_codegen!` (`--use-async`, `--method-model`, `--property-model`, `--alloc` for `no_alloc = false`,
  which is off by default here) plus `--server` for the struct name and `-o` to write to a new file. Replaces the
  unfinished `ww api server-methods`.

- `ww list`: lists connected USB devices with product, serial, API name and version, hash and user label, without
  opening them. Only devices matching the device selection are shown, `--all` includes devices without an API id,
  `--plain` prints one line per device.
- Global device selection flags, usable before or after any subcommand: `--serial` (substring), `--label`,
  `--product`, `--manufacturer`, `--api name[@req]`, `--vid-pid VID:PID`, `--usb-path BUS-PORTS` (as shown by
  `ww list`) and `--timeout-ms`. Each can also be set with an environment variable (`WW_SERIAL`, `WW_LABEL`, ...) or
  in the `[device]` table of a project `ww.toml` (current directory or its closest parent, `--config`/`WW_CONFIG` to
  point to another one, `--no-config` to ignore it). Precedence per setting: flag > env variable > ww.toml.
- `ww config show` prints the resolved device selection and where each setting comes from, `ww config save` writes
  the selection flags given on the command line into ww.toml (keeping comments), `ww config unset <keys>` removes
  them.
- `ww introspect` prints the device's resource tree (ids, methods with signatures, properties with access, streams,
  sinks, nested traits and arrays of resources, docs) followed by a summary: resource/trait/type counts, traits and
  types left out of the bundle, introspection data size in bytes, referenced crates and API hash. `--raw` prints the
  previous debug dump instead, `-d`/`--skip-docs` hides docs. It now fails with an error when the device provided no
  introspection data, instead of printing nothing. Only warnings and errors are logged, so connection progress
  messages no longer surround the output.
- `ww api tree <path>` works again, printing the same tree for an API crate from source (`--name` to pick the trait,
  `-d`/`--skip-docs`). The `--skip-reserved` flag is removed: reserved items are no longer part of the API model.

- Shell completions for bash, zsh, fish, elvish and PowerShell, generated by `ww` itself
  (`source <(COMPLETE=bash ww)`, see README): subcommands, flags, `ww config unset` keys and paths, plus values of
  `--serial`, `--label`, `--product`, `--manufacturer`, `--api`, `--vid-pid` and `--usb-path` taken from the
  connected devices. In bash, values containing spaces or other special characters are inserted single-quoted (or
  escaped for an already open quote), and values after `@`, `:` or `=` (`--api name@^0.1`, `--vid-pid c0de:cafe`,
  `--label=...`) complete as well.

- `ww bootsel flash` takes an ELF file straight from `cargo build`, detected by its magic bytes rather than its
  extension (CLT-21): `PT_LOAD` segments are written at their flash physical address, no `objcopy`/`elf2uf2` step
  needed; UF2 and raw binaries still work as before.

### 🐛 Fixes

- `ww api scaffold` names types by their module path (`my_api::units::Volt`), like generated code (API-46).
- `ww info` and `ww report` exit with 0: they no longer end with "Called a method that required event loop to be running" (a second disconnect, see `wire_weaver_client`).
- `introspect` and `usb-loopback` never connected ("No devices found to connect to"): no interface was selected and
  `--serial` was not passed to the client. The only connected WireWeaver device is now used when no filters are given.
