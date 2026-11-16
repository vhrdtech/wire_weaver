"""Keyed arrays from Python (CLT-72): `dev.flow[key]` finds a flow by its key, whatever index the device gave it.
`tests/hub_pytest_device`'s `keyed_pytest_device` hosts the hub and `tests/keyed`'s Dissector (`inproc:keyed-device`),
which met `FLOWS` flows in shuffled order and removed the first `REMOVED` of them again.
"""

import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).parents[3]
DEVICE_KEY = "inproc:keyed-device"
READY_TIMEOUT = 10.0
# tests/keyed/src/device.rs: `FLOWS`, `REMOVED`, `flow(i)`
FLOWS = 12
REMOVED = 3


def _device_binary() -> Path:
    """Builds `keyed_pytest_device` if needed and returns its path."""
    subprocess.run(
        ["cargo", "build", "-q", "-p", "hub_pytest_device", "--bin", "keyed_pytest_device"],
        cwd=ROOT,
        check=True,
    )
    name = "keyed_pytest_device.exe" if sys.platform == "win32" else "keyed_pytest_device"
    exe = ROOT / "target" / "debug" / name
    assert exe.exists(), f"{exe} missing after cargo build"
    return exe


def _wait_for_socket(runtime_dir: Path, deadline: float) -> None:
    sock = runtime_dir / "ww" / "hub.sock"
    if sys.platform == "win32":
        time.sleep(0.3)
        return
    while time.monotonic() < deadline:
        if sock.exists():
            return
        time.sleep(0.02)
    raise TimeoutError(f"{sock} did not appear in time")


@pytest.fixture
def dev(monkeypatch):
    """The Dissector through a private hub, connected; hub and device torn down after the test."""
    import wire_weaver as ww

    exe = _device_binary()
    runtime_dir = Path(tempfile.mkdtemp(prefix="ww-pytest-keyed-"))
    monkeypatch.setenv("WW_RUNTIME_DIR", str(runtime_dir))
    log = (runtime_dir / "keyed_pytest_device.log").open("w")
    proc = subprocess.Popen(exe, env=dict(os.environ), stdout=log, stderr=log)
    try:
        _wait_for_socket(runtime_dir, time.monotonic() + READY_TIMEOUT)
        device = ww.connect(hub_device=DEVICE_KEY)
        yield device
        device.disconnect()
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=5)
        log.close()
        shutil.rmtree(runtime_dir, ignore_errors=True)


def _key(i: int) -> dict:
    """`flow(i)`'s key, as `tests/keyed/src/device.rs` builds it."""
    port = 6000 + i
    return {
        "proto": "Tcp" if port % 2 == 0 else "Udp",
        "src": {"addr": [10, 0, 0, 1], "port": port},
        "dst": {"addr": [10, 0, 0, 2], "port": 443},
        "first_seen": {"coord": 100 + i},
    }


def test_flows_by_key(dev):
    assert dev.flow.key_type == "FlowKey"
    for i in range(REMOVED, FLOWS):
        flow = dev.flow[_key(i)]
        assert int(flow.bytes.read().value) == 1000 * i + 7, f"flow {i}"
        # the second lookup takes the index from the key cache, the same flow
        assert int(dev.flow.by(_key(i)).bytes.read().value) == 1000 * i + 7


def test_removed_flows_are_not_found(dev):
    for i in range(REMOVED):
        assert dev.flow.by(_key(i)) is None
        with pytest.raises(KeyError):
            dev.flow[_key(i)]


def test_a_wrong_key_is_refused_before_asking(dev):
    with pytest.raises(ValueError):
        dev.flow[{"proto": "Sctp"}]
