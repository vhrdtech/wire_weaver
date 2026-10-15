# Properties

A property is a value of the device the host reads, writes or both: a setting, a measured state, a mode.

```rust
#[ww_trait]
trait MyDevice {
    property!(ro button_pressed: bool);
    property!(rw speed: f32);
    property!(wo beep: u16);
    #[errors(TooBig, NotSet)]
    property!(rw mode: u8, ModeError);
    /// Known at compile time, never changes while the firmware runs.
    const LIMITS: Limits<'i>;
}
```

| Access | Declared as | Read | Write | Observe |
|---|---|---|---|---|
| read/write | `property!(rw name: T)` | ✓ | ✓ | with `rw+observe` |
| read only | `property!(ro name: T)` | ✓ | | with `ro+observe` |
| write only | `property!(wo name: T)` | | ✓ | |
| const | `const NAME: T;` | ✓, once per connection | | |

`ro` means read only for the host: the device changes the value whenever it likes, and may tell observers about it.
`+observe` lets hosts observe it, see [Observing changes](#observing-changes).
A `const` never changes while the firmware runs, see [Const items](#const-items).

Any type the SerDes system supports can be a property. A second type after the value's, `property!(rw mode: u8,
ModeError)`, is the property's error type: reads and writes may fail with it. `#[errors(..)]` above such a property
lists the variants it returns, as on methods (see [Errors](errors.md#which-errors-a-method-returns)). `#[optional]` /
`#[optional(group)]` lets a device leave a property out (see [Optional items](optional_items.md)), and a property can
be an [array](arrays.md), `property!(rw gain[]: f32)`.

## Device side

A server implements each property in one of two ways, picked per property with the `property_model` argument of
`ww_codegen!`: a comma separated list of `regex=get_set|value_on_changed`, the regex matching the property's name, `_`
for the rest; `get_set` is the default.

```rust
mod server_impl {
    wire_weaver::ww_codegen!(
        my_device_api :: MyDevice for super::Server,
        server = true, no_alloc = true, use_async = false,
        property_model = "speed=value_on_changed, _=get_set",
    );
}
```

`ww api scaffold <api_crate> --property-model ".."` prints the handlers and fields either way.

### `get_set`: handlers

User code provides `get_<name>` for reads and `set_<name>` for writes:

```rust
impl Server {
    fn get_button_pressed(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> GetResult<bool, ()> {
        Value(self.button.is_low())
    }

    fn set_beep(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>, freq: u16) -> SetResult<()> {
        self.buzzer.start(freq);
        Set
    }

    fn get_mode(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> GetResult<u8, ModeError> {
        match self.mode {
            0 => GetError(ModeError::NotSet),
            mode => Value(mode),
        }
    }

    fn set_mode(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>, mode: u8) -> SetResult<ModeError> {
        if mode > 10 {
            return SetError(ModeError::TooBig);
        }
        self.mode = mode;
        Set
    }
}
```

- `GetResult<T, E>`: `Value(v)` answers the read with `v`; `GetError(e)` answers with the error; `Unimplemented`
  answers `ErrorKind::Unimplemented`.
- `SetResult<E>`: `Set` acknowledges the write; `SetError(e)` answers with the error, the value is not applied unless
  the handler did so; `Unimplemented` answers `ErrorKind::Unimplemented`.
- `E` is the property's error type, `()` without one. The error goes to the host serialized, as
  `ErrorKind::UserBytes`.

Both answer right away: a property has no deferred answer. Handlers get `cx: &mut Context<'_, impl BlockingEventOut,
Medium>` (`impl EventOut` and `async fn` for `use_async = true`), properties in arrays their index chain after it.

### `value_on_changed`: a field

Generated code reads and writes a field of the server struct named like the property, and calls `changed_<name>`
after a write that changed the value (the type needs `PartialEq`; writing the same value calls nothing):

```rust
struct Server {
    speed: f32,
}

impl Server {
    fn changed_speed(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {
        self.motor.set_speed(self.speed);
    }
}
```

Reads and writes can't fail with this model: a property whose writes have to be checked uses `get_set`. Properties of
nested traits carry the trait item chain in their names (`self.motor_speed`, `changed_motor_speed`), as every handler
does.

### Observing changes

A host observes the `+observe` properties of a trait level (the root API, or a trait implemented with `ww_impl!`)
with one `Observe` request to the level's path: per property `Off`, `OnChange { min_us }`, `Periodic { every_us }` or
`OnChangeOrBeat { min_us, max_us }`, and a budget in bytes per second for the level. The device answers with what it
granted and from then on sends one `PropertyUpdate` event per level whenever any of its observed properties is due,
carrying just those: a sparse `<Trait>Update` with an `Option` per observable property, at the slot of its resource
index (wire format: `ww_client_server::observe`, API-38).

```rust
#[ww_trait]
trait Motor {
    property!(ro+observe speed: f32);
    property!(ro+observe current: f32);
    property!(rw+observe mode: u8);
    property!(rw gain: f32); // not observable
}
```

For an API with `+observe` properties the generated server needs the `ww_stream` crate and three things from the
user: an `api_impl::ObserveState` in the server struct, handed out by `observe_state()`, a call to the generated
`poll_properties` in the event loop, and a `mark_changed_<name>()` call where the device changes a value by itself.
`ww api scaffold` prints the field and `observe_state()`.

```rust
struct Server {
    observe: api_impl::ObserveState,
    // ..
}

impl Server {
    fn observe_state(&mut self) -> &mut api_impl::ObserveState {
        &mut self.observe
    }

    fn on_new_measurement(&mut self, current: f32) {
        self.current = current;
        self.mark_changed_current(); // sent to hosts observing it on change
    }
}

let mut server = Server {
    // shortest period, per level and total budget (the properties' share of the link), timestamps
    observe: api_impl::ObserveState::new(ww_stream::observe::ObserveLimits::DEFAULT),
    // ..
};

// event loop, every millisecond or so, between requests
let mut buf = [0u8; 256]; // as big as the biggest update the link carries
server.poll_properties(&mut ww_server.sink(), medium, now_us, &mut buf).await?;
server.poll_streams(&mut ww_server.sink(), now_ms, &mut stream_buf, 4096).await?; // stored streams after
```

- **When a property is due**: `Periodic` on a fixed grid, never early; `OnChange` after `mark_changed_<name>()` and at
  least `min_us` after its previous send; `OnChangeOrBeat` also once `max_us` passed without a send. `value_on_changed`
  properties are marked changed by the generated code after a write that changed them; `get_set` ones by the device.
  A property that starts being observed is sent once right away, so the host has a value to start from.
- **Values**: `get_set` properties are read through their getter when they are sent (values without a size bound,
  such as strings, also once before that to measure them), `value_on_changed` ones from their field. A getter that
  fails skips that send.
- **Budget**: each level has a token bucket at its granted bytes per second, holding a tenth of a second of it (64
  bytes at least); all levels together get at most `ObserveLimits::total_budget`. Each `poll_properties` call sends at
  most one update per level: its due properties earliest deadline first, as many as fit the bucket and `buf`. When
  they don't all fit the bucket and it isn't full yet, the level sends nothing and holds until they fit or the bucket
  is full: a level short of budget sends fewer updates that carry everything due, rather than one value per call,
  each with its own header (see the [motor demo](../examples/motor_demo.md#a-level-short-of-budget)). What is left out
  slips to a later call; a property left out keeps its deadline, so it goes first then, and none starves.
- **Missed**: a periodic property counts one missed send for every whole period it falls behind, an on-change one
  when it waited its `min` or longer; held back for less, it is only late. The count goes to the host in the next
  update's `missed`.
- **Untrusted input**: an `Observe` request is checked in full before anything changes. Entries for an unknown index,
  a property without `+observe`, an index given twice or a mode the device doesn't know are refused one by one and
  listed in the answer; periods are raised to `ObserveLimits::min_period_us` and the budget is clamped. A request that
  doesn't decode, or has more than 16 refused entries, fails with `ArgsDesFailed` / `BadIndex` and changes nothing.
  When the answer doesn't fit the reply buffer, the request fails with `ResponseSerFailed` and is undone too.
- **Limits**: a level inside a resource array (`ww_impl!(bank[]: Bank)`) can't be observed yet (the bound of its
  indices isn't known, API-29), `Observe` there answers `OperationNotSupported`; nor can `+observe` properties that are
  in an array themselves or `#[optional]`. Observing stays set while the link is down; `ObserveState::reset()` stops
  it all.

**Sharing the link with stored streams**: properties have a reserved share of the link, the sum of the granted
level budgets, at most `ObserveLimits::total_budget`; reliable streams get what `poll_streams` is given per turn.
Polling properties first keeps their short deadlines, and their share bounds what they take from the streams.

#### Before observe requests

`stream_data_ser().<name>_send(&value, out)` (`_send_blocking` for sync servers) still sends one property's value as
stream data on its own path. Generated clients receive it with `observe_<name>()` for properties without `+observe`
(a plain `Stream<T>`, no request is sent to the device); for `+observe` properties `observe_<name>(mode)` is the
observe request below. It is superseded by observe requests.

## Host side

Generated clients have `read_<name>()`, `write_<name>(value)` and `observe_<name>()` per property, as its access
allows. Reads and writes are prepared first, then sent async, blocking or as a promise:

```rust
let speed = device.read_speed().read().await?;            // Result<f32, Error>
device.write_speed(1500.0).write().await?;                // waits for the device's acknowledgement
device.write_speed(1500.0).write_forget().await?;         // no answer asked for, nothing waited for
let pressed = device.read_button_pressed().blocking_read()?;
let mut read = device.read_speed().read_promise("speed"); // poll it from a GUI
```

- A write to a property with an error type returns `Result<(), CallError<E>>`: `CallError::Device(e)` is the
  device's error, decoded, see [Errors](errors.md#on-the-client-one-per-call). Without one it returns
  `Result<(), Error>`.
- A read returns `Result<T, Error>`; a device's error arrives as `Error::RemoteError` with `ErrorKind::UserBytes`,
  the error's bytes (`E::from_ww_bytes`).
- `ErrorKind::Unimplemented` (`CallError::Unimplemented { .. }` for writes) is a handler returning `Unimplemented`, or a
  property the device left out.
- `.with_timeout(..)` overrides the request timeout for one read or write.
- Several resources in one request, when the server is generated with `multi_req = true` (a server without it
  refuses the request): each index is answered on its own (`Result<T, ErrorOwned>` per item, in order).
    - Different resources at the same level: `(device.read_x(), device.read_y(), device.read_mode()).multi_read().await`
      and the `multi_call`/`multi_write` equivalents, implemented for tuples of 2 to 8 of the generated
      `PreparedRead`/`PreparedCall`/`PreparedWrite` builders, at any depth (any resources that share a common path
      prefix and diverge at exactly one element, optionally followed by one more shared element — e.g. the same
      property across two array elements).
    - The same resource across a whole array: `read_array`/`call_array`/`write_array` (+ `_blocking`) take a
      `MultiIndexOwned` selector (`Range`/`List`/`Mask32`) and the existing generated per-index accessor, e.g.
      `read_array(MultiIndexOwned::Range(UNib32(0)..UNib32(4)), |i| client.pin(i).read_mode()).await`. The dynamic
      client has the same as `DynResource::multi_read`/`multi_write`/`multi_call`.
    - Whether each index carries the same argument/value or a different one per index (`MultiArgs::Same`/
      `Different`) is chosen automatically from the serialized bytes.
- With an `#[errors(..)]` list, the generated client warns when a write fails with a variant outside it (the dynamic
  client checks reads too).

The dynamic client and Python have the same per property: `dev.speed.read()`, `dev.speed.write(1500)`
([Python](../python.md#using-the-api)).

### Observing

The host side of [observing changes](#observing-changes) (CLT-28). Each trait level with `+observe` properties gets
`observe(cfg)` on its client (the root client, or `device.motor()`), each `+observe` property `observe_<name>(mode)`;
both are prepared first and `.subscribe()` (`.subscribe_blocking()`) returns an `Observer` once the device answered:

```rust
use wire_weaver_client::observe::{on_change, on_change_or_beat, periodic};
use api_client::motor_api_motor::MotorObserve; // in the level's client module (`<api crate>_<trait>`)

let ms = Duration::from_millis(1);
let mut motor = device
    .observe(MotorObserve::default().speed(periodic(10 * ms)).current(on_change(ms)))
    .budget(20_000)                      // bytes per second, the device decides without it
    .subscribe()
    .await?;
let granted = motor.granted().unwrap();  // what the device applies: Granted { modes: MotorObserve, budget, refused }
while let Ok(u) = motor.recv().await {   // Observed { value: MotorUpdate, t, missed, received }
    if let Some(speed) = u.value.speed { /* .. */ }
}
motor.with_latest(|l| l.speed.as_ref().map(|s| (s.value, s.received)));

let mut mode = device.observe_mode(on_change(ms)).subscribe().await?; // Observer of u8 values
let m = mode.recv().await?.value;
mode.configure(MotorObserve::default().mode(periodic(100 * ms))).await?;
```

- **Types**: per level `<Trait>Update` (`<Trait>UpdateOwned` when a property holds a `String` or `Vec`), the level's
  `PropertyUpdate` data, one `Option` per `+observe` property, the ones sent present; `<Trait>Observe`, one
  `Option<ObserveMode>` per property with a setter each, for asking and in `Granted::modes` for what was granted;
  `<Trait>Latest`, the latest `Sample { value, received, t }` of each property. `t` is the device's time, present when
  it sends timestamps (`ObserveLimits::timestamps`): microseconds, a wrapping `u32` on the wire, unwrapped into a
  `u64` on the host.
- **One subscription per level**: every observer of a level asks for its own modes, the host sends the device one
  `Observe` that covers them all, per property the shortest on-change interval and the shortest period of all of
  them (`OnChange` and `Periodic` give `OnChangeOrBeat`), the biggest budget. It is sent again when an observer
  comes, calls `configure(..)` / `configure_with_budget(..)`, or is dropped; dropping the last one turns the level
  `Off`. Each observer gets every update of its level, a property observer only the updates that carry its
  property. The settings are what the device applies for all observers together, so an observer may get more often
  than it asked for.
- **Events**: `recv()` / `recv_blocking()` / `try_recv()` give updates, `recv_event()` also `Disconnected`, `Resumed`
  and `Dropped { events }` (the observer's queue on the host is bounded like a stream's,
  `ClientConfig::stream_queue_bytes`).
- **Reconnects**: observers outlive `disconnect().keep_streams()`; `resume_streams()` on the next connection sends
  every level's subscription again (the device may still have it, `ObserveState::reset` is the device's choice) and
  the observers see `Resumed`. A link that goes down for good ends them (`StreamError::Closed` once drained).
- **Old devices**: a device whose API model is `ww_client_server` before 0.2 can't observe, `.subscribe()` fails
  with `Error::Unsupported`.
- **Limits**: levels inside a resource array have no `observe()` yet, like on the device. Absolute paths only: a
  trait client (`client = "trait_client"`) observes once it has a base path (`attachment()`).

#### Without generated code

The dynamic client, `ww` and Python have the same, decoding a level's `PropertyUpdate` from introspection alone
instead of a generated `<Trait>Update`/`<Trait>Observe` (TOOL-11): a level's resource indices and types come from
`ApiBundleOwned`, reusing the `Level<U>`/`Observer<U, T>` machinery above unchanged — one device subscription per
level, `t` unwrapped, reconnect/resume included.

- **`DynResource`**: `observable_properties()` lists a level's `+observe` properties (`(resource index, name,
  type)`, what `entries` below addresses); `observe(cmd, entries)` / `observe_blocking(..)` (`entries`:
  `BTreeMap<u32, ObserveMode>`) build a `wire_weaver_client::DynObserver`, decoding each update's sparse
  `OptionAnyVec` (DER-13) by resource index instead of a generated struct's fields.
- **`DynObserved`**: `values` (`Vec<(String, ValueOwned)>`, present properties by name, in resource-index order),
  `t`, `missed`, `received`, and `data_len` (the update's raw byte length, e.g. for measuring link bytes/s).
- **CLI**: [`ww observe <path> [--rate MS]`](../cli.md#ww-observe) prints each update's values, the measured update
  rate and the link bytes/s it costs, until Ctrl+C.
- **Python**: [`node.observe(cfg)`](../python.md#observing) iterates the same updates as dicts.

## Const items

A `const` item of a trait is a value that doesn't change while the firmware runs: a firmware name and version, a
board record, chip ids, limits of an implementation.

```rust
#[ww_api_root]
trait Device {
    /// Known at compile time.
    const LIMITS: Limits<'i>;
    /// Read from the chip at boot.
    const SERIAL: &'i str;
    ww_impl!(board: ww_board_info::BoardInfo);
}
```

On the server, no handler is called for it. The value comes from the generated trait `<RootTrait>Consts`, which the
server struct implements, in one of two forms, picked per item with the `const_model` argument of `ww_codegen!`:

- `const` (the default): a Rust const, `const LIMITS: Limits<'static> = ..;`.
- `fn`: a value fixed at boot or link time (OTP, a chip id register, the build id in flash), `fn serial(&self) ->
  &str`. Items in an array of traits get the index chain: `fn channel_gain(&self, index_chain: [UNib32; 1]) -> u8`.

Names carry the trait item chain prefix, as handler names do: `BOARD` of `board: BoardInfo` is `BOARD_BOARD` as a
const and `board_board` as a `fn`. `const_model` is a comma separated list of `regex=const|fn`, the regex matching
the whole name of the item as written in its trait (`BOARD`, not `board_BOARD`); `_=fn` changes the default.

```rust
impl api_impl::DeviceConsts for Server {
    const LIMITS: Limits<'static> = Limits { max_len: 64, names: RefVec::Slice { slice: &["a", "b"] } };

    fn serial(&self) -> &str {
        &self.serial
    }

    fn board_board(&self) -> BoardSummary<'_> {
        self.board.summary()
    }
}

mod api_impl {
    wire_weaver::ww_codegen!(
        device_api :: Device for super::Server,
        server = true, no_alloc = true, use_async = false,
        const_model = "SERIAL|BOARD=fn",
    );
}
```

`ww api scaffold --const-model "SERIAL|BOARD=fn" <api_crate>` prints this impl with a `todo!()` for every value. An
`#[optional]` const left out with `omit = [..]` has no item in the trait. A trait without `const` items has no
`<RootTrait>Consts`. A const can't fail, so it has no error type and no `#[errors(..)]`.

Clients read a const from the device once per connection and keep the value: later reads of it (generated
`read_<NAME>()`, async, blocking or as a promise, the dynamic client and Python) return it without a request. A new
connection, or a `disconnect()` through the same `Commander`, reads it again; `Commander::clear_const_cache()` does
too. Errors are not kept. `ww introspect` reads the consts that are not in an array of traits and shows their values
in the tree (`--no-values` to skip them).
