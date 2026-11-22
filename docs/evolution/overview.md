---
level: basic
---

# Evolving an API

Firmware stays in the field for years. The host program that talks to it is updated whenever someone feels like it,
and a blob written to flash today is read by firmware built next year. So every version of your API will meet
versions that are older and newer than itself. This chapter is about changing an API so that they still understand
each other.

## A change that looks harmless

A blinking LED reports its status. Version 1 of the API has a blink period of up to 255 ms:

```rust
#[derive_shrink_wrap(borrowed)]
pub struct Status {
    pub on: bool,
    pub period: Millis<u8>,
}
```

Later, someone needs blinks of a whole second, so `period` becomes a `Millis<u16>`. Everything compiles. But a host
still built with version 1 now reads this from a new device:

| The `u16` version writes `period: 1000 ms` | Version 1 reads it as |
|--------------------------------------------|-----------------------|
| `80 e8 03`                                 | `period: 232 ms`      |

No error, just a wrong number. Version 1 reads one byte where two were written. Nothing on the wire says that the
field changed. (Going the other way, the `u16` version can't read version 1's two bytes at all and fails.)

This is what evolution is about. Two different things can break when an API crate changes:

* **The wire**: whether bytes written by one version can be read by the other. A break here reaches devices that are
  already deployed, and no `cargo update` fixes it.
* **The Rust code**: whether code still compiles against the new crate. A break here is annoying, but the compiler
  finds it before anything ships.

A change can break one without the other. Renaming a field breaks Rust code and not a single byte. Widening `u8` to
`u16` compiles fine and breaks the wire.

## Add, don't change

The safe way to evolve is to add things at the end, and to say what older data means for them. Here version 2 leaves
`period` alone and adds a brightness:

```rust
#[derive_shrink_wrap(borrowed)]
pub struct Status {
    pub on: bool,
    pub period: Millis<u8>,
    #[since = "w1.1"]
    #[default = None]
    pub brightness: Option<Percent<u8>>,
}
```

Both directions work, taken from a real run:

| Who writes                       | Bytes         | Who reads | What it gets                                 |
|----------------------------------|---------------|-----------|----------------------------------------------|
| version 1, `on`, 250 ms          | `80 fa`       | version 2 | `on`, 250 ms, `brightness: None` (the default) |
| version 2, `on`, 250 ms, 40 %    | `80 fa 80 28` | version 1 | `on`, 250 ms: it skips what it doesn't know  |

The same goes for the API itself. A new method goes at the end of its trait, marked with the wire version that adds
it:

```rust
#[ww_api_root]
pub trait BlinkyApi {
    #[effect(actuate)]
    fn led_on();
    #[effect(actuate)]
    fn led_off();
    #[since = "w1.1"]
    #[effect(actuate)]
    fn led_toggle();
}
```

A new host that calls `led_toggle` on an old device gets a clean error (`Error::OlderProtocol`) before anything is
sent, instead of a confused device.

Most things you will want to do are additions like these: a new method, property, stream, type, a field at the end
with a default, an enum variant at the end. Removing, reordering, renaming or changing the type of something that is
already out there is a break.

## Two versions

Every API crate carries two versions, because the two kinds of break need different answers:

```toml
[package]
name = "blinky_api"
version = "0.1.1"          # the crate version, for Rust code

[package.metadata.wire_weaver]
wire = "w1.1"              # the wire version, for devices and hosts
```

The **wire version** reads "line 1, revision 1". Devices and hosts on the same line always connect, whatever their
revision: the additions above bump the revision (`w1.0` -> `w1.1`). A change that breaks the wire starts a new line
(`w2.0`), and a device and a host on different lines refuse each other with a clear message instead of reading wrong
numbers. A new API starts on `w0.0`, the design line, where anything goes; its first release is `w1.0`.

The **crate version** is ordinary Cargo SemVer. An addition bumps the compatible position (patch before 1.0), a
change that breaks Rust code the breaking one (minor before 1.0).

## Let the checker decide

You don't have to remember which change needs which bump. The evolution checker compares your crate with a snapshot
of the last release and tells you. Add it to the API crate as a test, so `cargo test` and CI run it:

```toml
[dev-dependencies]
wire_weaver_evolution = "0.6"
```

```rust
// tests/evolution.rs
#[test]
fn api_evolution() {
    wire_weaver_evolution::Check::new(env!("CARGO_MANIFEST_DIR"))
        .require_current(true)
        .assert();
}
```

Then the routine for each version is:

1. Bump the version when you start on the next one.
2. Change the API.
3. Save its snapshot, `WW_API_SAVE=1 cargo test --test evolution` (or `ww api save my_api`), and commit
   `api_snapshots/` with the change.

When a change needs a bigger bump than you gave it, the test fails and says which versions to write:

```text
  breaking: BlinkyApi::led_off removed
      rule: traits, resources and types are never removed, clients and code using them break (docs/evolution/rules.md#api)
...
Bump `version` in examples/blinky_api/Cargo.toml to 0.2.0 or later
```

## Where to go from here

The rest of the chapter goes deeper, in the order you are likely to need it:

* [Versions and wire lines](versions.md): how a device and a host decide whether they can talk, `#[since]`, the
  design line, and how one crate holds several lines.
* [What can change](rules.md): every kind of change to types and APIs, and whether it is compatible.
* [The evolution checker](checker_tool.md): `ww api check`, `save` and `diff`, the test, hooks and CI.
* [Units compared by meaning](units_by_meaning.md): why `Millis` -> `Micros` is a break although the bytes stay.
* [A host for several wire lines](multi_line_host.md): one driver for devices on line 1, 2 and 3.
* [Changing WireWeaver itself](crate_layers.md): the layers below your API and how they are versioned.
* [Inside the checker](inside_the_checker.md): snapshots, signatures, padding bits and old snapshot formats.
