# Changelog

## Unreleased

### 🚀 Features

- `HandoverRequest` / `HandoverAnswer` (CLT-43): what a program asks a direct holder of a device on the holder's socket,
  frozen like the preamble: `"WWHO"`, version (1), op (`Op::HandToHub`) or status (`HandoverStatus::Released`,
  `Refused`, `Failed`), and the preamble's skippable trailer, whose `TAG_META` entries carry the sender's client
  metadata. `files::HOLDERS_DIR` also holds the holders' sockets.

- Preamble version 2 (SRV-30): a trailer after the key, `| trailer_len: u16 LE | entries |` of `| tag | len | value |`, that a reader skips where it doesn't know an entry; later fields only come as new entries. `TAG_LINK_VERSION` and `TAG_API_MODEL_VERSION` carry the client's `ww_link` and `ww_client_server` versions (`Version`, `Version::is_compatible` by Cargo's SemVer rule); `Preamble::with_versions`, `Preamble::link_version` / `api_model_version` (None for version 1), `Preamble::v1()` for a hub that reads only version 1. `Preamble::read` takes versions 1 and 2, `PREAMBLE_VERSION` is 2, `PreambleError::TrailerTooLong`.
- `Status::Lent` (5): the preamble's answer for a device the hub lent to a program outside it (SRV-32), and `DISCONNECT_LENT`, the `DisconnectReason::Other` code its tunnels are closed with then.
- `files::OPENS_DIR` (the open counter of each device) and `files::HUB_LOG` (stderr of a hub a client started), CLT-39.

- New crate (TR-16): WireWeaver over local sockets. `runtime_dir()` (the per-user `ww` directory, created 0700 and
  refused when someone else owns it), `listen` / `connect` (a Unix socket on Linux and macOS, a named pipe on Windows,
  user-only: socket 0600, pipe DACL for the owner only and no network logons; the other end's user is checked on
  both sides), the hub `Preamble` / `Answer` (`WWHB`, frozen), and `MessageReader` / `MessageWriter`
  (`len: u32 LE | kind | payload`, cancel-safe; the reader takes a batch of messages with one read). Feature `socket` (default) has everything but `runtime_dir` and
  `files`, so the lock files of `wire_weaver_client::holders` share the runtime dir without pulling in sockets.
