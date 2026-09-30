# Command line tool

`ww` is the WireWeaver command line tool. It finds connected devices and lists them with their API, prints a
device's API as a resource tree using its introspection data (or an API crate's tree, from source), and runs USB
loopback and speed tests.

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
    Listing devices works without any setup, but opening one (`introspect`, `usb-loopback`) needs read/write access
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

## Commands

| Command                            | Needs a device | What it does                                                       |
|------------------------------------|----------------|--------------------------------------------------------------------|
| [`ww list`](#ww-list)              | no (not opened)| Lists connected devices matching the device selection              |
| [`ww introspect`](#ww-introspect)  | yes            | Prints the device's resource tree from its introspection data      |
| [`ww api`](#ww-api)                | no             | Prints the resource tree or AST of an API crate, from source       |
| [`ww usb-loopback`](#ww-usb-loopback) | yes         | Runs USB loopback and throughput tests                             |
| [`ww config`](#ww-config)          | no             | Shows or saves the device selection in a project `ww.toml`         |

`ww help <command>` or `ww <command> --help` prints all options of a command.

### `ww list`

Lists devices reporting a WireWeaver API id, without opening them: the information comes from USB string
descriptors (see [USB](transport/usb.md)), so it works even while another application is connected to the device.

```
$ ww list
LOCATION             PRODUCT             SERIAL                    API                 HASH              LABEL
usb 003-1 c0de:cafe  WireWeaver Generic  09000F000251313438333634  all_gpio_api@0.1.0  5ff989a95162d4d7
```

- `LOCATION`: transport, bus and port chain, VID:PID. The `003-1` part is what `--usb-path` expects.
- `API`: name and version of the API crate the firmware implements, `HASH`: its signature.
- `LABEL`: user label set by the firmware, if any.

Only devices matching the [device selection](#device-selection) are shown, so `ww list` is also the way to check
what a set of filters (or a `ww.toml`) selects. When some of the filters come from `ww.toml`, a note saying so is
printed to stderr.

| Option        | Description                                                               |
|---------------|---------------------------------------------------------------------------|
| `-a`, `--all` | Also list USB devices not reporting a WireWeaver API id                   |
| `--plain`     | One device per line with `key=value` fields, without table formatting     |

```
$ ww list --plain
usb 003-1 c0de:cafe "Vhrd.Tech" "WireWeaver Generic" serial=09000F000251313438333634 api=all_gpio_api@0.1.0 hash=5ff989a95162d4d7
```

### `ww introspect`

Connects to the selected device, downloads its introspection data (see [ww_self](std_library/ww_self.md)) and
prints the API as a resource tree: resource ids, methods with their signatures, properties with their access mode,
streams and sinks, nested traits, arrays of resources and doc comments. A summary follows: resource, trait and type
counts, size of the full API bundle and of the introspection data the device sent, referenced crates and the API
hash. Traits and types a device leaves out of its introspection data because they are known from
[snapshots](#ww-api) are put back first, any that could not be put back are counted as not included.

The tree looks the same as the one printed by [`ww api tree`](#ww-api) from source. Here with docs hidden:

```
$ ww introspect -d
trait AllGpioApi all_gpio_api@0.1.0
└─ 0 impl port[]: ww_gpio::Bank
   ├─ 0 impl pin[]: ww_gpio::Pin
   │  ├─ 0 fn set_output_level(level: Level)
   │  ├─ 1 fn toggle()
   │  ├─ 2 fn output_level() -> Level
   │  ├─ 3 fn input_level() -> Level
   │  ├─ 7 stream event: IoPinEvent
   │  ├─ 8 fn set_mode(mode: Mode, initial: Option<Level>) -> Result<(), Error>
   │  ├─ 9 fn mode() -> Mode
   │  ├─ 10 rw property pull: Pull, write error: Error
   │  ├─ 11 rw property speed: Speed, write error: Error
   │  └─ 12 fn configure_events(enabled: IoPinEnabledEvents) -> Result<(), Error>
   ├─ 8 fn capabilities() -> BankCapabilities
   ├─ 9 rw property reference_voltage: Volt, write error: Error
   └─ 10 fn name() -> String

15 resources, 2 traits, 11 types, 4362 bytes full, 72 bytes sent
crates: all_gpio_api@0.1.0, ww_gpio@0.1.0, ww_si@0.1.0, ww_numeric@0.2.0
api hash: 6e87e4113afd0168
```

`impl name[]: crate::Trait` is a nested trait (`[]` marking an array of them), properties show their access
(`const`, `ro`, `rw`, `wo`) and whether they are observable, `stream` is device-to-host and `sink` host-to-device.

| Option              | Description                                                             |
|---------------------|-------------------------------------------------------------------------|
| `-d`, `--skip-docs` | Do not print doc comments                                               |
| `--raw`             | Print raw introspection data (Rust debug format) instead of the tree    |

Fails with an error if the device did not provide introspection data.

### `ww api`

Works on API crate sources instead of a device: the crate is parsed the same way `ww_codegen!` does it, so no
device and no build are needed.

`ww api tree <path>` prints the same resource tree as `ww introspect`, for the `#[ww_trait]` defined in the crate
at `<path>`:

```
$ ww api tree examples/blinky_api
trait BlinkyApi blinky_api@0.1.0
├─ 0 fn led_on()
└─ 1 fn led_off()
```

`ww api ast <path>` prints the parsed API bundle in [RON](https://github.com/ron-rs/ron) format, useful when
debugging codegen or introspection.

Both take `--name <Trait>` to pick the trait when the crate defines more than one, and `tree` also takes
`-d`/`--skip-docs`.

`ww api save <path>` saves every `#[ww_trait]`/`#[ww_api_root]` trait and every `#[derive_shrink_wrap]` type
defined in the crate's `src/lib.rs` (or re-exported from one of its modules with `pub use`) into `<path>/api_snapshots/<crate>_<major>_<minor>_<patch>.ron`, for example
`ww_stdlib/ww_gpio/api_snapshots/ww_gpio_0_1_0.ron`. Commit the snapshot and never change it afterward.
It is a regular API bundle in RON: the root is named after the crate and has one nested trait resource per trait,
definitions from this crate are included, and traits and types from other crates are only referenced by crate
version, name and signature. Save a snapshot of each of those crates too.

A signature is a hash of the full definition with docs, including everything it refers to, calculated as if nothing
was skipped (`ww_self::signature`). A definition restored from another crate's snapshot can then be checked to be
exactly the one that was left out, which catches a crate changed without bumping its version. The client does the same
when it compares its API with a device's: a skipped trait or type with the same crate version but a different
signature makes the resources using it incompatible.

Snapshots of `ww_global` and the `ww_stdlib` crates are embedded into the `wire_weaver_snapshots` crate, used by
codegen and by `wire_weaver_client` (and so by `ww`). A device leaves traits and types known from them out of its
introspection data, referring to them by crate version, name and signature, which makes it much smaller (e.g.
`examples/all_gpio_api` goes from 4363 to 72 bytes). A definition is only left out if it is exactly the one in the
snapshot, e.g. an unpublished crate version that changed after its snapshot was saved stays in-line. The API hash is
calculated over the data as sent. Generated clients embed their own API in the same form. A client puts the left out
definitions back from its own snapshots after download or loading from cache, checking signatures; one it doesn't know (e.g. from a newer `ww_stdlib` crate) stays skipped, and resources using it
are only checked by crate version and signature. In this repo, `just save-snapshots` saves the snapshots and copies
them into `wire_weaver_snapshots/api_snapshots/`; a `wire_weaver_core` test fails if any of them is out of date.

```
$ ww api save ww_stdlib/ww_uart
saved ww_stdlib/ww_uart/api_snapshots/ww_uart_0_1_0.ron, 12 resources, 1 traits, 11 types (0 traits and 2 types not included)
```

Running it again does nothing if nothing changed. Any change is an error, including doc comments only:
bump the crate version (see [evolution rules](evolution/rules.md#doc-comments), doc-only changes bump the compatible
position, so that tools can tell that improved docs are available) and save a new snapshot. `--force` overwrites anyway, only use it for a
version that was never published.

Current limitations, same as for codegen: traits are only found in `src/lib.rs`, a dependency reached through another
crate's re-export is only resolved for the ones `wire_weaver` re-exports (`ww_version`, `shrink_wrap`), crates with a
workspace-inherited version can't be loaded, and self-referential types (like the ones in `ww_self`) are not supported.

### `ww usb-loopback`

Checks the USB link to the selected device: runs a loopback test (packets sent to the device and back, verifying
nothing was lost or corrupted), then one-way transmit and receive speed tests, with a progress bar for each.

| Option                 | Default | Description                                                       |
|------------------------|---------|-------------------------------------------------------------------|
| `--duration-sec <N>`   | `10`    | How long to run each test                                         |
| `--packet-size <N>`    | `max`   | Size of each test packet, capped to the max USB packet size       |

Lost or corrupted packets usually point at the hardware rather than the software: a bad cable, a malfunctioning
hub, a powered hub used without power, or power problems on an externally powered device.

### `ww config`

Manages the [project config file](#project-config-wwtoml):

```sh
ww config save --api blinky_api@^0.1 --serial 4B333720  # write flags into ww.toml (comments are kept)
ww config show                                         # resolved selection and where each setting comes from
ww config unset serial                                 # remove settings from ww.toml
```

`ww config save` writes into the `--config` file if given, otherwise into the `ww.toml` found in the current
directory or its closest parent, otherwise creates `ww.toml` in the current directory. Only flags from the command
line are saved, never environment variables, so a `WW_*` variable exported in a shell profile does not end up in
project files.

## Device selection

Commands working with a device (`introspect`, `usb-loopback`) connect to the only device matching the selection,
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

- [USB](transport/usb.md): how devices report their API id and user label, and how the host finds them
- [ww_self](std_library/ww_self.md): introspection data format
