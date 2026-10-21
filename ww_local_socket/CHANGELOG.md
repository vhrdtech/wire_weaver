# Changelog

## Unreleased

### 🚀 Features

- `Status::Lent` (5): the preamble's answer for a device the hub lent to a program outside it (SRV-32), and `DISCONNECT_LENT`, the `DisconnectReason::Other` code its tunnels are closed with then.
- `files::OPENS_DIR` (the open counter of each device) and `files::HUB_LOG` (stderr of a hub a client started), CLT-39.

- New crate (TR-16): WireWeaver over local sockets. `runtime_dir()` (the per-user `ww` directory, created 0700 and
  refused when someone else owns it), `listen` / `connect` (a Unix socket on Linux and macOS, a named pipe on Windows,
  user-only: socket 0600, pipe DACL for the owner only and no network logons; the other end's user is checked on
  both sides), the hub `Preamble` / `Answer` (`WWHB`, frozen), and `MessageReader` / `MessageWriter`
  (`len: u32 LE | kind | payload`, cancel-safe; the reader takes a batch of messages with one read). Feature `socket` (default) has everything but `runtime_dir` and
  `files`, so the lock files of `wire_weaver_client::holders` share the runtime dir without pulling in sockets.
