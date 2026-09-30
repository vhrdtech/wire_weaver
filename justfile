# List all available targets
default:
    @just --list

# Test everything
test:
    cargo nextest run --workspace --no-fail-fast

# cargo check everything
check: check-core check-mcu check-examples-mcu

# cargo check repo root workspace
check-core:
    # Checking core
    @cargo check
    # check ww_device with features used on embedded targets
    @cargo check -p ww_device --features=defmt,embassy-time

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
    @cargo check

[working-directory('examples_mcu/usb_stm32h725ig')]
check-examples-mcu-stm32h725ig:
    # Checking usb_stm32h725ig
    @cargo check

[working-directory('examples_mcu/usb_stm32h725ig')]
upload-examples-mcu-usb-stm32h725ig:
    mx3 fw upload --bin uart --release --rename usb_b135_uart

# Build the ww CLI in release mode and copy it into ~/.local/bin for quick testing
install-cli:
    cargo build --release -p ww
    install -Dm755 target/release/ww ~/.local/bin/ww
    @echo "Installed ww into ~/.local/bin"

pre-commit:
    cargo sort -w
    cargo clippy

# Serve the documentation localy
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
