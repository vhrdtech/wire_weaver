# List all available targets
default:
    @just --list

# Test everything
test:
    cargo nextest run --workspace --no-fail-fast

# ww_settings power cuts at every step of every write and erase (release; `just test` samples them in debug)
test-power-cut:
    cargo nextest run -p ww_settings --release --profile power-cut --test power_cut

# Regenerate the ww_dfu scenario transcripts (tests/dfu_scenarios/transcripts, included by docs/std_library/ww_dfu_ab.md and ww_dfu_one_slot.md)
dfu-transcripts:
    DFU_TRANSCRIPTS=update cargo nextest run -p dfu_scenarios

# Regenerate the docs site's Features page (docs/features_today.md) from FEATURES.md and tests/features_page/entries.md (DOC-9)
features-page:
    FEATURES_PAGE=update cargo nextest run -p features_page

# Regenerate the WireWeaver and ShrinkWrap logos in docs/assets (light, dark, 2x PNG; needs rsvg-convert)
logos:
    uv run docs/assets/src/logos.py

# What CI checks besides the tests: formatting, manifests, typos, clippy and rustdoc, warnings are errors
lint:
    cargo fmt --all --check
    cargo sort -w -g --check
    typos
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
    just lint-c

# Build the shrink_wrap C runtime on its own, warnings are errors (SW-43); build and run the generated fixed-layout (SW-45) and head+tail (SW-46) C readers
lint-c:
    cc -std=c99 -Wall -Wextra -Werror -pedantic -Wconversion -Wsign-conversion -c shrink_wrap/c/shrink_wrap.c -o /dev/null
    mkdir -p target
    cc -std=c99 -Wall -Wextra -Werror -pedantic -Wconversion -Wsign-conversion -o target/fixed_layout_c shrink_wrap/shrink_wrap/tests/fixed_layout_c.c
    target/fixed_layout_c
    cc -std=c99 -Wall -Wextra -Werror -pedantic -Wconversion -Wsign-conversion -o target/head_tail_c shrink_wrap/shrink_wrap/tests/head_tail_c.c
    target/head_tail_c

# Build the Python module (wire_weaver_py) into its own venv and run its tests
[working-directory('wire_weaver_py')]
test-py:
    uv run --group dev pytest

# Python console with `ww` imported and the device connected as `dev` (rebuilds the module if needed), `just py --help`
[working-directory('wire_weaver_py')]
py *args:
    WW_CWD="{{invocation_directory()}}" uv run python -i scripts/console.py {{args}}

# cargo check everything
check: check-core check-mcu check-examples-mcu

# cargo check repo root workspace
check-core:
    # Checking core
    @cargo check
    # check ww_device with features used on embedded targets
    @cargo check -p ww_device --features=defmt,embassy-time
    @cargo check -p ww_device --features=defmt,embassy-net,ws,udp
    # check ww_si with no number type and with a few (NumericValue variants are features)
    @cargo check -p ww_si --no-default-features
    @cargo check -p ww_si --no-default-features --features=f32,u16,defmt

# cargo check mcu workspace
[working-directory('mcu')]
check-mcu:
    # Checking mcu
    @cargo check --workspace --exclude ww_ecc_flash
    # ww_ecc_flash needs its MCU family
    @cargo check -p ww_ecc_flash --features nmi-handler --target thumbv6m-none-eabi
    @cargo check -p ww_ecc_flash --features stm32h7 --target thumbv7em-none-eabihf

check-examples-mcu:
    # Checking examples-mcu
    @just check-examples-mcu-qemu
    @just check-examples-mcu-qemu-m4f
    @just check-examples-mcu-nucleo-h743zi2
    @just check-examples-mcu-nucleo-g0b1re
    @just check-examples-mcu-stm32h725ig
    @just check-examples-mcu-rp2
    @just check-examples-mcu-ecc-flash-g0b1

[working-directory('examples_mcu/mcu_qemu_m4f')]
check-examples-mcu-qemu-m4f:
    # Checking mcu_qemu_m4f
    @cargo check

[working-directory('examples_mcu/mcu_qemu')]
check-examples-mcu-qemu:
    # Checking mcu_qemu
    @cargo check

[working-directory('examples_mcu/nucleo_h743zi2')]
check-examples-mcu-nucleo-h743zi2:
    # Checking nucleo_h743zi2
    @cargo check
    @cargo check --features settings

[working-directory('examples_mcu/nucleo_h743zi2')]
upload-examples-mcu-nucleo-h743zi2:
    mx3 fw upload --bin blinky --release --rename usb_nucleo_h743zi2_blinky
    mx3 fw upload --bin all_gpio --release --rename usb_nucleo_h743zi2_all_gpio

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

