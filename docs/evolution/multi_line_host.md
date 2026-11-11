---
level: advanced
---

# A host for several wire lines

A wire break starts a new [wire line](rules.md#the-wire-version) of an API: a `w2.0` device and a `w1.2` host refuse
each other in link setup. Firmware in the field doesn't update all at once, so a host application often has to talk
to devices of several lines for a while. Generated clients are static, one per line, so the host bundles one client
per line and picks one at connect time. This page shows how, on the example in `tests/thermo` (EVO-28, EVO-42): a
thermometer API whose line 2 broke line 1 in several ways and whose line 3 broke line 2 in others, and one host
driver, `Thermo`, that runs the same code against firmware of any of the three lines. Scripts and tools without
generated code (the dynamic client, Python) are [at the end](#scripts-and-tools-without-generated-code).

The shape in short:

1. One API crate holds every line still in use, one root per module (`wire_v1`, `wire_v2`, `wire_v3`), and what more
   than one line uses in a module of its own (`common`), see [several lines in one crate](rules.md#several-lines-in-one-crate).
2. The host has a module per line with its generated client and its conversions to the driver's own types; the
   types in `common` are converted once, for every line.
3. A facade, an enum over the line clients made by `wire_weaver_client::facade!`, hides which line the device is on.
4. The connection speaks every line the driver knows; link setup takes the device on whichever it is, and the
   facade picks the matching client from the line the device reported. A reconnect (after `disconnect().keep_streams()`
   or a lost link) picks the client again, so a DFU to another line is not the end of the session (CLT-63).

Adding a line is one more root in the API crate (`ww api line new`, TOOL-33), one more module in the host and one
more variant of the facade enum. When the new line holds a value more finely or in a wider type than the driver's
types do (line 3's `uptime` went from whole seconds in a `u32` to milliseconds in a `u64`), the driver's type
follows the new line and the older lines' modules convert into it, exactly.

## Every line in one API crate

```text
thermo_api/
├── Cargo.toml        # [package.metadata.wire_weaver.roots]: "wire_v1::Thermo" = "w1.2", ..
├── api_snapshots/    # each line's history, then one snapshot per root
└── src/
    ├── lib.rs        # pub mod common; pub mod wire_v1; pub mod wire_v2; pub mod wire_v3;
    ├── common.rs     # RateError, Command (every line); Sample, Config, Mode, Channel, .. (lines 2 and 3)
    ├── wire_v1.rs    # #[ww_api_root] pub trait Thermo, its own Sample, Mode and Channel
    ├── wire_v2.rs    # #[ww_api_root] pub trait Thermo, everything else from common
    └── wire_v3.rs    # #[ww_api_root] pub trait Thermo, Batch and Health
```

A line's module takes what it shares from `common` with a `use`, as any Rust module does:

```rust
// wire_v2.rs
use super::common::{Channel, Command, Config, Mode, RateError, Sample, SelfTestError};

#[ww_api_root]
pub trait Thermo {
    fn configure(config: Config) -> Result<(), RateError>;
    fn read_once() -> Sample;
    ww_impl!(channel[]: Channel);
    // ..
}
```

- A type goes into `common` once more than one line uses it unchanged, not only when every line does: when line 2
  is retired (`ww api line retire`), line 3 must not lose what it took from line 2's module.
- The crate version is Rust-only: one crate at 4.0.0 serves firmware of w1.2, w2.1 and w3.0. Each root is checked
  against its own line's latest snapshot (`ww api check`, a `tests/evolution.rs` with `Check::run`), and a change to
  a `common` type is named on every line it reaches.
- An API whose lines were crates of their own before (here 1.x, 2.x and 3.x, each with its root at the crate root)
  moves into this layout without touching the wire: keep the old snapshots in `api_snapshots/`, and each root is
  compared with the last snapshot of its line. Firmware built from the old crates keeps working with a host built
  from the new one; the bench tests that with the published 1.0.0 and 2.0.0 (`tests/thermo_api_old`). The move
  changes every Rust path (`thermo_api::Sample` is `thermo_api::wire_v1::Sample`), so it is a breaking crate bump,
  which the checker doesn't ask for yet (EVO-54).

The host takes the crate once and names each root by its module path:

```rust
// v2.rs
wire_weaver_client::facade_line!(
    thermo_api::wire_v2::Thermo,
    sub_levels = [ww_indication::Indication => crate::IndicationClient],
);
```

`facade_line!` emits the line's `pub struct Client` and its `ww_codegen!(.. for Client, client = "std_client")`;
more `ww_codegen!` options go after a comma (`sub_levels`, see [shared sub-traits](#shared-sub-traits)). The generated
module of a root is named after its path, `api_client::thermo_api_wire_v2_thermo`. `ww api scaffold thermo_api
--root wire_v2::Thermo` prints the firmware side.

Before EVO-37 the lines had to be versions of one crate under renamed dependencies (`thermo_api_w1 = { package =
"thermo_api", .. }`), kept out of a workspace that can't hold two packages of one name and so out of `cargo clippy
--workspace`. That still works, for an API published that way, but every type is then a distinct Rust type per line.

## The driver's own types

The application should not see three `Sample` types. The driver has its own types (`model.rs`), shaped like the
newest line, and each line converts to and from them.

- Types from shared crates are the same Rust type in every line, as long as the lines take one version of that
  crate. All of `ww_si` (`Fixed`, `Point`, `Seq`, `IndexFrom1`) is in this group, so most fields are copied as they
  are:

    ```rust
    pub type Temp = Point<Celsius, Fixed<Kelvin, Milli, i32>>;
    pub type Rate = Fixed<Hertz, One, u16, Range<1, 1000>>;
    ```

- The API crate's `common` types are one Rust type for every line that uses them, so one conversion serves all of
  them (`tests/thermo/src/common.rs`); a `From<DeviceError<RateError>>` can only exist once anyway. Only what a line
  defines itself (line 1's `Sample` and `Mode`) is converted in that line's module.
- An evolvable enum read from the device has an `Unknown(u32)` variant for variants newer than the client. The
  conversion maps it to an error of the driver (`Error::UnknownVariant`).
- What an older line has and the newest one dropped stays in the model and is marked as such: `Mode::Triggered` is
  line 1 only, and lines 2 and 3 answer it with `Error::Unsupported`.

A change of scale or storage is a conversion through `exact_units`. `to()` returns `None` when the value has no exact
form, so a value line 1 can't take is refused before anything is sent, never rounded:

```rust
// v1.rs: line 1 takes whole gain steps, the model hundredths
async fn set_gain(c: &Client, g: Gain) -> Result<()> {
    let whole = g
        .to::<ww_si::prefix::One, u8>()
        .and_then(|g| g.to_bounds().ok())
        .ok_or_else(|| not_representable(c, "gain", g))?;
    Ok(c.write_gain(whole).write().await?)
}
```

## The facade

The driver's type is an enum over the line clients, `match`ed per operation. Everything is enum dispatch: each arm
is a statically known call, with no trait object, no boxed future and nothing hidden to step through in a debugger.
`wire_weaver_client::facade!` (CLT-64) writes it from the operations, listed once with their docs. It is async
only: each line's client is the async `std_client`.

```rust
wire_weaver_client::facade! {
    /// A `thermo_api` device on any line this driver knows.
    pub enum Thermo {
        V3(v3::Client),
        V2(v2::Client),
        V1(v1::Client),
    }

    /// The operations of [Thermo], a trait per line, implemented by that line's client.
    pub mod line;

    error = Error;

    /// Sets rate and mode. Line 1 has no single call for it: the rate first, then the mode.
    async fn configure(&self, rate: Rate, mode: Mode);
    async fn read_once(&self) -> Sample;
    async fn observe_gain(&self) -> stream GainUpdates<Gain>;
    async fn samples(&self) -> stream Samples<Sample>;
    async fn commands(&self) -> sink Commands<Command>;
    // ...
    /// From w2.1, lines 2 and 3.
    #[unsupported(V1, needs = "line 2 from w2.1")]
    async fn self_test(&self);
}
```

It emits:

- the enum, the facade itself, with one method per operation returning `Result<T, Error>`;
- the module `line` with a trait per line, named after its variant (`line::V3`), one associated function per
  operation, which the line implements for its client;
- for each `stream` and `sink` operation an enum of its own over the lines' receivers or senders (`Samples { V3(..),
  V2(..), V1(..) }`), with `async fn recv()` or `async fn send(item)` that `match`es the same way;
- `cmd()`, `wire()`, `Thermo::config(..)` and `WwClient`, with every line's `speaks` and a `try_from_cmd` that picks
  the variant of the device's line (see [below](#connecting-once-on-either-line)).

Each line writes its functions with the client as `c`, with only that line's generated types in scope:

```rust
// v3.rs
impl crate::line::V3 for Client {
    type GainUpdates = ObserverOf<api_client::thermo_api_wire_v3_thermo::ThermoUpdate, Gain, Gain, Error>;
    type Samples = SplitStreamOf<thermo_api::wire_v3::BatchOwned, model::Sample, Error>;
    type Commands = SinkOf<thermo_api::common::Command, model::Command, Error>;

    async fn read_once(c: &Client) -> Result<model::Sample> {
        sample(c.read_once().call().await?)
    }

    async fn samples(c: &Client) -> Result<Self::Samples> {
        Ok(SplitStreamOf::new(c.samples().call().await?, batch))
    }
    // ...
}
```

The trait's functions take no `self`, so `c.read_once()` is always the generated method, even where it has the
operation's name.

- Every line writes every operation, and the compiler checks it: a line that forgets one doesn't build
  (`not all trait items implemented, missing: samples`).
- An operation a line can't have is named for that line in `#[unsupported(V1, V2, needs = "..")]`, after or before
  its docs. Only the lines named there get a default, which answers `wire_weaver_client::facade::Unsupported`: the
  operation's name, the device's wire version and `needs` (optional, else "another line"). The driver's error takes
  it with `From`; line 1 writes no `self_test`. For a stream or a sink such a line still names its receiver,
  `type Samples = Lacking;`.
- An emulation is written by hand, and its doc comment says what differs. Line 1 has no `configure`, so its
  function makes two calls, rate first so a refused rate leaves the mode alone. That is not atomic: if atomicity
  matters, the API needs an atomic method.
- A stream, an observed property or a sink is a different type on each line, so each line names its receiver or
  sender as an associated type: a line's `Stream<A>`, `Observer<U, A>` or `Sink<A>` with its conversion, a plain
  `fn`, in one of `wire_weaver_client::facade`'s adapters (`StreamOf`, `SplitStreamOf` for a batch given item by
  item, `ObserverOf`, `SinkOf`), or a type of its own implementing `Receives` or `Sends`.
- Errors of each line fold into the driver's one `Error`. A call to an item newer than the device's revision is
  refused by the generated client before it is sent (`Error::OlderProtocol`, see [`#[since]`](rules.md#since)); the
  driver turns that into `Error::Unsupported`, which names the operation.
- A variant holds the line's whole client: `match` on the facade reaches what only one line has.
- A mistake in the operations (`&mut self`, a generic operation, a plain `fn`, a line in `#[unsupported]` the enum
  doesn't have) is a compile error naming the operation and what is wrong.

## Connecting once, on either line

Link setup refuses a device on any line but the client's own, so a facade must tell it every line it speaks.
`ClientConfig::speaks` adds a line from a generated client's `client_line()`. Link setup then takes a device on any
of them, sends that line's client version and gives the `Commander` that line's introspection data. The facade picks
its client in `WwClient::try_from_cmd` by the line the device reported (CLT-62). `facade!` writes both:

```rust
// what `facade!` emits for `Thermo`, in short
impl WwClient for Thermo {
    /// Every line this facade has a client of, in the order of its variants.
    fn default_config() -> ClientConfig {
        ClientConfig::default()
            .speaks(<v3::Client as LineClient>::line())
            .speaks(<v2::Client as LineClient>::line())
            .speaks(<v1::Client as LineClient>::line())
    }

    fn try_from_cmd(cmd: Commander) -> Result<Self, Error> {
        let wire = cmd.device_api_info().user_api_wire;
        if wire.map(|w| w.line) == Some(<v3::Client as LineClient>::line().wire.line) {
            return Ok(Self::V3(v3::Client::try_from_cmd(cmd)?));
        }
        // .. the same for V2 and V1
        Err(Error::NoClientForLine { .. })
    }
}
```

Link setup already refused any other line, so the `Err` is a bug in the facade's own set of clients, not something
to reach through the device: `try_from_cmd` says so rather than `unreachable!`, and the connect call that reaches it
fails cleanly.

A single-line client (one `impl WwClient` per generated client, never a facade) implements `try_from_cmd` too,
answering `Ok` unconditionally (`Ok(Self { cmd })`): `try_from_cmd` is the one method every `WwClient` provides,
`from_cmd`'s default panics on its `Err` and is sound to call only where that can't happen, as here.

A device on a line the driver doesn't know (one newer than all of them) is refused in link setup with
`Error::IncompatibleDeviceProtocol`, which names every line the host speaks and says to update the host. Two clients
of one line, or a line of another API, are refused when connecting.

## Reconnecting onto another line

A device can leave its line behind: a DFU flashes new firmware while the host program keeps running. After
`disconnect().keep_streams()` or a lost link, `Thermo::reconnect()` (CLT-63, `facade!`'s own method, built on
`Commander::reconnect_as::<Thermo>()`) picks the client of whichever line the device is on now, the same way
`try_from_cmd` did at the first connect — link setup refuses any line the connection doesn't speak, so this only
ever sees a line the facade already has a client of.

```rust
let t = Thermo::config(|c| c.usb().serial_eq(serial)).connect().await?;
// .. the link goes (a DFU), or t.cmd().disconnect().keep_streams().await? first
let t = t.reconnect().await?; // may now be a different variant than before
```

- On the **same line**, this resumes streams, sinks and observers exactly like the plain `Commander::reconnect`
  (CLT-36, CLT-40) and returns an equivalent `Thermo`.
- On **another line** this facade also speaks, old streams, sinks and observers end instead of resuming: their
  resource paths assumed the old line's API, so nothing of that handle still applies. Use what `reconnect()`
  returns from here on, not the value it was called on — its calls still assume the old line.
- A single-line client (no facade) never reaches a line change here: its connection speaks one line, so link setup
  itself refuses a device on any other one before a reconnect gets this far.
- Calling the plain `Commander::reconnect()` (no `T`, so it can't pick a client) on a connection whose device came
  back on another line fails instead, with the new `Error::ReconnectedOnAnotherLine`, naming both wire lines; the
  hub-supervised reconnect (CLT-40) gives up the same way, since it has no `T` either.

Test `tests/thermo` `lines::reconnect_after_a_dfu_to_another_line`.

## Shared sub-traits

A sub-trait every line uses unchanged, a `ww_stdlib` one (here `ww_indication::Indication`, the status LED) or one
of the API crate's own (`common::Channel`), would get a client of its own in each line's generated client: one Rust
type per line for the same trait, and the driver forwards every call per line. Generate its client once instead,
with `client = "sub_level"`, and let each line's client return it (CLT-68, `tests/shared_sub_level`; `Thermo`'s
`indication()` is one `match` returning it from any line):

```rust
mod indication_client {
    wire_weaver::ww_codegen!(
        ww_indication :: Indication for IndicationClient,
        client = "sub_level",
    );
}
pub use indication_client::IndicationClient;

mod api_v2 {
    wire_weaver::ww_codegen!(
        thermo_api :: wire_v2 :: Thermo for super::ClientV2,
        client = "std_client",
        sub_levels = [ww_indication::Indication => crate::IndicationClient],
    );
}
```

* `client = "sub_level"` generates the struct itself (`IndicationClient<'i>`, named by the plain identifier after
  `for`), with the trait's methods, its nested levels and its types, in a module beside it.
* Each `sub_levels` entry names a trait by its crate and module path and the shared client for it. Wherever that
  line's tree has the trait, flat or as an array, at any depth, the accessor (`indication()`, `probe().channel()`,
  `aux(i)`) returns the shared client and no per-line client is generated for it. Code over the trait is written
  once: `async fn blink(i: IndicationClient<'_>)` takes it from any line.
* Under a resource array (`aux[i]`) the accessor returns the same `ChannelClient<'_>`: an element of an array is
  observed by its own path ([API-63](../api/observing.md#levels-under-resource-arrays)), so the observe methods are
  the same everywhere.
* The calls go by path, as every other call of the line: nothing changes on the device, and global trait paths are
  not needed. A `trait_client` can't take `sub_levels`, its paths start at its own trait.
* A `const` assert in each accessor checks that the line has the trait the shared client was generated from: the
  trait's signature, with docs and `#[since]` left out. It fails to compile when the API crate takes another version
  of the trait than the driver does (two versions of `ww_indication`), and the message says to generate a client per
  line for it, which is then right: the trait really differs.
* `#[since]` still works per line. Loaded on its own, a shared trait has no line, so its `#[since = "w1.1, w2.1"]`
  means nothing to the shared client; its calls go through the line client's `Commander`, which checks each call's
  path against that line's own API, `#[since]` included. A `w1.0` device refuses `reset` with
  `Error::OlderProtocol` naming `w1.1`, a `w2.0` device naming `w2.1`.

A `client = "trait_client"` of the trait reaches it by its global path instead, wherever the device has it. That
needs the device to resolve global paths, which costs it memory and some CPU and is the device's choice, so a
driver of several lines reaches its sub-traits by path, through the shared client.

## Scripts and tools without generated code

A script, `ww` or a test bench usually has no generated client at all: the dynamic client
(`wire_weaver_client::DynClient`) and Python read the API from the device's own introspection data, so one script
talks to every line with no client per line. `tests/thermo/tests/dynamic.rs` and
`wire_weaver_py/tests/test_thermo_lines.py` run the bench's scenario this way.

- **Values in another scale or storage need no code.** A `Fixed` is read as a quantity with its unit and scale
  (`DynQuantity`, `ww.Quantity`), so 21.5 °C reads the same from 0.01 K in an `i16` and 0.001 K in an `i32`. A
  quantity written into a line's type converts exactly or is refused before anything is sent: `ww.Quantity("1.5",
  "Ratio")` into a gain of whole steps, or 0.001 K into 0.01 K steps, raises `ValueError` naming the type. Write
  quantities (`ww.Quantity("0.25 K")`, `DynQuantity::to_value`), not stored numbers, which differ per line.
- **Look items up by what the device has, not by its line number.** A moved item is found at its old or new path
  (`uptime` or `health.uptime`), a replaced method by its name (`configure` or `set_rate`), a changed stream item by
  its shape (a `Batch` has `temps`, a `Sample` has `temp`), an enum variant in the type's variants. `dev.wire` (Python)
  and `DeviceApiInfo::user_api_wire` give the line when a choice really depends on it.
- A `Point` (`Point<Celsius, Fixed<Kelvin, ..>>`) is read through its one field, `coord`, a quantity from the
  origin: the dynamic client and Python don't read a point as such yet (CLT-65).

## Testing every line

`tests/thermo/tests/lines.rs` runs one scenario against simulated firmware of each line through the same `Thermo`
and checks that every operation behaves the same, or fails with an error naming what the device's line lacks. The
simulated firmware is a crate of its own, `tests/thermo_devices`, built from the API crate's roots, so the dynamic
client's test and Python's (`thermo_pytest_devices` hosts every line behind a ww hub) use the same devices.

- Simulate the first revision of each line too (here w1.0 and w2.0), to see `#[since]` items refused. A server is
  always built at its root's revision, so such firmware comes from the API crate as it was then: the bench keeps
  the published 1.0.0 and 2.0.0 in `tests/thermo_api_old`, outside the workspace (a workspace can't hold two
  packages of one name), as old firmware still in the field. A device built from the current crate claiming an
  older wire is refused (CLT-61). Building a server at an earlier revision of its root is SRV-58.
- Old crates of the same name make `-p thermo_api` ambiguous for Cargo: name the current one with its version,
  `-p thermo_api@4.0.0`.
- An observed `get_set` property reports a host's write by itself: the generated code marks it changed after the
  setter returns `SetResult::Set` (SRV-57). Changes the firmware makes on its own still need `mark_changed_<name>`.
- A device of a line the driver has no client of checks the refusal and its message.
- The checker test compares the roots with each other (line 2 against line 1, line 3 against line 2: each a new
  line) and each line's root with the snapshot of its first revision (a new revision). The crate version is the API
  crate's own `tests/evolution.rs`, per root.
- Build and check an old API crate kept outside the workspace through the host's workspace, never on its own: a
  `cargo check` inside it writes its own `Cargo.lock`, which may lock `exact_units` at another commit. The checker
  then loads `ww_si` types from two revisions and reports a shared trait (`ww_indication`) as changed.

## Cost

Measured on the bench, three lines, on one PC in one session (10 Nov 2026):

- **Build time and size are no reason to drop a line.** An extra line adds about 650 to 720 lines of generated
  client code and about 125 KiB to a stripped release binary of about 10 MB that already carries tokio and the client
  runtime. A rebuild of the driver crate takes about 1.1 s (dev) and 1.6 s (release) with all three lines. The API
  crate itself builds in no time, as `#[ww_api_root]` and `#[ww_trait]` expand to nothing.
- **Shared sub-traits cost one client.** With `ww_indication` through one shared client instead of one per line,
  the generated code is 15% shorter (2,193 lines instead of 2,575) and the binary 113 KiB smaller.
- **The hand-written part is what grows.** The driver is 699 lines for three lines: 245 shared by every line
  (facade, errors, model), 76 converting the `common` types once, and 105 to 155 per line, most of it the bodies of
  the operations. A line that shares more with the others (lines 2 and 3) is cheaper; one with its own types and
  emulations (line 1) is the most. With five crates and no `common` it was 764 lines, and 1,074 with a hand-written
  boxed facade before `facade!`.
