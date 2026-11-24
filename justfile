# `just jobs=N <recipe>` caps build and test parallelism for that run (test, test-all, lint, check and anything
# they call); unset (the default) leaves CARGO_BUILD_JOBS / NEXTEST_TEST_THREADS as inherited, or as cargo's own
# default (the CPU count, a number maturin can read too) when not set (tpm's gate sets them itself for its own runs).
jobs := ""
export CARGO_BUILD_JOBS := if jobs != "" { jobs } else { env_var_or_default('CARGO_BUILD_JOBS', num_cpus()) }
export NEXTEST_TEST_THREADS := if jobs != "" { jobs } else { env_var_or_default('NEXTEST_TEST_THREADS', 'num-cpus') }

# List all available targets
default:
    @just --list

# The tests. `just test`: all of them (test-all), the gate run before merging. `just test --changed`: only the
# workspace crates changed against main and the crates that depend on them, what to run before opening a PR
# (uses our internal tooling when available, otherwise falls back to the full suite).
test *flags:
    {{ if flags == "--changed" { "changedtests run" } else { just_executable() + " test-all " + flags } }}

# Test everything
test-all:
    cargo nextest run --workspace --no-fail-fast
    # doctests: nextest doesn't run them, so plain cargo test for this one case
    cargo test --workspace --doc
    # a server built as a firmware is, without ww_client_server's multi and streams, refuses their requests (SRV-60)
    just test-lean-server
    # the conformance corpus through Python (TEST-23)
    just test-conformance-py
    # the docs site's Markdown extensions (the `bits` fence)
    uv run --group dev pytest -q docs_ext
    # ww_lz4's blocks decoded by C liblz4
    just test-lz4-c
    # ww_ecc_flash's read modes on a simulated flash (mcu/ workspace)
    just test-ecc-flash
    # the wasm module images on wasefire-interpreter and wasmi (ww_wasm/ workspace, nightly)
    just test-wasm
    # facade!'s compile errors (CLT-64): trybuild, too slow for nextest's hang limit
    just test-facade-ui
    # the client alone, as a downstream crate builds it: no dev-dependency or workspace member adds tokio features
    cargo check -p wire_weaver_client --lib
    # a shared item's handler that takes &mut self doesn't compile (SRV-65): trybuild, as above
    just test-shared-ui
    # no f32, f64 or 128-bit code from ww_si's device operations where a firmware didn't opt into floats (TEST-33)
    just no-float-guard

# A server with ww_client_server built without `multi` and `streams`, as in a firmware, refusing the corpus's multi,
# credit sink and Ack requests as not supported (SRV-60); alone, since a workspace build turns the features on
test-lean-server:
    cargo nextest run -p lean_server --run-ignored only

# ww_ecc_flash's zeros and strict reads on a simulated flash, with each family's word size
[working-directory('mcu')]
test-ecc-flash:
    cargo test -q -p ww_ecc_flash --features stm32h7
    cargo test -q -p ww_ecc_flash --features stm32g0

# ww_wasm's module ABI and both interpreters running the example images in a mount slot (SRV-85)
[working-directory('ww_wasm')]
test-wasm:
    cargo test -q --features wasefire,wasmi

# Build the wasm module example (examples_mcu/wasm_module) into `out` (default: the B135B firmware's modules/):
# wasm_module_api and wasm_module_api_evolved, each with wasefire-interpreter's side table (SRV-85)
wasm-modules out="examples_mcu/usb_stm32h725ig/modules":
    cd examples_mcu/wasm_module && cargo build -q --release
    cd examples_mcu/wasm_module && cargo build -q --release --no-default-features --features evolved --target-dir target/evolved
    mkdir -p {{ out }}
    cd ww_wasm && cargo run -q --features wasefire --bin ww-wasm-prepare -- ../examples_mcu/wasm_module/target/wasm32-unknown-unknown/release/wasm_module.wasm {{ justfile_directory() }}/{{ out }}/calc.wasm
    cd ww_wasm && cargo run -q --features wasefire --bin ww-wasm-prepare -- ../examples_mcu/wasm_module/target/evolved/wasm32-unknown-unknown/release/wasm_module.wasm {{ justfile_directory() }}/{{ out }}/calc_evolved.wasm

