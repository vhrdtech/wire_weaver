"""The bench supply example's pytest bench (EX-19): drives the simulated device like a lab test script would,
through the ww hub, with no real hardware (TOOL-18, SRV-29, same pattern as `tests/hub_pytest_device`).

`bench_supply_hub_device` (`examples/bench_supply/src/bin/hub_device.rs`) hosts the ww hub and an in-process
bench supply in one binary; this file's `hub_device` fixture starts it with a private `WW_RUNTIME_DIR`, so every
test connects with `ww.connect(hub_device=DEVICE_KEY)` and never starts a hub of its own.
"""

import os
import subprocess
import time
from pathlib import Path

import pytest
import wire_weaver as ww

DEVICE_KEY = "inproc:bench-supply"
# the repository root: this checkout's `cargo run` builds and runs the device binary
ROOT = Path(__file__).resolve().parents[3]


# --8<-- [start:hub_device]
@pytest.fixture(scope="module")
def hub_device(tmp_path_factory):
    runtime_dir = tmp_path_factory.mktemp("ww-runtime")
    env = dict(os.environ, WW_RUNTIME_DIR=str(runtime_dir))
    process = subprocess.Popen(
        ["cargo", "run", "-q", "-p", "bench_supply", "--bin", "bench_supply_hub_device"],
        cwd=ROOT,
        env=env,
    )
    socket = runtime_dir / "ww" / "hub.sock"
    deadline = time.monotonic() + 60
    while not socket.exists():
        if process.poll() is not None:
            raise RuntimeError(f"bench_supply_hub_device exited with {process.returncode}")
        if time.monotonic() > deadline:
            process.kill()
            raise TimeoutError("bench_supply_hub_device did not open its socket in time")
        time.sleep(0.05)
    os.environ["WW_RUNTIME_DIR"] = str(runtime_dir)
    try:
        yield
    finally:
        del os.environ["WW_RUNTIME_DIR"]
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
# --8<-- [end:hub_device]


# --8<-- [start:dev]
@pytest.fixture
def dev(hub_device):
    with ww.connect(hub_device=DEVICE_KEY) as device:
        # a clean channel per test: the device process is shared across the module's tests
        device.channel[0].set_output(False)
        device.channel[0].voltage_limit.write(0)
        device.channel[0].current_limit.write(0)
        yield device
# --8<-- [end:dev]


# --8<-- [start:no_voltage]
def test_output_refuses_with_no_voltage_set(dev):
    with pytest.raises(ww.RemoteError) as excinfo:
        dev.channel[0].set_output(True)
    assert excinfo.value.value == "NoVoltageSet"
    assert not dev.channel[0].output_on.read()
# --8<-- [end:no_voltage]


def test_output_refuses_over_current_and_says_how_much(dev):
    # channel 0's load is 10 Ω: at 30 V it would draw 3 A
    dev.channel[0].voltage_limit.write(30_000)
    dev.channel[0].current_limit.write(1_000)
    with pytest.raises(ww.RemoteError) as excinfo:
        dev.channel[0].set_output(True)
    would_draw = excinfo.value.value["OverCurrent"]["would_draw"]
    assert would_draw.value == 3_000
    assert not dev.channel[0].output_on.read()


# --8<-- [start:measures]
def test_output_turns_on_within_limits_and_measures(dev):
    dev.channel[0].voltage_limit.write(5_000)
    dev.channel[0].current_limit.write(1_000)
    dev.channel[0].set_output(True)
    assert dev.channel[0].output_on.read()
    measured = dev.channel[0].measured.read()
    assert measured["voltage"].value == 5_000
    # 5 V over the channel's 10 Ω load is 500 mA
    assert measured["current"].value == 500

    dev.channel[0].set_output(False)
    assert not dev.channel[0].output_on.read()
    measured = dev.channel[0].measured.read()
    assert measured["voltage"].value == 0
    assert measured["current"].value == 0
# --8<-- [end:measures]


# --8<-- [start:wrong_state]
def test_limits_refuse_a_change_while_the_output_is_on(dev):
    dev.channel[0].voltage_limit.write(5_000)
    dev.channel[0].current_limit.write(1_000)
    dev.channel[0].set_output(True)
    with pytest.raises(ww.RemoteError) as excinfo:
        dev.channel[0].voltage_limit.write(6_000)
    assert "WrongState" in excinfo.value.common
    assert dev.channel[0].voltage_limit.read().value == 5_000
# --8<-- [end:wrong_state]


def test_channels_are_independent(dev):
    dev.channel[0].voltage_limit.write(5_000)
    dev.channel[0].current_limit.write(1_000)
    dev.channel[0].set_output(True)

    assert dev.channel[0].output_on.read()
    assert not dev.channel[1].output_on.read()
    assert dev.channel[1].voltage_limit.read().value == 0
