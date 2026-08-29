---
icon: lucide/home
hide:
  - navigation
  - toc
---

# WireWeaver { style="text-align:center" }

Collection of crates for designing **no_std** APIs with rich type system, **no allocation** and dense **bit-level** wire format.
{ style="text-align:center" }

![WireWeaver Logo](assets/logo.png){ width="250" style="display:block;margin:0 auto" }

Plus a whole set of standard types and traits, all written in Rust.
{ style="text-align:center" }

<div style="text-align:center" markdown>
[Get started](user/quickstart.md){ .md-button .md-button--primary }
[Standard library](user/usage.md){ .md-button }
[Examples](user/usage.md){ .md-button }
</div>

---

## What it does

<div class="grid cards" markdown>

-   :material-usb: __USB Hub__

    ---

    Not enough ports on your machine? Donguru puts a small test rack worth of equipment on just one cable.

-   :material-usb: __Full control of Type-C plug__

    ---

    Turn power on and off, disconnect data lines, communicate over USB-PD protocol, enter alternate or debug-accessory modes and more. 

-   :material-connection: __UART / I2C / GPIO / ADC__

    ---

    General-purpose interfaces for prototyping, bring-up, debugging and testing. On the plug SBU lines and 2.54 header.

-   :material-power-plug: __Power switch__

    ---

    Enable or disable power to downstream device(s) on demand.

-   :material-gauge: __Power meter__

    ---

    Monitor voltage and current draw in real time.

-   :material-creation: __More to come__

    ---

    Hardware also supports SPI, FDCAN, PWM and a whole SWD interface. Which will be released with upcoming software updates.

</div>

## Take the tour

<div class="grid cards" markdown>

-   :lucide-book-open-check: __Documentation__

    ---

    Quickstart and everyday usage of the `donguru` (`dg`) CLI from an end-user
    point of view.

    [:octicons-arrow-right-24: Start here](user/quickstart.md)

-   :lucide-terminal: __CLI__

    ---

    Understand the `donguru` (`dg`) command-line interface: design, command tree and
    it's unixi archtitecture.

    [:octicons-arrow-right-24: Read the design](design/cli/index.md)

-   :material-language-python: __Scripting__

    ---

    Take full control of the hardware through Rust or Python.

    [:octicons-arrow-right-24: Learn more](user/scripting.md)

-   :lucide-wrench: __Development__

    ---

    Set up your environment and build everything yourself.

    [:octicons-arrow-right-24: Start building](development/index.md)

-   :lucide-drafting-compass: __Design__

    ---

    Design documents, the library evaluations etc. to understand our decisions and desings.

    [:octicons-arrow-right-24: Browse designs](design/index.md)

-   :lucide-info: __About__

    ---

    What Donguru is, its feature set, and where to go next.

    [:octicons-arrow-right-24: Learn more](about/index.md)

</div>

WireWeaver is an API code generator for microcontrollers, supporting user-defined types, methods, properties, streams,
and traits.
It handles unsized types like Vec<T> and String even in no_std environments without an allocator,
and ensures full backward and forward compatibility between devices across format versions.

# Recommended learning order

1. Get familiar with the SerDes functionality: [wire format](./serdes/shrink_wrap.md)
   and [derive macro](./serdes/derive.md).
2. See the full list of [supported types](./types.md).
3. Understand [API capabilities](./api/overview.md).
4. See it in action on real hardware or on virtual device [template](https://github.com/vhrdtech/wire_weaver_template).
5. Read the rest of the docs, in particular: [evolution rules](./evolution/rules.md), [versioning](./api/versioning.md)
   and [addressing](./api/addressing.md).
