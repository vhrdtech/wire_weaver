# Changelog

## Unreleased

### 🚀 Features

- `medium` (feature `device`): one connection on a byte stream as a `ww_device` medium, `StreamTx` and `StreamRx`.
- `MessageReader`, `MessageWriter`, `Preamble::read` and `Answer::read` take futures-io streams, no tokio needed; `MessageWriter::close` replaces `shutdown`.
- Preamble target `Target::Service(name)` (TR-19): one listener serves several APIs; statuses `UnknownTarget`, `UnknownService` and `Busy`.
- New crate (TR-19): `Preamble`, `Answer`, `MessageReader` and `MessageWriter` moved here from `ww_local_socket`.
