"""The simulated thermometer the book's scripts run against, started by each test that asks for it."""

import subprocess
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parents[1]
ROOT = HERE.parents[1]


@pytest.fixture
def sim_url():
    """A simulated thermometer in a still room, on a free port, fresh for each test and stopped after it."""
    sim = subprocess.Popen(
        ["cargo", "run", "-q", "-p", "thermometer", "--example", "thermometer_sim", "--", "--port", "0", "--still"],
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
