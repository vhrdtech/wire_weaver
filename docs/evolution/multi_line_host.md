---
level: advanced
---

# A host for several wire lines

A wire break starts a new [wire line](rules.md#the-wire-version) of an API: a `w2.0` device and a `w1.2` host refuse
each other in link setup. Firmware in the field doesn't update all at once, so a host application often has to talk
to devices of both lines for a while. Generated clients are static, one per line, so the host bundles one client per
line and picks one at connect time. This page shows how, on the example in `tests/thermo` (EVO-28): a thermometer
API whose line 2 broke line 1 in several ways and whose line 3 broke line 2 in others, and one host driver,
`Thermo`, that runs the same code against firmware of any of the three lines. Scripts and tools without generated
code (the dynamic client, Python) are [at the end](#scripts-and-tools-without-generated-code).

Since EVO-37 one API crate can hold every line, one root per module, with the types the lines share in one module
([several lines in one crate](rules.md#several-lines-in-one-crate)): no renamed dependencies and no conversions of
unchanged types. This page still shows the earlier layout, two versions of the crate; its rework for one crate is
EVO-42.

The shape in short:

1. Both versions of the API crate are dependencies of the host, under two names.
2. Each gets a module with its generated client and the conversions to the driver's own types.
3. A facade trait, implemented once per line, hides which line the device is on.
4. The connection speaks every line the driver knows; link setup takes the device on whichever it is, and the
   facade picks the matching client from the line the device reported. One handshake, no reconnect.

Adding a line is one more module of the same kind and one more line in each of the two lists in step 4. When the new
line holds a value more finely or in a wider type than the driver's types do (line 3's `uptime` went from whole
seconds in a `u32` to milliseconds in a `u64`), the driver's type follows the new line and the older lines'
modules convert into it, exactly.

## Two versions of one API crate

Cargo takes two versions of one crate under different names:

```toml
[dependencies]
thermo_api_w1 = { package = "thermo_api", version = "1.2" }
thermo_api_w2 = { package = "thermo_api", version = "2.1" }
```

Each gets its own generated client, named by the dependency key:

```rust
// v1.rs
pub struct Client {
    cmd: Commander,
}

mod api_client {
    wire_weaver::ww_codegen!(
        thermo_api_w1 :: Thermo for super::Client,
        client = "std_client",
    );
}
```

and the same in `v2.rs` with `thermo_api_w2`. `ww api scaffold` prints the package name (`thermo_api ::`): write the
dependency key instead.

Inside one repository a workspace can't hold two packages of one name, so the bench keeps its API crates
(`tests/thermo_api/{w1_0,w1,w2_0,w2,w3}`) in the root `exclude` and adds them to `[workspace.dependencies]` by path,
each with its `package = "thermo_api"`. A host taking released versions from a registry needs none of that.

## The driver's own types

The application should not see two `Sample` types. The driver has its own types (`model.rs`), shaped like the
newest line, and each line converts to and from them.

- Types from shared crates are the same Rust type in every line, as long as both lines take one version of that
  crate. All of `ww_si` (`Fixed`, `Point`, `Seq`, `IndexFrom1`) is in this group, so most fields are copied as they
  are:

    ```rust
    pub type Temp = Point<Celsius, Fixed<Kelvin, Milli, i32>>;
    pub type Rate = Fixed<Hertz, One, u16, Range<1, 1000>>;
    ```

- The API crate's own structs and enums are a distinct Rust type per line, even when they did not change
  (`RateError`, `Command`): each needs a conversion per line.
- An evolvable enum read from the device has an `Unknown(u32)` variant for variants newer than the client. The
  conversion maps it to an error of the driver (`Error::UnknownVariant`).
- What an older line has and the newest one dropped stays in the model and is marked as such: `Mode::Triggered` is
  line 1 only, and line 2 answers it with `Error::Unsupported`.

A change of scale or storage is a conversion through `exact_units`. `to()` returns `None` when the value has no exact
form, so a value line 1 can't take is refused before anything is sent, never rounded:

```rust
// v1.rs: line 1 takes whole gain steps, the model hundredths
fn set_gain(&self, g: Gain) -> Fut<'_, ()> {
    Box::pin(async move {
        let whole = g
            .to::<ww_si::prefix::One, u8>()
            .and_then(|g| g.to_bounds().ok())
            .ok_or_else(|| self.not_representable("gain", g))?;
        Ok(self.write_gain(whole).write().await?)
    })
}
```

## The facade trait

Every operation of the driver is a method of one trait, implemented by each line's client. `async fn` is not allowed
in a trait object, so each returns a boxed future:

```rust
pub(crate) type Fut<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

pub(crate) trait Backend: Send + Sync {
    fn wire(&self) -> WireVersion;
    fn configure(&self, rate: Rate, mode: Mode) -> Fut<'_, ()>;
    fn read_once(&self) -> Fut<'_, Sample>;
    fn observe_gain(&self) -> Fut<'_, Box<dyn Updates<Gain>>>;
    fn samples(&self) -> Fut<'_, Box<dyn Updates<Sample>>>;
    // ...
}
```

A trait object rather than an `enum { V1, V2 }` keeps the per-line code in the line's module: no method of the facade
grows an arm per line.

- A method that a line lacks is emulated or refused. Line 1 has no `configure`, so its `Backend` makes two calls,
  rate first, so a refused rate leaves the mode alone. Such an emulation is not atomic: say so in its doc comment.
- Streams, sinks and observed properties need one generic adapter each, shared by all lines: a line's
  `Stream<A>`, `Sink<A>` or `Observer<U, A>` and a conversion `fn` (`StreamOf`, `SinkOf`, `ObserverOf` in
  `backend.rs`).
- Errors of each line fold into the driver's one `Error`. A call to an item newer than the device's revision is
  refused by the generated client before it is sent (`Error::OlderProtocol`, see [`#[since]`](rules.md#since)); the
  driver turns that into `Error::Unsupported`, which names the operation.
- Inside `impl Backend for Client` a generated method may have the trait method's name (`read_once`, `samples`).
  Inherent methods win, so `Client::read_once(self)` calls the generated one.

## Connecting once, on either line

Link setup refuses a device on any line but the client's own, so a facade must tell it every line it speaks.
`ClientConfig::speaks` adds a line from a generated client's `client_line()`. Link setup then takes a device on any
of them, sends that line's client version and gives the `Commander` that line's introspection data. The facade picks
its client in `WwClient::from_cmd` by the line the device reported:

```rust
impl WwClient for Thermo {
    /// Every line this driver has a client of, the newest first.
    fn default_config() -> ClientConfig {
        ClientConfig::default()
            .speaks(v2::Client::client_line())
            .speaks(v1::Client::client_line())
    }

    fn from_cmd(cmd: Commander) -> Self {
        let wire = cmd.device_api_info().user_api_wire;
        let backend: Box<dyn Backend> = match wire.map(|w| w.line.0) {
            Some(1) => Box::new(v1::Client::from_cmd(cmd)),
            Some(2) => Box::new(v2::Client::from_cmd(cmd)),
            // link setup refuses a device on any other line, see `default_config`
            _ => unreachable!("a thermo_api device at wire {wire:?} got through link setup"),
        };
        Thermo { backend }
    }
}
```

A device on a line the driver doesn't know (one newer than all of them) is refused in link setup with
`Error::IncompatibleDeviceProtocol`, which names every line the host speaks and says to update the host. Two clients
of one line, or a line of another API, are refused when connecting.

## Shared sub-traits

A sub-trait every line uses unchanged, a `ww_stdlib` one (here `ww_indication::Indication`, the status LED) or one
of the API crate's own (`common::Channel`), would get a client of its own in each line's generated client: one Rust
type per line for the same trait, and the driver forwards every call per line. Generate its client once instead,
with `client = "sub_level"`, and let each line's client return it (CLT-68, `tests/shared_sub_level`):

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
* Under a resource array (`aux[i]`), the accessor returns `ChannelClient<'_, false>`: a level under an array can't
  be observed yet (SRV-18), so, as on a client of its own, it has no `observe(cfg)` and its `+observe` properties'
  `observe_<name>()` is the plain stream. Elsewhere it is `ChannelClient<'_>` (`OBSERVE = true`). Code taking both
  is generic over it: `async fn read<const O: bool>(c: ChannelClient<'_, O>)`.
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
simulated firmware is a crate of its own, `tests/thermo_devices`, so the dynamic client's test and Python's
(`thermo_pytest_devices` hosts every line behind a ww hub) use the same devices.

- Simulate the first revision of each line too (here w1.0 and w2.0), to see `#[since]` items refused. Build it from
  its own version of the API crate: a device built from the newer crate has the client's API hash, and identical
  hashes skip the resource checks, `#[since]` included.
- An observed `get_set` property reports a host's write by itself: the generated code marks it changed after the
  setter returns `SetResult::Set` (SRV-57). Changes the firmware makes on its own still need `mark_changed_<name>`.
- A device of a line the driver has no client of checks the refusal and its message.
- Build and check the API crates through the host's workspace, never on their own: a `cargo check` inside one
  that is excluded from the workspace writes its own `Cargo.lock`, which may lock `exact_units` at another commit.
  The checker then loads `ww_si` types from two revisions and reports a shared trait (`ww_indication`) as changed.

## Cost

Measured on the bench (one, two and three lines, 8 Nov 2026): each extra line adds about 800 lines of generated
client code, 0.4 to 0.8 s (dev) and 0.5 to 1.7 s (release) to the driver crate's rebuild, and 125 to 133 KiB to a
stripped release binary. The API crates themselves build in no time, as `#[ww_api_root]` and `#[ww_trait]` expand
to nothing. The cost that grows is the hand-written part: about 200 lines of conversions and facade impl per line,
about half of it the same signatures and boxing in every module (CLT-64).
