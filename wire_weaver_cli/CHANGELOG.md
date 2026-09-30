## Unreleased

### 🚀 Features

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

### 🐛 Fixes

- `introspect` and `usb-loopback` never connected ("No devices found to connect to"): no interface was selected and
  `--serial` was not passed to the client. The only connected WireWeaver device is now used when no filters are given.
