# Handoff: sink-switch (API-34 piece 2)

Done on this branch (parent's commits 2f025b37..6c548128 included): server codegen for credit sinks on `StreamOpen` / `SinkWrite(Units)` / `Ack` / `Query` / `Resend` / status read (+ `Open` to the handler on a new session), host `Sink` on `SinkTx` (`send_units`, `open_session`, `SinkSessionLost`), dynamic client and Python units, every user moved (tests, dfu_scenarios transcripts, H725 uart.rs, Donguru on typec_dongle branch s/19oct2026-sink-switch ac427bf), old credit messages / `CreditWrite` / `ww_client_server::credit` removed, `ResendFrom` -> `Resend`, docs and changelogs.

Gotcha: the per-user /tmp quota; run cargo with `CARGO_TARGET_DIR=/mnt/data/omarchy/cargo-build/target-sink-switch TMPDIR=/mnt/data/omarchy/cargo-build/tmp-sink-switch`.

Next (piece 3, sink-resume): `Sink::cursor()`, sinks in `resume_streams()`, `<sink>_resume(cursor)`, DFU plan continuing after a reconnect.
