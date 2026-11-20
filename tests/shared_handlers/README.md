# shared_handlers

Servers with shared handlers (SRV-65): a blocking `no_alloc` server and an async allocating one answer every request of `shared_handlers_api` through `process_request_bytes` and `process_shared_bytes`, and `is_shared` says which entry a request belongs to. Also checks side-by-side shared calls, the connection's `CallContext` in handlers and, with trybuild (`just test-shared-ui`), that a `&mut self` handler of a shared item does not compile.
