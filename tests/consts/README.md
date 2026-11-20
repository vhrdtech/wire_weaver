# consts

Tests `const` items (API-36): served from the server's consts implementation (a Rust const or a `fn`) and read by clients once per connection.

Runs a mock server on `ww_device::Server` through `tests_common::start_device`, clients through the real host event loop.
