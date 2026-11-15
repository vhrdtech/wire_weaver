# ww_tape_fuzz

Fuzzing of ww_tape's decoders, as a library the `fuzz/` targets and the tests share:

- `targets::<name>(data)`: the body of the fuzz target `tape-<name>`.
- `hostile::<decoder>(data)`: a block or file whose structure is right but whose counts, lengths, offsets, widths
  and layout fields take hostile values drawn from `data`, CRCs computed again.
- `targets::<group>_hostile(data)`: the body of `tape-<group>-hostile`, a hostile block through its decoders.
- `seeds/<target>/`: the seed files of each target (valid blocks from ww_tape's golden files, the inputs behind
  ww_tape's review fixes, hostile stream inputs, the known failures), written by `ww_tape_fuzz_seeds write`;
  `ww_tape_fuzz_seeds minimize TARGET HEX` shrinks a failing input.
- `case::run`: one case with its panic and any allocation over 128 MiB as its outcome.

Not published.
