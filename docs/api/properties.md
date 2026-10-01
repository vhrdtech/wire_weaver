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

* Const (`const`) - property is not going to change, observe not available
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
