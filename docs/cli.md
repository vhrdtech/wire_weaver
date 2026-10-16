---
level: basic
---

# Command line tool

`ww` is the WireWeaver command line tool. It finds connected devices and lists them with their API, prints a
device's API as a resource tree using its introspection data (or an API crate's tree, from source), observes
properties, asks any device what it is and what it counted, and tests the link.

## Installation

Install from git with cargo (the binary ends up in `~/.cargo/bin`):

```sh
cargo install --git https://github.com/vhrdtech/wire_weaver wire_weaver_cli
```

Or from a local checkout of the repository:

```sh
cargo install --path wire_weaver_cli
```

Re-run the same command with `--force` to update an existing installation.

Inside the repository it can also be run without installing, with `cargo ww <args>` (alias from
`.cargo/config.toml`). `just install-cli` builds the current checkout in release mode and copies the binary into
`~/.local/bin/ww`, which is handy when testing changes to the tool itself.

!!! note "Linux permissions"
    Listing devices works without any setup, but opening one (`introspect`, `info`, ...) needs read/write access
    to `/dev/bus/usb/...`, usually granted to your user with a udev rule. See [USB](transport/usb.md).

### Shell completions

Completions are produced by `ww` itself, so they always match the installed version. Values of the device
selection flags (`--serial`, `--label`, `--api`, `--usb-path`, ...) are completed from the devices connected right
now, with the product and label of each shown as a hint. Register them once in the shell's startup file:

```sh
echo 'source <(COMPLETE=bash ww)' >> ~/.bashrc                  # bash
echo 'source <(COMPLETE=zsh ww)' >> ~/.zshrc                    # zsh
echo 'COMPLETE=fish ww | source' >> ~/.config/fish/config.fish  # fish
```

Elvish and PowerShell are supported as well (`COMPLETE=elvish`, `COMPLETE=powershell`). In bash, values containing
spaces or quotes are inserted quoted, and values after `@` or `:` complete too (`--api blinky_api@<TAB>`,
`--vid-pid c0de:<TAB>`).

## First steps

Plug in a device and look at what is there:

```
$ ww list
LOCATION             PRODUCT             SERIAL                    API                 HASH              LABEL
usb 003-1 c0de:cafe  WireWeaver Generic  09000F000251313438333634  all_gpio_api@0.1.0  5ff989a95162d4d7
```

