# just cheatsheet https://cheatography.com/linux-china/cheat-sheets/justfile/

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
    @just header "Checking core"
    @cargo check
    # check wire_weaver_usb_link with actual features to be used
    @cargo check -p wire_weaver_usb_link --features=device,host,defmt

# cargo check mcu workspace
[working-directory('mcu')]
check-mcu:
    @just header "Checking mcu"
    @cargo check

check-examples-mcu:
    @just header "Checking examples-mcu"
    @just check-examples-mcu-qemu
    @just check-examples-mcu-nucleo-h743zi2
    @just check-examples-mcu-stm32g0b1cetxn
    @just check-examples-mcu-stm32h725ig

[working-directory('examples_mcu/mcu_qemu')]
check-examples-mcu-qemu:
    @just header "Checking mcu_qemu"
    @cargo check

[working-directory('examples_mcu/nucleo_h743zi2')]
check-examples-mcu-nucleo-h743zi2:
    @just header "Checking usb_nucleo_h743zi2"
    @cargo check

[working-directory('examples_mcu/nucleo_h743zi2')]
upload-examples-mcu-nucleo-h743zi2:
    mx3 fw upload --bin blinky --release --rename usb_nucleo_h743zi2_blinky
    mx3 fw upload --bin all_gpio --release --rename usb_nucleo_h743zi2_all_gpio

[working-directory('examples_mcu/nucleo_g0b1re')]
check-examples-mcu-usb-stm32g0b1cetxn:
    @just header "Checking usb_stm32g0b1cetxn"
    @cargo check

[working-directory('examples_mcu/usb_stm32h725ig')]
check-examples-mcu-usb-stm32h725ig:
    @just header "Checking usb_stm32h725ig"
    @cargo check

[working-directory('examples_mcu/usb_stm32h725ig')]
upload-examples-mcu-usb-stm32h725ig:
    mx3 fw upload --bin uart --release --rename usb_b135_uart

pre-commit:
    cargo sort -w

# Serve the documentation localy
[group('docs')]
serve-docs:
    uv run --with zensical zensical serve

# Build the documentation
[group('docs')]
build-docs:
    uv run --with zensical zensical build --clean

header text:
    @printf "\033[34m\033[1m%s\033[0m\n" "{{ text }}"
