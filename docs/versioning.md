# Versioning

Each type and "ww-trait" version is it's crate version, same versioning rules apply.
Types and "ww-trait's" are globally identified by their crate name and version. `FullVersion` type is provided in
`ww_version` crate
that carries crate name in addition to version numbers.

## Compact ww-trait version

There is a possibility to make API calls on "ww-traits", without knowing the exact resource path. For example one could
make a "sleep"
call on all devices in a CAN Bus network, that support "PowerManagement" trait. Or "get_fw_version" on any device
supporting "FirmwareInfo" trait. In order to do so, instead of relying on resource path (a vector of numbers from API
root), `FullVersion` is sent instead.

Compared to resource paths that can only take a few bytes (numbers are UNib32 encoded, so the smallest path is 4 bits),
`FullVersion` is likely
to take about 8-16 bytes or more and vary with the crate name. This is unfortunate for constrained systems, or if one
want to pack many calls into one packet.

Solution to this is `CompactVersion`, which carries globally unique type id and major.minor version components only, all
UNib32 encoded.
The only downside is that guaranteeing globally unique IDs is not as simple as using crate's name anymore. IDs are
manually assigned and tracked via git instead.
