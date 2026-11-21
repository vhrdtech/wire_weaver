# host_server

Tests of the host server (SRV-12, `wire_weaver_server`): many clients on one API in one process over the in-process listener, each connection with its own backend. Covers shared calls side by side and the write barrier behind them, the connection's `CallContext` in handlers, stream pumps while a call runs, the limits (connections, requests across connections answered `Overloaded`, events) and graceful shutdown.
