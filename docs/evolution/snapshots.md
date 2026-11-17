---
level: advanced
---

# Finding snapshots

A device does not send the definitions of the traits and types it takes from other crates. Its introspection data
names each one by the crate, the crate's version, the item's name and a signature, and leaves the definition out
(`SkippedFullVersion` in the model). That keeps the data small: ww_stdlib's `examples/all_gpio_api` goes from 4363 to 72 bytes.
A host has to find those definitions again. This page explains where it looks, what it trusts, and what happens when
nothing is found.

The source of every definition is a snapshot, the file `ww api save` writes into a crate's `api_snapshots/`
(`<crate>_<major>_<minor>_<patch>.ron`, see [the evolution checker](checker_tool.md)). Every crate keeps its own
snapshots in its own repository: `ww_tap`, `ww_link`, `ww_timebase` and `ww_client_server` in this one
(`just save-snapshots`), the base crates in ww_base, the standard library's in ww_stdlib, `exact_units` in its own. There is no central copy.

## The chain

The crate `ww_snapshots` finds the snapshot of a crate version through a chain of sources, in this order. The first
source that has it wins.

1. **Embedded sets.** An application embeds snapshots at build time (below). `ww` and the Python wheel embed the
   snapshot of every crate in their build's `Cargo.lock` that has an `api_snapshots/` directory, so a device built on
   the standard library is understood offline, with nothing installed but `ww`.
2. **Directories the caller names.** Codegen, `ww api save` and `ww api check` look in the `api_snapshots/` of the
   crates they walked, and nowhere else at compile time: a build depends only on its sources. Codegen leaves a
   definition out of a device's data only when the dependency's own snapshot has exactly the same one, so an
   unpublished crate that changed after its snapshot was saved stays in-line.
3. **The cache**, `~/.wire_weaver/snapshots/`. Codegen and `ww api save` put the snapshots they used there, so a
   device built on this machine is understood by every host on it. `ww api cache PATH...` adds every crate under a
   path (a checkout of `ww_stdlib`, a product's API crates).
4. **Cargo's sources on this machine.** The workspace a `Cargo.lock` above the current directory names, its members
   and `[patch]`es, then what Cargo fetched under `CARGO_HOME`: registry crates and git checkouts. A hit is copied
   into the cache.
5. **A fetch.** The crate's `.crate` archive from crates.io, else an archive of the GitHub repository
   `~/.wire_weaver/sources.toml` names for the crate. Only hosts fetch; the evolution checker never does. A miss is
   remembered for a day.
6. **Nothing.** The definition stays skipped. The resources that use it are checked by crate version and signature
   only, and a value of it is opaque (below).

`ww api fetch CRATE VERSION` walks the chain by hand and says where it found the snapshot:

```
$ ww api fetch ww_adc 0.3.0
ww_adc 0.3.0: embedded in ww
```

## What is trusted

A device's reference is the anchor. A definition from any source is put back only when the shape signature computed
from its bytes matches the one the device sent, whatever file or server it came from. No source can make a host
decode a value with another layout than the device wrote it with.

Every file read passes the same checks, wherever it came from: the header's SHA-256 (`// shrink_wrap sha256: ..`)
must match the bytes the RON encodes to, the snapshot must be of the crate and version its name says, and a crate
or definition name must be a plain identifier before it forms a path. Sizes and counts are bounded
(`ww_snapshots::limits`): a snapshot file, a fetched archive, the entries in it, the files kept from it, the
directories a Cargo scan visits. A fetch uses `rustls` only, with timeouts and a redirect limit, and reads the
archive in memory. The cache is a per-user directory and is read with the same checks as anything else.

A snapshot refreshed in place (an unpublished version saved again) may no longer hold the definition a device names
by its old signature. `ww api save` keeps the replaced definition in `api_snapshots/superseded/`, and every source of
the chain carries that directory along: the resolver looks there by signature, then in the
[definition cache](../std_library/ww_self.md#the-definition-cache) of the host, then in the same version's snapshot in
the other sources. See [superseded definitions](inside_the_checker.md#superseded-definitions-evo-68).

## Sources and offline use

`~/.wire_weaver/sources.toml` says what a fetch may do. A missing file means crates.io only.

```toml
offline = true                      # no fetch at all; WW_OFFLINE=1 does the same for one run

[repositories]
my_api = { url = "https://github.com/owner/my_api", ref = "main" }   # a crate not on crates.io
```

A repository's archive is fetched at `ref`, the default branch when absent. Old versions' files never leave a
crate's `api_snapshots/`, so the default branch has every version's snapshot. Only GitHub repositories are fetched
from; for anything else, `ww api cache` a checkout. Every fetch logs one line naming the URL.

### Another root: `WIRE_WEAVER_HOME`

Everything a host keeps lives under one root, `~/.wire_weaver` by default: the cache, `sources.toml`, the
definition cache (`definitions/`) and the APIs that codegen and clients store. `WIRE_WEAVER_HOME=DIR` moves all of
it to `DIR`, for codegen, clients, `ww` and the hub alike.

The workspace's tests use both variables: `.config/test-home.sh`, a nextest setup script, points `WIRE_WEAVER_HOME`
(and the clock-sync state directory, `WW_STATE_DIR`) at a scratch directory and sets `WW_OFFLINE=1`. No test reads or
changes what the machine has, and none reaches the network. A test device puts its own introspection data into its
client's cache (`tests_common::api!`), as described in [introspection](../api/introspection.md).

## Embedding snapshots in an application

An application that must work offline embeds the snapshots at build time. A `build.rs` writes the set from the
workspace's `Cargo.lock`:

```rust
// build.rs
fn main() {
    ww_snapshots::build::workspace_set("stdlib_set.rs")
        .unwrap_or_else(|e| panic!("embedding API snapshots: {e:#}"));
}
```

The application includes it and registers it before it talks to a device:

```rust
static STDLIB: ww_snapshots::Set = include!(concat!(env!("OUT_DIR"), "/stdlib_set.rs"));

fn main() {
    ww_snapshots::embed(&STDLIB);
    // ...
}
```

Every crate the lock names that has an `api_snapshots/` directory is embedded, superseded definitions included,
each file checked at build time. The embedded set is the first source of the process-wide resolver,
`ww_snapshots::Resolver::global()`, which `wire_weaver_client` and everything on it use. A set embedded by one crate
is seen by every crate of the process.

## The opaque value

A value whose type reaches a definition the chain did not find cannot be decoded, and with the size of an unsized
type unknown, nothing after it in the same message can be either. The dynamic client then yields one opaque value for
the property, return value or stream item: the crate, its version, the type's name, the signature and the raw bytes
(`ww_self::Value::Opaque`). `ww` prints it with the fix:

```
<opaque my_api::Status 0.3.0 0x0a1f...: no snapshot of my_api 0.3.0, see `ww api fetch`>
```

Python gets a `ww.Opaque` object with the same fields. Sending a value into such a type is refused with the same
message. A device never sends an opaque value: it exists on the host only.
