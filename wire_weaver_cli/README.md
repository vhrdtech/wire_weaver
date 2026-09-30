# ww — WireWeaver command line tool

Lists connected USB devices with their API id, runs introspection and USB loopback tests.

## Installation

Install from git with cargo (the binary ends up in `~/.cargo/bin`):

```sh
cargo install --git https://github.com/vhrdtech/wire_weaver ww
```

Or from a local checkout of this repository:

```sh
cargo install --path wire_weaver_cli
```

Re-run the same command with `--force` to update an existing installation.

### Shell completions

Completions are produced by `ww` itself, so they always match the installed version, and values of the device
selection flags (`--serial`, `--label`, `--api`, `--usb-path`, ...) are completed from the currently connected
devices. Register them once in the shell's startup file:

```sh
echo 'source <(COMPLETE=bash ww)' >> ~/.bashrc              # bash
echo 'source <(COMPLETE=zsh ww)' >> ~/.zshrc                # zsh
echo 'COMPLETE=fish ww | source' >> ~/.config/fish/config.fish  # fish
```

Elvish and PowerShell are supported as well (`COMPLETE=elvish`, `COMPLETE=powershell`).

### Quick testing during development

`just install-cli` builds the current checkout in release mode and copies the binary into `~/.local/bin/ww`,
without going through `cargo install`. Alternatively, run it without installing from anywhere in the repository
with `cargo ww <args>` (alias from `.cargo/config.toml`).

## Usage

```sh
ww --help
ww list
ww introspect --serial 4b33
```

## Device selection

Commands working with a device (`introspect`, `usb-loopback`) connect to the only device matching the selection,
and `ww list` shows the devices it matches. With no selection, the only connected device reporting a WireWeaver API
id is used; if several match, all of them are printed and nothing is opened.

| ww.toml key    | Flag                | Env variable      | Matches                                          |
|----------------|---------------------|-------------------|--------------------------------------------------|
| `serial`       | `-s, --serial`      | `WW_SERIAL`       | serial number containing the value               |
| `label`        | `-l, --label`       | `WW_LABEL`        | user label, whole                                |
| `product`      | `-p, --product`     | `WW_PRODUCT`      | product description containing the value         |
| `manufacturer` | `--manufacturer`    | `WW_MANUFACTURER` | manufacturer containing the value                |
| `api`          | `--api`             | `WW_API`          | implemented API: `name[@req]`, e.g. `blinky_api@^0.1` |
| `vid_pid`      | `--vid-pid`         | `WW_VID_PID`      | USB VID:PID in hex, e.g. `c0de:cafe`             |
| `usb_path`     | `--usb-path`        | `WW_USB_PATH`     | USB bus and port chain as shown by `ww list`, e.g. `003-1.2` |
| `timeout_ms`   | `--timeout-ms`      | `WW_TIMEOUT_MS`   | request timeout, not a filter                    |

All text matching ignores case. Settings of different kinds are combined, so `api` from ww.toml and `--serial` on the
command line narrow the selection together.

Each setting is taken from the first place that has it: **command line flag > environment variable > ww.toml**.
`ww.toml` is looked up in the current directory and then its parents, so one file at the project root covers the
whole project. `--config <path>` (or `WW_CONFIG`) uses another file, `--no-config` ignores it.

```toml
# ww.toml
[device]
api = "blinky_api@^0.1"
serial = "4B333720"
timeout_ms = 2000
```

```sh
ww config save --api blinky_api@^0.1 --serial 4B333720  # write flags into ww.toml (comments are kept)
ww config show                                         # resolved selection and where each setting comes from
ww config unset serial                                 # remove settings from ww.toml
```

Environment variables are never saved by `ww config save`, so a `WW_*` variable exported in a shell profile does
not end up in project files.
