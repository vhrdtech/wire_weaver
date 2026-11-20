# hub_pytest_device

Hosts the ww hub and in-process test devices in one process for `wire_weaver_py`'s pytest tests, no real hardware needed (TOOL-18, SRV-29). Two binaries: `hub_pytest_device` (the `counter` property device) and `keyed_pytest_device` (hosts `tests/keyed`'s Dissector, CLT-72).