# The committed module images are the ones examples_mcu/wasm_module builds now, byte for byte: `just wasm-modules`
# refreshes them
check-wasm-modules: (wasm-modules "target/wasm-modules")
    cmp target/wasm-modules/calc.wasm examples_mcu/usb_stm32h725ig/modules/calc.wasm
    cmp target/wasm-modules/calc_evolved.wasm examples_mcu/usb_stm32h725ig/modules/calc_evolved.wasm

# facade! and facade_line!: what they accept and their compile errors (CLT-64), with trybuild; TRYBUILD=overwrite to update
test-facade-ui:
    cargo test -q -p wire_weaver_client --test facade_ui -- --ignored

# a shared item (receiver_model) whose handler takes &mut self is a compile error (SRV-65), with trybuild;
# TRYBUILD=overwrite to update
test-shared-ui:
    cargo test -q -p shared_handlers --test shared_ui -- --ignored

# ww_lz4's blocks of a few streams decoded by C liblz4 (python-lz4 with the earlier bytes as dictionary)
test-lz4-c:
    cargo run -q -p ww_lz4 --example liblz4_vectors | uv run -q --script ww_lz4/tests/liblz4/check.py

# The conformance corpus through Python (TEST-23): wire_weaver_py of this checkout as the `wire_weaver` module
[working-directory('tests/conformance_py')]
test-conformance-py:
    uv run --group dev pytest -q tests

# Fill in the golden bytes of new conformance cases ("bytes": "?") from the derive and write docs/serdes/conformance_matrix.md (TEST-23)
conformance-update:
    CONFORMANCE=update cargo nextest run -p conformance shapes
    CONFORMANCE=update cargo nextest run -p conformance corpus
    CONFORMANCE=update cargo nextest run -p conformance report

# Fill in the golden bytes of new dispatch_golden cases ("request"/"reply"/an event "?") from the sync server (TEST-31)
dispatch-golden-update:
    DISPATCH_GOLDEN=update cargo nextest run -p dispatch_golden corpus

# Observe cells under miri: the ring's writer and reader on two threads over many interleavings (about a minute);
# shrink_wrap's SeriesCells round trips run under miri in its own repository (`just miri` there)
miri:
    cargo +nightly-2026-11-19 miri test -p ww_stream --features std --lib in_place
    MIRIFLAGS="-Zmiri-many-seeds=0..16" cargo +nightly-2026-11-19 miri test -p ww_stream --features std --lib cells_tests

# ww_settings power cuts at every step of every write and erase (release; `just test` samples them in debug)
test-power-cut:
    cargo nextest run -p ww_settings --release --profile power-cut --test power_cut

# Regenerate the docs site's Features page (docs/features_today.md) from FEATURES.md and tests/features_page/entries.md (DOC-9)
features-page:
    FEATURES_PAGE=update cargo nextest run -p features_page

# Regenerate the WireWeaver and ShrinkWrap logos in docs/assets (light, dark, 2x PNG; needs rsvg-convert)
logos:
    uv run docs/assets/src/logos.py

# What CI checks besides the tests: formatting, manifests, typos, clippy and rustdoc, warnings are errors, then spirit_level
lint:
    cargo fmt --all --check
    cargo sort -w -g --check
    typos
    # cargo's own warnings (manifests, lints tables) are errors too, as in CI (setup-rust-toolchain's build-warnings)
    CARGO_BUILD_WARNINGS=deny cargo clippy --workspace --all-targets -- -D warnings
    CARGO_BUILD_WARNINGS=deny cargo clippy --workspace --all-targets --all-features -- -D warnings
    CARGO_BUILD_WARNINGS=deny RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
    just lint-c
    just spl

# spirit_level's lints of spirit_level.toml, after the book pages are assembled so its link and nav lints see them
spl: book-assemble
    spl check --progress never

