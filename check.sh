#!/usr/bin/env bash

cargo check --quiet --workspace --all-targets
cargo check --quiet --workspace --all-features --lib --target thumbv6m-none-eabi
cargo fmt --all -- --check
cargo clippy --quiet --workspace --all-targets --all-features --  -D warnings -W clippy::all
cargo test --quiet --workspace --all-targets --all-features
cargo test --quiet --workspace --doc