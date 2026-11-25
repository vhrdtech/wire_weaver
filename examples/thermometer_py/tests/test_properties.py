"""properties.py against the simulated thermometer: what it prints is what the book shows, in
docs/generated/thermometer/properties_py.txt. A stale file fails; WW_BOOK_OUTPUTS=write (`just book-outputs`)
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
OUTPUT = ROOT / "docs" / "generated" / "thermometer" / "properties_py.txt"

sys.path.insert(0, str(HERE))
import properties  # noqa: E402


@pytest.fixture
def sim_url():
    """A simulated thermometer on a free port, stopped after the test."""
    sim = subprocess.Popen(
        ["cargo", "run", "-q", "-p", "thermometer", "--example", "thermometer_sim", "--", "--port", "0"],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        text=True,
    )
    try:
        line = sim.stdout.readline()
        assert line.startswith("thermometer at "), line
        yield line.removeprefix("thermometer at ").strip()
    finally:
        sim.terminate()
        sim.wait(timeout=10)


def test_properties_prints_what_the_book_shows(sim_url):
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        properties.main(sim_url)
    text = "$ python properties.py\n" + out.getvalue()
    if os.environ.get("WW_BOOK_OUTPUTS") == "write":
        OUTPUT.parent.mkdir(parents=True, exist_ok=True)
        OUTPUT.write_text(text)
    assert OUTPUT.read_text() == text, f"{OUTPUT} is stale: run `just book-outputs`"
