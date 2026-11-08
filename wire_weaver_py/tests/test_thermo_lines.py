"""One script against three wire lines of an API (EVO-28): the `thermo_api` bench's scenario from Python, with no
generated code, against simulated firmware of line 1 (w1.2), line 2 (w2.1) and line 3 (w3.0), each a wire break of
the one before. `tests/thermo_devices`' `thermo_pytest_devices` hosts the ww hub and the three devices in one process
(`inproc:thermo-w1`, `-w2`, `-w3`), as `test_hub.py`'s `hub_pytest_device` does.

Values in another scale or storage need no per-line code: a `ww.Quantity` is read whatever the step, and one made
from text (`ww.Quantity("0.25 K")`) is written exactly into each line's type, or refused before it is sent. What moved
or changed shape is looked up by what the device has (`configure` or `set_rate`, `uptime` at the root or in `health`,
a `Sample` or a `Batch` on the stream), never by its line number.
"""

import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import pytest

import wire_weaver as ww

ROOT = Path(__file__).parents[2]
READY_TIMEOUT = 10.0
LINES = {1: (1, 2), 2: (2, 1), 3: (3, 0)}
BURST = 3  # samples per trigger on line 3, thermo_devices::w3::BURST


def _devices_binary() -> Path:
    subprocess.run(
        ["cargo", "build", "-q", "-p", "thermo_devices", "--bin", "thermo_pytest_devices"],
        cwd=ROOT,
        check=True,
    )
    name = "thermo_pytest_devices.exe" if sys.platform == "win32" else "thermo_pytest_devices"
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


@pytest.fixture(scope="module")
def thermo_hub():
    """A private ww hub holding a device of each line, torn down after the module's tests."""
    exe = _devices_binary()
    runtime_dir = Path(tempfile.mkdtemp(prefix="ww-pytest-thermo-"))
    env = dict(os.environ, WW_RUNTIME_DIR=str(runtime_dir))
    log = (runtime_dir / "thermo_pytest_devices.log").open("w")
    proc = subprocess.Popen(exe, env=env, stdout=log, stderr=log)
    old = os.environ.get("WW_RUNTIME_DIR")
    os.environ["WW_RUNTIME_DIR"] = str(runtime_dir)
    try:
        _wait_for_socket(runtime_dir, time.monotonic() + READY_TIMEOUT)
        yield
    finally:
        if old is None:
            os.environ.pop("WW_RUNTIME_DIR", None)
        else:
            os.environ["WW_RUNTIME_DIR"] = old
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=5)
        log.close()
        shutil.rmtree(runtime_dir, ignore_errors=True)


def find(dev, *paths):
    """The first of `paths` (`a.b` for a sub-level) the device has: where an item is on its line."""
    for path in paths:
        node = dev
        try:
            for name in path.split("."):
                node = getattr(node, name)
            return node
        except AttributeError:
            continue
    return None


def kelvin(temp) -> float:
    """A `Point<Celsius, Fixed<Kelvin, ..>>` read as its one field, the `Fixed` in K from the origin."""
    q = temp["coord"]
    assert q.unit == "Kelvin"
    return float(q)


def samples(item) -> list:
    """A stream item as `(seq, kelvin)` pairs: a `Sample`, or a `Batch` of temperatures numbered from `first`."""
    if "temps" in item:
        return [(item["first"]["seq"] + i, kelvin(t)) for i, t in enumerate(item["temps"])]
    return [(item["seq"]["seq"], kelvin(item["temp"]))]


@pytest.mark.parametrize("line", [1, 2, 3])
def test_one_script_every_line(thermo_hub, line):
    with ww.connect(hub_device=f"inproc:thermo-w{line}", timeout=5.0) as dev:
        assert dev.wire == LINES[line], dev.info

        # configure(Config) where the device has it, else set_rate and the mode; a rate is the same type on every line
        configure = find(dev, "configure")
        if configure is not None:
            configure({"rate": 50, "mode": "Continuous"})
        else:
            dev.set_rate(50)
            dev.mode.write("Continuous")
        assert dev.mode.read() == "Continuous"

        # the same temperature from every line: 0.01 K in an i16 or 0.001 K in an i32, a Quantity either way
        assert kelvin(dev.read_once()["temp"]) == pytest.approx(21.5)

        # whole steps on line 1, hundredths on lines 2 and 3: 1.5 is refused before sending where it isn't exact
        dev.gain.write(ww.Quantity("2", "Ratio"))
        assert float(dev.gain.read()) == 2.0
        if line == 1:
            with pytest.raises(ValueError, match="Fixed"):
                dev.gain.write(ww.Quantity("1.5", "Ratio"))
        else:
            dev.gain.write(ww.Quantity("1.5", "Ratio"))
            assert float(dev.gain.read()) == 1.5

        # a stream up and a sink down; Mode::Triggered only where the enum has it (line 1)
        with dev.samples as stream, dev.commands as sink:
            if line == 1:
                dev.mode.write("Triggered")
            else:
                with pytest.raises((TypeError, ValueError)):
                    dev.mode.write("Triggered")
            sink.send({"Mark": {"seq": 100}})
            sink.send("Trigger")
            got = samples(stream.recv(timeout=5))
            assert got[0] == (100, pytest.approx(21.5))
            assert len(got) == (BURST if line == 3 else 1)

        # an array of sub-levels
        dev.channel[2].enabled.write(False)
        assert dev.channel[2].enabled.read() is False
        assert dev.channel[1].enabled.read() is True

        # an argument in another resolution: 0.25 K to every line, 0.001 K only where the type takes it
        dev.calibrate(ww.Quantity("0.25 K"))
        assert kelvin(dev.read_once()["temp"]) == pytest.approx(21.75)
        if line == 1:
            with pytest.raises(ValueError, match="Fixed"):
                dev.calibrate(ww.Quantity("1 mK"))
        else:
            dev.calibrate(ww.Quantity("1 mK"))
            assert kelvin(dev.read_once()["temp"]) == pytest.approx(21.501)

        # uptime at the root or in `health`, whole seconds or ms in a u64: 42 s on every line
        uptime = find(dev, "uptime", "health.uptime")
        assert uptime.read().to_si() == 42.0

        # a method where the device has it, at the root or in `health`
        self_test = find(dev, "self_test", "health.self_test")
        assert (self_test is None) == (line == 1)
        if self_test is not None:
            assert self_test() == ()  # Ok(()); an Err would raise

        # a ww_stdlib trait, the same path and types on every line
        dev.indication.mode.write("OnlyEssential")
        assert dev.indication.mode.read() == "OnlyEssential"
