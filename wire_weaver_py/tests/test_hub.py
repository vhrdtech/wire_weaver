"""The ww hub from Python (TOOL-18): a kernel restart reconnects in ms with the device still open and steered by
the hub, and two Python processes share one device through the hub's tunnel (SRV-29) — no real hardware needed,
`tests/hub_pytest_device` hosts the hub and an in-process `counter` property in one binary (`inproc:pytest-device`).
"""

import concurrent.futures
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).parents[2]
DEVICE_KEY = "inproc:pytest-device"
READY_TIMEOUT = 10.0


def _hub_binary() -> Path:
    """Builds `hub_pytest_device` if needed and returns its path (`cargo build` is a no-op once it's current)."""
    subprocess.run(
        ["cargo", "build", "-q", "-p", "hub_pytest_device"],
        cwd=ROOT,
        check=True,
    )
    exe = ROOT / "target" / "debug" / ("hub_pytest_device.exe" if sys.platform == "win32" else "hub_pytest_device")
    assert exe.exists(), f"{exe} missing after cargo build"
    return exe


def _wait_for_socket(runtime_dir: Path, deadline: float) -> None:
    sock = runtime_dir / "ww" / "hub.sock"
    if sys.platform == "win32":
        # a named pipe, not a file: give the hub the same time to come up without checking its path
        time.sleep(0.3)
        return
    while time.monotonic() < deadline:
        if sock.exists():
            return
        time.sleep(0.02)
    raise TimeoutError(f"{sock} did not appear in time")


@pytest.fixture
def hub_device():
    """A private ww hub (TOOL-18's `hub_pytest_device`) holding `inproc:pytest-device`, torn down after the test.
    Yields the environment every client process (this one or a subprocess) needs: `WW_RUNTIME_DIR` set so they all
    find the same hub.
    """
    exe = _hub_binary()
    runtime_dir = Path(tempfile.mkdtemp(prefix="ww-pytest-hub-"))
    env = dict(os.environ, WW_RUNTIME_DIR=str(runtime_dir))
    log = (runtime_dir / "hub_pytest_device.log").open("w")
    proc = subprocess.Popen(exe, env=env, stdout=log, stderr=log)
    try:
        _wait_for_socket(runtime_dir, time.monotonic() + READY_TIMEOUT)
        yield env
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=5)
        log.close()
        shutil.rmtree(runtime_dir, ignore_errors=True)


def _client_script(action: str) -> str:
    """A tiny program run as a separate Python process (a notebook kernel, for the restart test): connects through
    the hub by device key and does `action`, `set:<n>` or `get`, printing the counter's value.
    """
    return (
        "import wire_weaver as ww\n"
        f"dev = ww.connect(hub_device={DEVICE_KEY!r})\n"
        f"action = {action!r}\n"
        "if action.startswith('set:'):\n"
        "    dev.counter.write(int(action[4:]))\n"
        "print(dev.counter.read())\n"
        "dev.disconnect()\n"
    )


def _run_client(env: dict, action: str) -> tuple[int, float]:
    """Runs a client subprocess, its printed counter value and how long it took end to end (connect included)."""
    start = time.monotonic()
    out = subprocess.run(
        [sys.executable, "-c", _client_script(action)],
        cwd=Path(__file__).parent,
        env=env,
        capture_output=True,
        text=True,
        timeout=20,
    )
    elapsed = time.monotonic() - start
    assert out.returncode == 0, f"stdout:\n{out.stdout}\nstderr:\n{out.stderr}"
    return int(out.stdout.strip()), elapsed


def test_kernel_restart_reconnects_fast_with_the_device_still_open(hub_device):
    """A notebook kernel is just a Python process: killing and restarting it (simulated by two subprocesses, one
    after the other) must not lose the hub's hold on the device, and the second process's `connect()` — a fresh
    TCP^Wsocket, link setup and introspection fetch, not a cold USB enumeration — should be fast.
    """
    value, _ = _run_client(hub_device, "set:4711")
    assert value == 4711

    # the "restarted kernel": a brand new process, same device key, through the same (still running) hub
    value, elapsed = _run_client(hub_device, "get")
    assert value == 4711, "the hub kept the device open across the restart"
    assert elapsed < 5.0, f"reconnect took {elapsed:.3f}s, expected a local-socket round trip, not a cold USB one"


def test_two_python_processes_share_one_device(hub_device):
    """Two independent Python processes, through the hub's tunnel (SRV-29): one writes, the other reads the same
    value, neither ever touching the device directly.
    """
    written, _ = _run_client(hub_device, "set:99")
    assert written == 99
    seen, _ = _run_client(hub_device, "get")
    assert seen == 99

    # concurrently, not just one after the other
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        a = pool.submit(_run_client, hub_device, "set:7")
        b = pool.submit(_run_client, hub_device, "get")
        a.result()
        b.result()
    final, _ = _run_client(hub_device, "get")
    assert final == 7


def test_hub_registry_lists_the_device_and_both_clients_after_connecting(hub_device, monkeypatch):
    import wire_weaver as ww

    monkeypatch.setenv("WW_RUNTIME_DIR", hub_device["WW_RUNTIME_DIR"])
    dev = ww.connect(hub_device=DEVICE_KEY)
    h = ww.hub()
    devices = h.registry.devices()
    assert any(d["key"] == DEVICE_KEY for d in devices), devices
    clients = h.registry.clients()
    assert len(clients) == 2, clients
    h.disconnect()
    dev.disconnect()
