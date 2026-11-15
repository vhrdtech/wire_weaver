# ww_tape_fuzz

Fuzzing of ww_tape's decoders, as a library the `fuzz/` targets and the tests share:

- `targets::<name>(data)`: the body of the fuzz target `tape-<name>`.
- `hostile::<decoder>(data)`: a block or file whose structure is right but whose counts, lengths, offsets, widths
  and layout fields take hostile values drawn from `data`, CRCs computed again.
- `targets::<group>_hostile(data)`: the body of `tape-<group>-hostile`, a hostile block through its decoders.

Not published.