# Build the shrink_wrap C runtime and sw_requantize.c (also with SW_NO_F64) on their own, warnings are errors (SW-43, CLT-54); the generated fixed-layout and head+tail C readers are checked in the shrink_wrap repository
lint-c:
    cc -std=c99 -Wall -Wextra -Werror -pedantic -Wconversion -Wsign-conversion -c shrink_wrap_c/shrink_wrap.c -o /dev/null
    cc -std=c99 -Wall -Wextra -Werror -pedantic -Wconversion -Wsign-conversion -c shrink_wrap_c/sw_requantize.c -o /dev/null
    cc -std=c99 -Wall -Wextra -Werror -pedantic -Wconversion -Wsign-conversion -DSW_NO_F64 -c shrink_wrap_c/sw_requantize.c -o /dev/null

# cargo check everything
check: check-core check-mcu check-wasm check-examples-mcu

# cargo check repo root workspace
check-core:
    # Checking core
    @cargo check
    # check ww_device with features used on embedded targets
    @cargo check -p ww_device --features=defmt,embassy-time
    @cargo check -p ww_device --features=defmt,embassy-net,ws,udp

# cargo check mcu workspace
[working-directory('mcu')]
check-mcu:
    # Checking mcu
    @cargo check --workspace --exclude ww_ecc_flash
    # ww_ecc_flash needs its MCU family
    @cargo check -p ww_ecc_flash --features nmi-handler --target thumbv6m-none-eabi
    @cargo check -p ww_ecc_flash --features stm32h7 --target thumbv7em-none-eabihf

# cargo check and clippy the ww_wasm workspace (nightly: wasefire-interpreter) and the wasm module example
check-wasm:
    # Checking ww_wasm
    cd ww_wasm && CARGO_BUILD_WARNINGS=deny cargo clippy -q --all-targets --features guest,wasefire,wasmi -- -D warnings
    cd ww_wasm && CARGO_BUILD_WARNINGS=deny cargo clippy -q --lib --features wasefire,wasmi,defmt --target thumbv7em-none-eabihf -- -D warnings
    cd examples_mcu/wasm_module && CARGO_BUILD_WARNINGS=deny cargo clippy -q -- -D warnings
    just check-wasm-modules

check-examples-mcu:
    # Checking examples-mcu
    @just check-examples-mcu-qemu
    @just check-examples-mcu-qemu-m4f
    @just check-examples-mcu-nucleo-h743zi2
    @just check-examples-mcu-nucleo-g0b1re
    @just check-examples-mcu-stm32h725ig
    @just check-examples-mcu-rp2
    @just check-examples-mcu-ecc-flash-g0b1
    @just check-examples-mcu-ecc-flash-h7
    @just check-examples-mcu-size-bench-reply

[working-directory('examples_mcu/mcu_qemu_m4f')]
check-examples-mcu-qemu-m4f:
    # Checking mcu_qemu_m4f
    @cargo check

[working-directory('examples_mcu/mcu_qemu')]
check-examples-mcu-qemu:
    # Checking mcu_qemu
    @cargo check
    @cargo check --features flash-async
    @cargo check --features streams

[working-directory('examples_mcu/nucleo_h743zi2')]
check-examples-mcu-nucleo-h743zi2:
    # Checking nucleo_h743zi2
    @cargo check
    @cargo check --features settings

[working-directory('examples_mcu/nucleo_h743zi2')]
upload-examples-mcu-nucleo-h743zi2:
    mx3 fw upload --bin blinky --release --rename usb_nucleo_h743zi2_blinky

[working-directory('examples_mcu/nucleo_g0b1re')]
check-examples-mcu-nucleo-g0b1re:
    # Checking nucleo_g0b1re
    @cargo check --features usb
    @cargo check --no-default-features --features nucleo_g0b1re,rtt_target
    @cargo check --no-default-features --features nucleo_g0b1re,rtt_target,settings

[working-directory('examples_mcu/ecc_flash_g0b1')]
check-examples-mcu-ecc-flash-g0b1:
    # Checking ecc_flash_g0b1
    @cargo check --bins

