---
level: advanced
---

# Who connects: client meta

A device, a log or a list of holders is more useful when it can say *who* is on the other end of a link: "`ww`, pid 4711, started by roman on pc1, a command line tool" instead of "a host". Every client therefore tells the device who it is in link setup, in an optional `client_meta` field of `ww_link`'s `LinkSetup`. The hub (SRV-13) and the holder lock files (CLT-38) show the same fields.

Nothing decides on it: it is free text for people, it is not authentication, and a client can say what it likes.

## What is sent

| Field | Meaning | Default |
|---|---|---|
| `program` | program name | file name of the running executable, without `.exe` |
| `app_version` | version of the program, free text | empty |
| `label` | a name the user gave this client | empty |
| `kind` | `Cli`, `Gui`, `Notebook`, `Service`, `Hub`, `Test` (`Unknown`; a kind added later reads as `Newer`) | `Cli` when the parent process is a shell, else `Unknown`; Python sets `Notebook` |
| `pid` | process id | this process |
| `user` | login name | `$USER`, `$LOGNAME` or `$USERNAME` |
| `host` | host name of the PC | the PC's host name |
| `purpose` | what the program does with the device, e.g. "DFU update" | none |
| `instance_id` | random number per client object, shared by all its connections | picked when the `ClientMeta` is created |

The parent is a shell when its process name is one of `bash`, `zsh`, `fish`, `sh`, `dash`, `ksh`, `nu`, `pwsh`, `powershell` or `cmd`: read from `/proc` on Linux, `proc_name` on macOS and a Toolhelp snapshot on Windows. A GUI, a service or a test says what it is itself.

Long values are cut (names to 48 bytes, version 24, purpose 64) so that the message stays small. A device with a tiny receive buffer gets a shortened form (program, user and host cut to 16 bytes, no label, version or purpose), and one that can't take even that gets none: the connection never fails because of the meta.

## Setting it

```rust
use wire_weaver_client::{ClientConfig, ClientKind, ClientMeta};

let config = ClientConfig::new()
    .usb()
    .client_meta(
        ClientMeta::new()
            .kind(ClientKind::Gui)
            .app_version(env!("CARGO_PKG_VERSION"))
            .label("bench 2")
            .purpose("calibration"),
    );
```

Everything not set is filled in. `no_client_meta()` sends nothing. The instance id is picked when `ClientMeta::new()` runs, so every connection made from clones of one config shares it, and two clients of the same program and pid are told apart. `wire_weaver_py`'s `connect()` sets the kind to `Notebook`.

## On the device

`ww_device::DeviceLink::peer_meta()` returns the last host's `PeerMeta` (program, user and host cut to 24 bytes, kind, pid, instance id) or `None` for a host that sent nothing. It is set before `LinkEvent::Up` is reported, so firmware that wants to log who connected reads it there:

```rust
if let Some(LinkEvent::Up) = server.handle(ready, &mut backend).await {
    if let Some(peer) = server.link().peer_meta() {
        info!("host: {} ({}) on {}", peer.program.as_str(), peer.pid, peer.host.as_str());
    }
}
```

It costs about 90 bytes of RAM in the `DeviceLink`.

## Compatibility

`client_meta` is appended to `LinkSetup` as `#[default = None] Option<ClientMeta>`, the same way the setup token was added. A device built before it reads the longer message and ignores the end; a host built before it sends a `LinkSetup` that ends before it and a new device reads `None`. Golden bytes of both forms are in the conformance corpus (`link_setup.without_meta`, `link_setup.with_meta`), and `ClientKind` has `future_version = Newer`, so a kind added later still decodes (as `Newer`). Adding a field to `LinkSetup` is wire-compatible, see [evolution rules](../evolution/rules.md).
