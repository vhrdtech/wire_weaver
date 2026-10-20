# /// script
# requires-python = ">=3.12"
# dependencies = ["lz4>=4.3"]
# ///
"""Decodes the blocks of `cargo run -p ww_lz4 --example liblz4_vectors` with C liblz4 (LZ4_decompress_safe_usingDict
through python-lz4), the earlier bytes of each stream as dictionary, and checks them against the plain bytes."""

import sys

import lz4
import lz4.block

streams = blocks = 0
end = False
history = b""
name = ""
for line in sys.stdin:
    line = line.strip()
    if line.startswith("# "):
        name, history, streams = line[2:], b"", streams + 1
        continue
    if line == "end":
        end = True
        continue
    if line == "reset":
        history = b""
        continue
    block_hex, _, plain_hex = line.partition(" ")
    block, plain = bytes.fromhex(block_hex), bytes.fromhex(plain_hex)
    got = lz4.block.decompress(block, uncompressed_size=max(len(plain), 1), dict=history[-65536:])
    if got != plain:
        sys.exit(f"{name}: block {blocks} decodes to {len(got)} B, not the {len(plain)} B sent")
    history += plain
    blocks += 1

if streams == 0 or not end:
    sys.exit("the vectors stopped before their end (did the example fail?)")
print(f"liblz4 {lz4.library_version_string()} decoded {blocks} blocks of {streams} streams")
