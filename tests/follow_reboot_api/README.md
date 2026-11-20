# follow_reboot_api

The lean app API of `tests/follow_reboot` (CLT-21, CLT-34): a node that reports the app image's version and reboots between the app and its updater. Stands in for `ww_dfu::UpdateNode` + `ww_dfu::BootControl` without depending on `ww_dfu` (R7).