[working-directory('examples_mcu/usb_stm32h725ig')]
check-examples-mcu-stm32h725ig:
    # Checking usb_stm32h725ig
    @cargo check

[working-directory('examples_mcu/rp2')]
check-examples-mcu-rp2:
    # Checking rp2
    @cargo check
    @cargo check --features usb
    @cargo check --features ws_ncm
    @cargo check --features udp_ncm
    @cargo check --no-default-features --features rtt_target --bin rp2_ww_rtt
    @cargo check --no-default-features --features usb_log --bin rp2_usb_log

[working-directory('examples_mcu/usb_stm32h725ig')]
upload-examples-mcu-usb-stm32h725ig:
    mx3 fw upload --bin uart --release --rename usb_b135_uart

# Build the ww CLI in release mode and copy it into ~/.local/bin for quick testing
install-cli:
    cargo build --release -p wire_weaver_cli
    install -Dm755 target/release/ww ~/.local/bin/ww
    @echo "Installed ww into ~/.local/bin"

# Save API snapshots of ww_global and ww_stdlib crates, and copy them into wire_weaver_snapshots to be embedded.
# Pass --force to overwrite snapshots of versions that were never published.
save-snapshots *args:
    cargo build -q -p wire_weaver_cli
    # ww_client_server is the protocol itself, not used in APIs
    for crate in ww_global ww_stdlib/*/; do \
        [ "$(basename $crate)" = ww_client_server ] || target/debug/ww api save $crate {{ args }} || exit 1; \
    done
    rm -rf wire_weaver_snapshots/api_snapshots
    mkdir -p wire_weaver_snapshots/api_snapshots
    cp ww_global/api_snapshots/*.ron ww_stdlib/*/api_snapshots/*.ron wire_weaver_snapshots/api_snapshots/

pre-commit:
    cargo sort -w -g
    cargo clippy

# Serve the documentation locally
[group('docs')]
serve-docs:
    uv run --with zensical zensical serve

# Build the documentation
[group('docs')]
build-docs:
    uv run --with zensical zensical build --clean

# shrink_wrap's code size on small CPUs (RV32I, RV32IC, Cortex-M0) per test case, `just size-bench --help` (TEST-17)
size-bench *args:
    cargo run -q --release -p size_bench -- {{ args }}

# header text:
#     @printf "\033[34m\033[1m%s\033[0m\n" "{{ text }}"

# Install dependencies for fuzzing
deps-ext:
    cargo install cargo-fuzz

# Fuzz ww_framer
[working-directory('fuzz')]
fuzz-framer:
    cargo +nightly fuzz run framer-tx-rx -- -max_len=32768

# Fuzz shrink_wrap TailSize types (SW-22)
[working-directory('fuzz')]
fuzz-shrink-wrap:
    cargo +nightly fuzz run shrink-wrap-tail-size -- -max_len=4096

# Fuzz shrink_wrap Delta/DeltaOfDelta/XorFloat series types (SW-31, SW-32)
[working-directory('fuzz')]
fuzz-shrink-wrap-series:
    cargo +nightly fuzz run shrink-wrap-series -- -max_len=4096

# Fuzz shrink_wrap RefVec of plain numbers against its decode pass (SW-37)
[working-directory('fuzz')]
fuzz-shrink-wrap-ref-vec:
    cargo +nightly fuzz run shrink-wrap-ref-vec -- -max_len=4096

# Fuzz shrink_wrap OptionAnyVec: random headers into the reader, round trips of any present set in every mode (SW-55)
[working-directory('fuzz')]
fuzz-shrink-wrap-option-any-vec:
    cargo +nightly fuzz run shrink-wrap-option-any-vec -- -max_len=4096

# Fuzz shrink_wrap's latched reads of U1..U64 / I2..I64 against reads with a Result (SW-38)
[working-directory('fuzz')]
fuzz-shrink-wrap-latched:
    cargo +nightly fuzz run shrink-wrap-latched -- -max_len=4096

# Fuzz Series<T> (SW-60): round trips of generated items, random bytes into the readers
[working-directory('fuzz')]
fuzz-shrink-wrap-series-type:
    cargo +nightly fuzz run shrink-wrap-series-type -- -max_len=4096

# Fuzz decoding recursive types at random nesting limits: bounded by the limit, never a stack overflow
[working-directory('fuzz')]
fuzz-shrink-wrap-nesting:
    cargo +nightly fuzz run shrink-wrap-nesting -- -max_len=4096
