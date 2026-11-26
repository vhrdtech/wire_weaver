"""blinky.py against the simulated blinky device: what it prints is what the book shows, in
docs/generated/blinky/blinky_py.txt. A stale file fails; WW_BOOK_OUTPUTS=write (`just book-outputs`)
writes it again.
"""

import contextlib
import io
import os
import subprocess
import sys
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parents[1]
ROOT = HERE.parents[1]
OUTPUT = ROOT / "docs" / "generated" / "blinky" / "blinky_py.txt"

sys.path.insert(0, str(HERE))
import blinky  # noqa: E402


@pytest.fixture
def sim_url():
    """A simulated blinky device on a free port, stopped after the test."""
    sim = subprocess.Popen(
        ["cargo", "run", "-q", "-p", "blinky", "--example", "blinky_sim", "--", "--port", "0"],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        text=True,
    )
    try:
        line = sim.stdout.readline()
        assert line.startswith("blinky at "), line
        yield line.removeprefix("blinky at ").strip()
    finally:
        sim.terminate()
        sim.wait(timeout=10)


def test_blinky_prints_what_the_book_shows(sim_url):
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        blinky.main(sim_url)
    text = "$ python blinky.py\n" + out.getvalue()
    if os.environ.get("WW_BOOK_OUTPUTS") == "write":
        OUTPUT.parent.mkdir(parents=True, exist_ok=True)
        OUTPUT.write_text(text)
    assert OUTPUT.read_text() == text, f"{OUTPUT} is stale: run `just book-outputs`"
