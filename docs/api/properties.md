# Properties

Properties of any type can be defined as follows:

```rust
#[ww_trait]
trait MyDevice {
    property!(ro button_pressed: bool);
    property!(rw speed: f32);
}
```

Property write is acknowledged by a server, unless request ID of 0 is used.

Properties have access mode associated with them:

* Const (`const NAME: T;` in the trait) - value is fixed for the life of the firmware, observe not available, see
  [Const items](#const-items)
* Read only (`ro`) - property can only be read, can change and be observed for changes
* Write only (`wo`) - property can only be written
* Read/Write (`rw`) - property can be read, written and observed for changes

There are two supported way of implementing properties on the server side:

* get / set - user code provides `get_speed` and `set_speed` implementation.
* value / on_changed - generated code directly reads and writes `speed` field and calls user provided `speed_changed`
  implementation.

### Observing changes

`ro` and `rw` property updates are sent as stream data on the property's path: the server side generates
`stream_data_ser().<property>_send(&value, out)` (`_send_blocking` for sync servers) and the client
`observe_<property>()` / `observe_<property>_blocking()`, returning a `Stream` of new values. Sending is up to the
server, e.g., from `set_speed` or `changed_speed` through their `cx`, or from the event loop through `server.sink()`
when a `ro` property changes on its own:

```rust
fn set_speed(&mut self, cx: &mut Context<'_, impl BlockingEventOut>, speed: f32) -> SetResult<()> {
    self.speed = speed;
    _ = server_impl::stream_data_ser().speed_send_blocking(&speed, cx);
    Set
}
```

```rust
let mut speed = client.observe_speed().await?;
while let Ok(speed) = speed.recv().await { /* .. */ }
```

### Fallible property set

Sometimes setting a property can result in an error, in such cases user defined error can be specified as well:

```rust
#[ww_trait]
trait MyDevice {
    property!(rw mode: Mode, Error);
}
```

Now the expected signature of set method is: `set_mode(mode: Mode) -> Result<(), Error>`.
When `Err` variant is encountered, generated server code will serialize user error into bytes and forward it to
client in `ww_client_server::ErrorKind::UserBytes(err_bytes)`.

When `on_changed` flavor is used, it's signature and behavior is changed accordingly.

### Const items

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

* `const` (the default): a Rust const, `const LIMITS: Limits<'static> = ..;`.
* `fn`: a value fixed at boot or link time (OTP, a chip id register, the build id in flash), `fn serial(&self) ->
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
`<RootTrait>Consts`.

Clients read a const from the device once per connection and keep the value: later reads of it (generated
`read_<NAME>()`, async, blocking or as a promise, the dynamic client and Python) return it without a request. A new
connection, or a `disconnect()` through the same `Commander`, reads it again; `Commander::clear_const_cache()` does
too. Errors are not kept. `ww introspect` reads the consts that are not in an array of traits and shows their values
in the tree (`--no-values` to skip them).