[working-directory('examples_mcu/ecc_flash_h7')]
check-examples-mcu-ecc-flash-h7:
    # Checking ecc_flash_h7
    @cargo check

[working-directory('examples_mcu/size_bench_reply')]
check-examples-mcu-size-bench-reply:
    # Checking size_bench_reply (the runner, and the reply image on Cortex-M0)
    @cargo check
    cd fw && cargo check --features reply,finish_for --target thumbv6m-none-eabi

# Hardware check of ww_ecc_flash on an STM32H725IG: real ECC errors from a scratch flash word programmed twice, read
# in zeros and strict mode; replaces the board's firmware, erases 0x080E0000.. (probe: PROBE_RS_PROBE=VID:PID)
[working-directory('examples_mcu/ecc_flash_h7')]
hw-ecc-flash-h7:
    cargo run --release

# Flash the wasm slot firmware (SRV-85) to B135B with both interpreters, and print its log (probe: PROBE_RS_PROBE=VID:PID)
[working-directory('examples_mcu/usb_stm32h725ig')]
flash-wasm-slot:
    cargo run --release --features wasefire,wasmi --bin stm32h725ig-wasm-slot

# Hardware check of the wasm slot firmware on B135B over USB, flashed by flash-wasm-slot: calls, a reload the client
# relearns, a trap unloading the module, wasmi's fuel, and the numbers of docs/api/mounts.md#wasm-modules
hw-wasm-slot:
    cargo nextest run -p wasm_slot --run-ignored only --no-capture

[working-directory('examples_mcu/usb_stm32h725ig')]
check-examples-mcu-stm32h725ig:
    # Checking usb_stm32h725ig
    @cargo check
    @cargo check --features wasefire,wasmi --bin stm32h725ig-wasm-slot

[working-directory('examples_mcu/rp2')]
check-examples-mcu-rp2:
    # Checking rp2
    @cargo check
    @cargo check --features usb
    @cargo check --features ws_ncm
    @cargo check --features udp_ncm
    @cargo check --no-default-features --features rtt_target --bin rp2_ww_rtt

# No install or deploy recipe here: ww and the Python wheel are built and installed from vhrdtech/ww_tools
# (`cargo install --git https://github.com/vhrdtech/ww_tools ww`, or `just install` in that repository).
# Recipes below that need a ww for this repository's own work use wire_weaver_cli's example `ww_generic`.

# Save API snapshots of ww_tap, ww_timebase, ww_trace_context, ww_link, ww_client_server and the test fixtures other
# crates' tests read, into each crate's api_snapshots/, with wire_weaver_cli's generic `ww` (the ww_base and ww_stdlib
# crates save theirs in their own repositories). Pass --force to overwrite snapshots of versions that were never
# published.
save-snapshots *args:
    cargo build -q -p wire_weaver_cli --example ww_generic
    for crate in ww_tap ww_timebase ww_trace_context ww_link ww_client_server tests/wire_line_fixture_api tests/skipped_dep_api tests/global_fixture_api; do \
        target/debug/examples/ww_generic api save $crate {{ args }} || exit 1; \
    done

pre-commit:
    cargo sort -w -g
    cargo clippy

# Copy the pages and feature trackers of the repositories in book_sources.toml (shrink_wrap) into docs/, from the
# checkouts Cargo.lock pins; the copies are git-ignored (ww_book)
[group('docs')]
book-assemble:
    cargo run -q -p ww_book -- assemble

# Serve the documentation locally
[group('docs')]
serve-docs: book-assemble
    uv run --with zensical zensical serve

# Build the documentation
[group('docs')]
build-docs: book-assemble
    uv run --with zensical zensical build --clean

# Code size of a generated server's method reply on small CPUs (examples_mcu/size_bench_reply), through shrink_wrap's
# size bench runner: `just size-bench-reply --help`, `--fw-features finish_for` (TEST-17)
[working-directory('examples_mcu/size_bench_reply')]
size-bench-reply *args:
    cargo run -q --release -- {{ args }}