`ww list` reads the USB descriptors only, without opening the device. `API` is the API crate the firmware
implements. With one device connected, every other command uses it; with several, pick one with a
[device selection](#device-selection) flag such as `--serial 0900`.

Next, the device's API as a resource tree, from the introspection data it carries (`-d` hides doc comments):

```
$ ww introspect -d
trait AllGpioApi all_gpio_api@0.1.0
└─ 0 impl port[]: ww_gpio::Bank
   ├─ 0 impl pin[]: ww_gpio::Pin
   │  ├─ 0 fn set_output_level(level: Level)
   │  ├─ 1 fn toggle()
   ...
```

From there:

- `ww info`: what the device is (firmware, board, ids, label, last reset), on any device implementing the
  [device contract](std_library/device_contract.md).
- `ww observe <path>`: watch a trait level's [observed properties](api/observing.md) change, no generated code
  needed. For calls and streams from a script, use [Python](python.md).
- `ww usb-loopback`: check the link itself, round trips and throughput, whatever the API.
- `ww api tree <path>` and `ww api scaffold <path>`: the same tree from an API crate's source, and the server
  handlers to implement for it.

## Commands

| Command                                              | Needs a device | What it does                                                       |
|------------------------------------------------------|----------------|--------------------------------------------------------------------|
| [`ww list`](cli_reference.md#ww-list)                | no (not opened)| Lists connected devices matching the device selection              |
| [`ww introspect`](cli_reference.md#ww-introspect)    | yes            | Prints the device's resource tree from its introspection data      |
| [`ww api`](cli_reference.md#ww-api)                  | no             | From an API crate's source: resource tree, AST, server scaffold, snapshots (save, check, diff) |
| [`ww observe`](cli_reference.md#ww-observe)          | yes            | Observes a trait level's `+observe` properties: values, rate, link bytes/s |
| [`ww usb-loopback`](cli_reference.md#ww-usb-loopback) | yes, any API  | Runs link loopback and throughput tests                            |
| [`ww dfu`](cli_reference.md#ww-dfu)                  | yes, any API   | Firmware update steps through `ww_dfu` traits: `ww dfu reboot`     |
| [`ww counters`](cli_reference.md#ww-counters)        | yes, any API   | The device's `cnt` counters, decoded with its ELF: read, watch, TUI, reset |
| [`ww info`](cli_reference.md#ww-info)                | yes, any API   | What the device is: firmware, board, ids, label, last reset        |
| [`ww report`](cli_reference.md#ww-report)            | yes, any API   | A bug report in one file: info, counters, stored log, crash, decoded with the ELF |
| [`ww elf`](cli_reference.md#ww-elf)                  | no             | The ELF directory: `ww elf add` a release archive, `ww elf id`     |
| [`ww settings`](cli_reference.md#ww-settings)        | no             | Decodes a `ww_settings::Store` flash dump                          |
| [`ww bootsel`](cli_reference.md#ww-bootsel)          | a boot ROM     | RP2040 / RP2350 in BOOTSEL mode: list, flash, reboot, follow back  |
| [`ww config`](cli_reference.md#ww-config)            | no             | Shows or saves the device selection in a project `ww.toml`         |

`ww help <command>` or `ww <command> --help` prints all options of a command; the
[command reference](cli_reference.md) explains each one with examples.

## Device selection

Commands working with a device (`introspect`, `observe`, `info`, ...) connect to the only device matching the selection,
and `ww list` shows all the devices it matches. With no selection, the only connected device reporting a WireWeaver
API id is used; if several match, they are all printed and nothing is opened, run `ww list` to see them and narrow
the selection.

| Flag             | Env variable      | `ww.toml` key  | Matches                                                        |
|------------------|-------------------|----------------|----------------------------------------------------------------|
| `-s, --serial`   | `WW_SERIAL`       | `serial`       | Serial number containing the value                             |
| `-l, --label`    | `WW_LABEL`        | `label`        | User label, whole                                              |
| `-p, --product`  | `WW_PRODUCT`      | `product`      | Product description containing the value                       |
| `--manufacturer` | `WW_MANUFACTURER` | `manufacturer` | Manufacturer containing the value                              |
| `--api`          | `WW_API`          | `api`          | Implemented API: `name[@req]`, e.g. `blinky_api@^0.1`          |
| `--vid-pid`      | `WW_VID_PID`      | `vid_pid`      | USB VID:PID in hex, e.g. `c0de:cafe`                           |
| `--usb-path`     | `WW_USB_PATH`     | `usb_path`     | USB bus and port chain as shown by `ww list`, e.g. `003-1.2`   |
| `--timeout-ms`   | `WW_TIMEOUT_MS`   | `timeout_ms`   | Request timeout in milliseconds, not a filter                  |

- All text matching ignores case.
- The version requirement in `--api` uses Cargo's [SemVer syntax](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#version-requirement-syntax);
  without it, any version matches.
- `--usb-path` pins a physical port, so it keeps selecting the same device across re-enumeration, and tells apart
  identical devices without serial numbers.
- The flags are global: they can be given before or after the subcommand (`ww --serial 2100 list` and
  `ww list --serial 2100` are the same).

Settings of different kinds are combined, so for example `api` from `ww.toml` and `--serial` on the command line
narrow the selection together. Each setting is taken from the first place that has it:
**command line flag > environment variable > `ww.toml`**.

### Project config: `ww.toml`

Device selection that stays the same for a project goes into the `[device]` table of a `ww.toml`, using the keys
from the table above:

```toml
# ww.toml
[device]
api = "blinky_api@^0.1"
serial = "4B333720"
timeout_ms = 2000
```

`ww.toml` is looked up in the current directory and then its parents, so one file at the project root covers the
whole project. `--config <path>` (or `WW_CONFIG`) uses another file, `--no-config` ignores it. Unknown keys are
reported as errors rather than ignored. `ww config show` prints the config file in use, then every setting with its
resolved value and where it came from (flag, env variable or file), so it answers "why is this device (not)
selected".

## See also

- [Command reference](cli_reference.md): every command and option
- [USB](transport/usb.md): how devices report their API id and user label, and how the host finds them
- [ww_self](std_library/ww_self.md): introspection data format
