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
