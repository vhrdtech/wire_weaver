# List all available targets
default:
    @just --list

# Test everything
test:
    cargo nextest run --workspace --no-fail-fast

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

# cargo check mcu workspace
[working-directory('mcu')]
check-mcu:
    # Checking mcu
    @cargo check

check-examples-mcu:
    # Checking examples-mcu
    @just check-examples-mcu-qemu
    @just check-examples-mcu-nucleo-h743zi2
    @just check-examples-mcu-nucleo-g0b1re
    @just check-examples-mcu-stm32h725ig
    @just check-examples-mcu-rp2

[working-directory('examples_mcu/mcu_qemu')]
check-examples-mcu-qemu:
    # Checking mcu_qemu
    @cargo check

[working-directory('examples_mcu/nucleo_h743zi2')]
check-examples-mcu-nucleo-h743zi2:
    # Checking nucleo_h743zi2
    @cargo check

[working-directory('examples_mcu/nucleo_h743zi2')]
upload-examples-mcu-nucleo-h743zi2:
    mx3 fw upload --bin blinky --release --rename usb_nucleo_h743zi2_blinky
    mx3 fw upload --bin all_gpio --release --rename usb_nucleo_h743zi2_all_gpio

[working-directory('examples_mcu/nucleo_g0b1re')]
check-examples-mcu-nucleo-g0b1re:
    # Checking nucleo_g0b1re
    @cargo check --features usb
    @cargo check --no-default-features --features nucleo_g0b1re,rtt_target

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
