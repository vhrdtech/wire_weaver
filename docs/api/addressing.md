# Resources addressing

Primary mode of addressing to uniquely identify a resource (method, property, stream or trait) is an array of indices,
starting from an API root.
It is the most efficient way as well, utilising UNib32 variable length numbers based on nibbles
(e.g., `[0, 1, 2]` is only 2 bytes, including array length).

For example, `turn_on` address is `[0]` and `turn_off` is `[1]`.

```rust
#[ww_trait]
trait ApiRoot {
    fn turn_on();
    fn turn_off();
}
```

As a user, you do not have to worry about these indices, as they are automatically assigned and actual names are used in
generated code.

When a resource is an array, one more index is added, selecting the required one. On the server code side this index is
passed as an additional argument to user handler function.

There could be multiple arrays on the way to a resource (e.g., array of trait implementations and then an array of
resources
inside said trait). Multiple indices are added in the appropriate positions in this case.

Using this scheme requires knowing exactly which API and it's exact version that peer implements.
When this is not possible or not desired (e.g., if addressing multiple nodes at the same time) - trait addressing can be
used.

## Global trait addressing (FullVersion)

There is a possibility to make API calls on "ww-traits", without knowing the exact resource address. For example one
could
make a "sleep" call on all devices in a CAN Bus network, that support "PowerManagement" trait. Or "get_fw_version" on
any device
supporting "FirmwareInfo" trait. In order to do so, instead of relying on resource address, `FullVersion` is sent
instead.

Compared to resource addresses that can only take a few bytes, `FullVersion` is likely
to take about 8-16 bytes or more and vary with the crate name. This is unfortunate for constrained systems, or if one
wants to pack many calls into one packet.

## Global trait addressing (CompactVersion)

Solution to this is `CompactVersion`, which carries globally unique type id and major.minor version components only, all
UNib32 encoded.
The only downside is that guaranteeing globally unique IDs is not as simple as using crate's name anymore. IDs are
manually assigned and tracked via git instead
in [ww_global registry](https://github.com/vhrdtech/wire_weaver/tree/master/ww_global).

## API report

TODO: export a file describing all levels of API with IDs