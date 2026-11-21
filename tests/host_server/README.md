# host_server

Tests of the host server (SRV-12, `wire_weaver_server`): many clients on one API in one process over the in-process listener, each connection with its own backend. Covers shared calls side by side and the write barrier behind them, the connection's `CallContext` in handlers, stream pumps while a call runs, the limits (connections, requests across connections answered `Overloaded`, events) and graceful shutdown.

Byte-stream listeners (TR-19, `src/tests/byte_streams.rs`): clients over TCP with their address as the peer, streams the program hands in, and two APIs on one TCP listener (`HostServer` and `keep_trying_api::Counter` as the service `counter`, an unknown service refused). `tests/stdio.rs` starts the `stdio_server` binary with `ClientConfig::command` and talks over its stdin and stdout.
