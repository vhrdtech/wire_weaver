# introspect_compressed

Tests compressed introspection data (INT-16): a server built with `ww_codegen!(.., introspect_compression = "lz4")` against a plain server of the same API, checking the client gets the same bundle (sync, async, promise, dynamic client, hub) and the same API hashes. Its API is `introspect_compressed_api`.
