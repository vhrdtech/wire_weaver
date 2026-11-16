# Changelog

## Unreleased

### 🚀 Features

- New crate (EVO-80): snapshots resolved by crate name and version through embedded `Set`s, the cache `~/.wire_weaver/snapshots/`, Cargo's sources and a fetch; every file checked against its SHA-256 header and its crate version; `file`, `dir`, `cache`, `limits`, `Set`.
