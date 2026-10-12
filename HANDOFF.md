# Handoff: sw-size-2 (shrink_wrap code size, items 2-4 of docs/serdes/code_size.md "What is left")

## Done (one commit each, numbers in docs/serdes/code_size.md, `tpm work say parent` sent after each)
- A (SW-38) 49230b50: latched reads of `U1`..`U64` / `I2`..`I64` (hidden `des_shrink_wrap_latch`,
  `BufReader::read_field`, `read_un*_latch`). bits_de 886 -> 846 B RV32IC. Nibble/u64/varints not latched
  (cost for one field). Tests tests/latched_reads.rs, fuzz `shrink-wrap-latched`.
- B (SW-39) 9449c3cd: byte-wise memcpy as a bench option (`just size-bench --fw-features byte_memcpy`), documented
  as advice for very small targets with downsides (user's wish: not adopted, nothing depends on it).
- f922daa4: fuzz target shrink-wrap-tail-size compared NaN values with == (test bug), now compares bytes.
- Fix 1 (SW-40) 77c86af6: bit reads vs the length nibble of the last byte (read_unx chunk limit =
  bits_in_byte_left, read_u4_rev free-nibble check). Model test `bounds_every_state_every_read` in buf_reader.rs,
  round trips tests/rev_nibble_bounds.rs. No valid layout rejected.
- Fix 2 (SW-41) 08acefcd: a kept error stops the reader (`kept()` at the start of every leaf read, byte reads share
  `take_bytes` (inline(never)), enum `_` arm checks latched() when no variant 0). +20 B busgen RV32IC; 4 forms
  measured. SW-38's all-latched rule and LATCHES consts removed with it. Test latched_errors.rs.

## Next: C, finish_for in generated server/client code (analysis done, no code yet)
Sites: wire_weaver_core/src/codegen/api_server.rs (process_request_bytes: `wr.finish_and_take()` after
EventBuilder, ~lines 245-373; `ser_method_output` ~line 1013 serializes the return value alone:
`output.ser_shrink_wrap(&mut wr); wr.finish_and_take()` -> candidate `finish_for_and_take::<RetTy>()`;
deferred `<method>_send_return` ~line 1109 `wr.finish()`), codegen/server/stream.rs:129 (StreamData event),
wire_weaver/src/context.rs `ser_into<T>` (`wr.finish()` -> `finish_for::<T>()` is exactly right there),
ww_stdlib/ww_client_server/src/util.rs (ser_ok_event / ser_err_event / ser_unit_return_event, Event).
Finding so far: `ww_client_server::Event` always has PUSHES_LEN = true (ErrorKind is evolvable -> size slot in
every error event; StreamData has a RefVec path), and every generated server has error replies, so the FIFO
encoder stays linked in any firmware with a generated server: switching to finish_for will most likely save
nothing (maybe a few bytes at the return-value site). Per the brief: measure it (a bench case that encodes a
reply of plain values the way the generated code does: return value alone into scratch, then Event with
TailBytes; the bench fw depends only on shrink_wrap, check whether ww_client_server builds there no_std), and if
it saves nothing or costs, do not land the code, write the numbers into code_size.md "What is left" item 4 and say
why (the error events). context.rs ser_into is a correctness-neutral candidate but only lands if it saves.
Wire format must not change; API snapshots must not be regenerated for published versions.

## End
- Update code_size.md "What is left" (items 2, 3, 4 status), `git rm HANDOFF.md`, `tpm work ready --note ...`
  naming what landed (SW-38, SW-39, SW-40, SW-41, C or not) and what was dropped (Nibble/u64/varint latching).
- `git worktree remove /tmp/sw-size-2-base` (temporary detached worktree of mine for before/after benches).

## Gotchas
- `just size-bench` takes ~30 s for all cases; compare against the previous run's table (save to /tmp).
- nextest output has colour codes: use `--color never` when grepping.
- Don't touch shrink_wrap_derive/src/transform/transform_ty.rs (session sw-derive-type-group); CHANGELOG
  conflicts are resolved at landing.
- Ask the user before `just build-docs` (it breaks a running `just serve-docs`); not run in this session.
