# WireWeaver

![Crates.io Version](https://img.shields.io/crates/v/wire_weaver)

<p align="center">
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/vhrdtech/wire_weaver/master/docs/assets/wire_weaver_logo-dark.png">
  <img src="https://raw.githubusercontent.com/vhrdtech/wire_weaver/master/docs/assets/wire_weaver_logo.png" alt="WireWeaver logo" width="480"/>
</picture>
</p>

> WireWeaver is a wire format and API code generator for resource constrained systems.

This is a convenience crate that re-exports several other ones:

* `shrink_wrap` - wire format and derive macros
* `wire_weaver_derive` - API code generation macros
* `ww_version` - SemVer for no_std no alloc systems

By default, this crate has `std` feature enabled, when using on `no_std`, add `default-features = false`.

Please see the [main repo](https://github.com/vhrdtech/wire_weaver)
or [documentation](https://vhrdtech.github.io/wire_weaver) for more information.