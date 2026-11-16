# ww

The WireWeaver command line tool, as a binary: `wire_weaver_cli`'s generic commands and `ww_std_cli`'s commands of the
standard library APIs, linked together. The commands, their flags, the shell completion and the plugins are described
in `docs/cli.md` and `docs/cli_reference.md`.

```sh
cargo install --git https://github.com/vhrdtech/wire_weaver ww
```

From a checkout, `just install` builds `ww` with the `deploy` profile and installs it; `cargo ww <args>` runs it
without installing.
