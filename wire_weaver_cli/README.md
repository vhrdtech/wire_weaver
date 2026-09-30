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

### Quick testing during development

`just install-cli` builds the current checkout in release mode and copies the binary into `~/.local/bin/ww`,
without going through `cargo install`. Alternatively, run it without installing from anywhere in the repository
with `cargo ww <args>` (alias from `.cargo/config.toml`).

## Usage

```sh
ww --help
ww list
```
