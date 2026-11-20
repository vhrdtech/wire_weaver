# skipped_root_api

Stands in for an API built on `ww_stdlib` traits (R7), made only of `skipped_dep_api`'s traits, so codegen
leaves every one of its definitions out against its own snapshot.

Used in `wire_weaver_core`'s snapshot and definition cache tests, in place of `uart_api`/`all_gpio_api`.
