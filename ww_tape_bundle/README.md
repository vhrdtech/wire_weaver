# ww_tape_bundle

Name paths and track keys of `ww_tape` tracks from an API bundle (`ww_self::ApiBundleOwned`).

A `ww_tape` track definition carries a numeric path (item ids and array indices). `name_path` turns it into the name
path that `ww_tape::track_key` hashes: identity names of resources (renames with `#[moved_from]` kept), array indices,
and keys' shrink_wrap bytes for keyed arrays. `key_in_bundle` gives the key under the bundle's own crate, so a reader
checks a track's key against the bundle stored in the tape.

The crate sits at layer 4 (API description), above both `ww_tape` (layer 3, storage) and `ww_self`: the tape format
itself knows no API model. Docs: [track identity](https://github.com/vhrdtech/wire_weaver/blob/master/docs/transport/tap.md#track-identity).
