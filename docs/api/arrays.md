# Arrays

Any resource can also be an array - method, property, stream and even a trait implementation:

```rust
#[ww_trait]
trait ArrayOf {
    fn run<N: u32>();
    stream!(adc[]: u16);
    property!(led[]: bool);
    ww_impl!(motor[]: ww_motor_control "0.1.0" :: Motor);
}
```

TODO: size bounds

Traits inside other traits can also contain arrays, all the indices leading up to them are accumulated and passed as
Rust array `[u32; N]` argument into a corresponding user handler.

That way generated code can be kept efficient and simple, because the whole API tree is essentially flattened and
simple function calls are used to interface with user provided implementation. At least that is the case for now on
`no_std` targets.

### Array of resources vs resource of array

Here, resource led is itself an array, when accessing it - an index will be added to the resource path.
Each one of three bool's is accessed separately from each other.

```rust
#[ww_trait]
trait ArrayOfResources {
    property!(led[3]: bool);
}
```

On the other hand, here led is not an array, but its type is. All three boolean's are accessed in one go.

```rust
#[ww_trait]
trait ResourceOfArrays {
    property!(led: [bool; 3]);
}
```

Both can be used together as well, for example:

```rust
#[ww_trait]
trait ArrayOfArrays {
    property!(rgb_led[3]: [u8; 3]);
}
```
