# follow_reboot

Tests CLT-21 end to end without hardware: a simulated device whose app reboots into the boot ROM, comes back as a BOOTSEL device under the same serial number, and takes a new image over PICOBOOT. Its API is `follow_reboot_api`; see `docs/transport/follow_reboot.md`.
