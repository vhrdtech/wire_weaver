# ww_plugin

> Device arguments and the protocol an executable ww plugin (ww-NAME) needs, without the ww hub

What an executable plugin (`ww-NAME` on `PATH`) takes from the `ww` tool: device arguments (`DeviceArgs`,
resolved with `Selection::resolve` from flags, then `WW_*` environment variables, then `ww.toml`), connecting
with them (`plugin::connect`), and the plugin protocol helpers (`plugin::complete_if_requested`,
`plugin::selection_env`). Never depends on `ww_hub`, directly or through another crate, so a plugin built on it
doesn't link the hub (checked by `tests/layers_lint`); `wire_weaver_cli` re-exports all of it at its old paths.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for the step-by-step guide.
