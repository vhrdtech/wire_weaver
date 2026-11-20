# tests_common

Shared test support, not a `<name>`/`<name>_api` pair: runs a generated server on `ww_device::Server` in a
device thread, connected to the host event loop through the `in_process` transport (optionally `local_socket`
or the simulated `ws` WebSocket, behind their own features).

Tests drive it with a generated client exactly as a real device over USB, link setup, timeouts and streams
included. Used as a dev-dependency by most other `tests/*` crates.
