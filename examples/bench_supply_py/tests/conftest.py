"""The simulated bench supply for the scripts' tests, and the check of what a script prints against the book's copy
in docs/generated/bench_supply/. A stale file fails; WW_BOOK_OUTPUTS=write (`just book-outputs`) writes it again.
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
GENERATED = ROOT / "docs" / "generated" / "bench_supply"

sys.path.insert(0, str(HERE))


@pytest.fixture(scope="session")
def sim_exe():
    """The simulated bench supply's executable, built once per test session."""
    build = subprocess.run(
        ["cargo", "build", "-q", "-p", "bench_supply", "--example", "bench_supply_sim", "--message-format=json"],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        text=True,
        check=True,
    )
    for line in build.stdout.splitlines():
        msg = json.loads(line)
        if msg.get("reason") == "compiler-artifact" and msg["target"]["name"] == "bench_supply_sim":
            return msg["executable"]
    raise AssertionError("cargo built no bench_supply_sim")


@pytest.fixture
def sim_url(sim_exe):
    """A simulated bench supply on a free port, stopped after the test: each test starts from a fresh device."""
    sim = subprocess.Popen([sim_exe, "--port", "0"], stdout=subprocess.PIPE, text=True)
    try:
        line = sim.stdout.readline()
        assert line.startswith("bench supply at "), line
        yield line.removeprefix("bench supply at ").strip()
    finally:
        sim.terminate()
        sim.wait(timeout=10)


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
