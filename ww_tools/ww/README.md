# ww

The WireWeaver command line tool, as a binary: `wire_weaver_cli`'s generic commands, `ww_std_cli`'s commands of the
standard library APIs, and the commands of the repositories built on WireWeaver (`ww logring` from logring,
`ww chunky` from chunky_mcu_updater, by git), linked together. `ww_tools/` is its own workspace. The commands, their flags, the shell completion and the plugins are described
in `docs/cli.md` and `docs/cli_reference.md`.

```sh
cargo install --git https://github.com/vhrdtech/wire_weaver ww
```

From a checkout, `just install` builds `ww` with the `deploy` profile and installs it; `cargo ww <args>` runs it
without installing.
