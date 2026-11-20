# flash_budget_api

API roots of the flash budget of a server (TEST-30): a `Floor` of one method, and `Base`/`BaseOnly` with resources of each kind. Each root is its own server bin in `examples_mcu/mcu_qemu` (`qemu-flash-*`); `just flash-budget` measures the difference between their sizes.