# Where a connect's time goes (CLT-79), step by step, empty cache then this PC's: in-process by default; a blinky board
# on USB with `--transport usb`, `--client blocking` or `dynamic`, `--clients 50`; `just connect-bench --help`
connect-bench *args:
    cargo run -q --release -p wire_weaver_client_benchmarks --bin connect_bench -- {{ args }}

# Units firmware budget (TEST-27): ww_si::Fixed against plain fields in instructions (QEMU M3), flash and stack, at the
# shipped firmware's profile; `--qemu-host HOST` runs QEMU there (this PC has none), `--cycles` adds the STM32H725 (DWT)
[working-directory('examples_mcu/fw_budget')]
units-budget *args:
    cargo run -q --release -- {{ args }}

# The ELF proof of the units budget (TEST-27): plain and Fixed code compared per function on M0, M4 and M33, `fw-z` and `fw-3`
[working-directory('examples_mcu/fw_budget')]
units-elf *args:
    cargo run -q --release -- --elf {{ args }}

# Flash budget of a server (TEST-30): the floor and bytes per resource of each kind on Cortex-M0+ at `fw-z` and `fw-3`,
# sync and `use_async`, plus checks for code a firmware shouldn't carry; `--targets T1,T2` for other cores
[working-directory('examples_mcu/fw_budget')]
flash-budget *args:
    cargo run -q --release -- --flash {{ args }}

# The no-float guard (TEST-33): a fixture firmware calling every device-relevant ww_si operation, built on Cortex-M0 and
# M4F at `fw-z` and `fw-3` with each number-feature set firmware uses; fails on code its group forbids
[working-directory('examples_mcu/fw_budget')]
no-float-guard *args:
    cargo run -q --release -- --guard {{ args }}

# Flash by inlined frame of a firmware ELF with debug info (TEST-32): crates, dispatcher functions, serdes impls; `--gen FILE` counts resources
[working-directory('examples_mcu/fw_budget')]
flash-attribute elf *args:
    cargo run -q --release -- --attribute {{ absolute_path(elf) }} {{ args }}

# Instructions per request of a server (TEST-30): one request of each kind through the dispatcher of the flash budget's
# `BaseOnly` server on a Cortex-M3 under QEMU at `fw-z` and `fw-3`, sync and `use_async`; `--qemu-host HOST` runs QEMU there,
# `--cycles [--probe-host HOST]` adds DWT cycles on the STM32H725 (replaces the board's firmware)
[working-directory('examples_mcu/fw_budget')]
dispatch-budget *args:
    cargo run -q --release -- --requests {{ args }}

# Zero-cost check (TEST-34): API bundle digests and firmware section hashes (every examples_mcu board, `fw-z` and
# `fw-3`) of BASE_DIR, a checkout the caller made, and of this tree, compared; `--boards B1,B2` or `none` builds fewer
[working-directory('examples_mcu/fw_budget')]
zero-cost base_dir *args:
    cargo run -q --release -- --snapshot ../../target/zero-cost/base --tree {{ absolute_path(base_dir) }} {{ args }}
    cargo run -q --release -- --snapshot ../../target/zero-cost/head {{ args }}
    cargo run -q --release -- --compare ../../target/zero-cost/base ../../target/zero-cost/head

# header text:
#     @printf "\033[34m\033[1m%s\033[0m\n" "{{ text }}"

# Install dependencies for fuzzing
deps-ext:
    cargo install cargo-fuzz

# Fuzz ww_framer
[working-directory('fuzz')]
fuzz-framer:
    cargo +nightly-2026-11-19 fuzz run framer-tx-rx -- -max_len=32768

# Fuzz the framer with a frame prefix over a lossy channel (UDP configuration)
[working-directory('fuzz')]
fuzz-framer-lossy:
    cargo +nightly-2026-11-19 fuzz run framer-lossy -- -max_len=32768

# Fuzz the link setup hello (EVO-23)
[working-directory('fuzz')]
fuzz-link-hello:
    cargo +nightly-2026-11-19 fuzz run link-hello -- -max_len=64

# ww_tape's fuzz targets, golden files and benchmarks live in vhrdtech/ww_tape, with their recipes in its justfile
