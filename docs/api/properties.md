---
level: basic
---

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

A host can watch a trait level's `+observe` properties instead of polling them, one subset per update instead of
a message per value - see [Observing properties](observing.md) for the full picture (device side, host side, the
wire format, cost vs plain streams). Declaring one is just the property flag:

```rust
#[ww_trait]
trait Motor {
    property!(ro+observe speed: f32);
    property!(ro+observe current: f32);
    property!(rw+observe mode: u8);
    property!(rw gain: f32); // not observable
}
```

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
  refuses the request): each index answered on its own (`Result<T, ErrorOwned>` per item, in order) - see
  [Multi requests](multi_requests.md) for the generated, array and dynamic-client APIs.
- With an `#[errors(..)]` list, the generated client warns when a write fails with a variant outside it (the dynamic
  client checks reads too).

The dynamic client and Python have the same per property: `dev.speed.read()`, `dev.speed.write(1500)`
([Python](../python.md#using-the-api)).

### Observing

The host side of [observing changes](#observing-changes) (CLT-28): `observe(cfg)` per trait level,
`observe_<name>(mode)` per property, `.subscribe()` returning an `Observer` with a latest-value cache, reconnect
support and more - see [Observing properties: the host side](observing.md#the-host-side) for the full API and a
dynamic-client/CLI/Python equivalent that needs no generated code at all.

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
