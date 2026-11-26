"""The simulated thermometer for the scripts' tests, and the check of what a script prints against the book's copy in
docs/generated/thermometer/. A stale file fails; WW_BOOK_OUTPUTS=write (`just book-outputs`) writes it again.
"""

import contextlib
import io
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parents[1]
ROOT = HERE.parents[1]
GENERATED = ROOT / "docs" / "generated" / "thermometer"

sys.path.insert(0, str(HERE))


@pytest.fixture(scope="session")
def sim_exe():
    """The simulated thermometer's executable, built once per test session."""
    build = subprocess.run(
        ["cargo", "build", "-q", "-p", "thermometer", "--example", "thermometer_sim", "--message-format=json"],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        text=True,
        check=True,
    )
    for line in build.stdout.splitlines():
        msg = json.loads(line)
        if msg.get("reason") == "compiler-artifact" and msg["target"]["name"] == "thermometer_sim":
            return msg["executable"]
    raise AssertionError("cargo built no thermometer_sim")


def _serve(exe, *flags):
    """Starts a simulated thermometer on a free port with `flags` and yields its URL; stops it afterwards."""
    sim = subprocess.Popen([exe, "--port", "0", *flags], stdout=subprocess.PIPE, text=True)
    try:
        line = sim.stdout.readline()
        assert line.startswith("thermometer at "), line
        yield line.removeprefix("thermometer at ").strip()
    finally:
        sim.terminate()
        sim.wait(timeout=10)


@pytest.fixture
def sim_url(sim_exe):
    """A simulated thermometer on a free port, stopped after the test: each test starts from a fresh device."""
    yield from _serve(sim_exe)


@pytest.fixture
def still_sim_url(sim_exe):
    """A simulated thermometer in a still room (21.50 °C), for scripts that print temperatures."""
    yield from _serve(sim_exe, "--still")


def check_output(script, main, url, output):
    """Runs `main(url)` and checks what it prints, after a `$ python <script>` line, against `output`."""
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        main(url)
    text = f"$ python {script}\n" + out.getvalue()
    path = GENERATED / output
    if os.environ.get("WW_BOOK_OUTPUTS") == "write":
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    assert path.read_text() == text, f"{path} is stale: run `just book-outputs`"
