# keep_trying

Tests keep-trying connections end to end (CLT-75): a generated client made with `connect_keep_trying()` against a generated server on the simulated WebSocket, on tokio's paused clock. Covers the server starting late, going away and coming back while an observer keeps delivering, and `retry_now()`. Its API is `keep_trying_api`. `discovery` follows a server found over DNS-SD to its new address, on real loopback sockets (CLT-82).
