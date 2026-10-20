# ww-lz4 handoff (SRV-27)

Done: crate `ww_lz4` (encoder, std Decoder, tests, examples `ratio` and `liblz4_vectors`), `just test-lz4-c` (C liblz4 via python-lz4, `uv run --script`), CI job `lz4-c`, FEATURES SRV-27, features page entry, CHANGELOG.

Decisions:
- `Encoder<HASH, BUF>` with HASH = table entries: stable Rust can't size `[u16; 1 << HASH_LOG]` (parent agreed).
- `input(want)`: a no-arg input that always leaves `BUF - keep` free would memmove `keep` bytes every chunk (parent agreed).
- Table holds stream positions truncated to u16; aliases are caught by the 4-byte compare, the table is never rewritten on a slide. Ratios equal lz4s within 0.1 % on the P2534 captures (copied from gpd-omarchy:/tmp/uartz/data, not committed).
- Back-off counted in bytes (1 KiB missed -> retry every 4 KiB); counting chunks (2 misses) cost 12 % on boot log at 64 B chunks.
- Decoder::new() takes no size: decode pre-scans the block for its decoded length.
- `ram` only applies on `target_os = "none"` (code in .data on a host would segfault).

Next: cycles on B153A with fwbench swapped to ww_lz4 (API-55 can do it); git rm this file before tpm work ready.
