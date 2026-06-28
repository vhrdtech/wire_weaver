# just cheatsheet https://cheatography.com/linux-china/cheat-sheets/justfile/

# List all available targets
default:
    @just --list

# cargo check everything
check: check-core check-mcu check-examples-mcu

# cargo check repo root workspace
check-core:
    @just header "Checking core"
    @cargo check

# cargo check mcu workspace
[working-directory('mcu')]
check-mcu:
    @just header "Checking mcu"
    @cargo check

check-examples-mcu:
    @just header "Checking examples-mcu"
    @just check-examples-mcu-qemu
    @just check-examples-mcu-usb-nucleo-h743zi2
    @just check-examples-mcu-usb-stm32g0b1cetxn
    @just check-examples-mcu-usb-stm32h725ig

[working-directory('examples_mcu/mcu_qemu')]
check-examples-mcu-qemu:
    @just header "Checking mcu_qemu"
    @cargo check

[working-directory('examples_mcu/usb_nucleo_h743zi2')]
check-examples-mcu-usb-nucleo-h743zi2:
    @just header "Checking usb_nucleo_h743zi2"
    @cargo check

[working-directory('examples_mcu/usb_stm32g0b1cetxn')]
check-examples-mcu-usb-stm32g0b1cetxn:
    @just header "Checking usb_stm32g0b1cetxn"
    @cargo check

[working-directory('examples_mcu/usb_stm32h725ig')]
check-examples-mcu-usb-stm32h725ig:
    @just header "Checking usb_stm32h725ig"
    @cargo check

# Serve the documentation localy
[group('docs')]
serve-docs:
    @uv run mkdocs serve

# Build the documentation
[group('docs')]
build-docs:
    @uv run mkdocs build

header text:
    @printf "\033[34m\033[1m%s\033[0m\n" "{{ text }}"
