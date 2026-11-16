"""`wire_weaver.logring`: logring's Python API compiled into this wheel (TOOL-39), on an image `ww logring
make-image` writes, offline."""

import subprocess
from pathlib import Path

import pytest

from wire_weaver import logring

WW_TOOLS = Path(__file__).resolve().parents[2]


@pytest.fixture(scope="module")
def demo_image(tmp_path_factory):
    image = tmp_path_factory.mktemp("wire_weaver_logring") / "demo.wlr"
    subprocess.run(
        [
            "cargo",
            "run",
            "--quiet",
            "-p",
            "ww",
            "--",
            "logring",
            "make-image",
            str(image),
            "--tracks",
            "2",
            "--records",
            "30",
        ],
        cwd=WW_TOOLS,
        check=True,
    )
    return image


def test_version():
    assert isinstance(logring.__version__, str)
    assert logring.__version__


def test_open_lists_sessions_and_tracks(demo_image):
    log = logring.open(str(demo_image))
    assert isinstance(log, logring.LogFile)
    sessions = log.sessions()
    assert len(sessions) == 1
    assert sessions[0]["device_id"] == "make-image"
    tracks = log.tracks()
    assert sorted(t.name for t in tracks) == ["sensor_0", "sensor_1"]
    for t in tracks:
        assert isinstance(t, logring.Track)
        assert t.resolved
        assert t.decode_errors == 0
        assert len(t) == 30


def test_track_rows(demo_image):
    rows = logring.open(str(demo_image)).track("sensor_0").rows()
    assert len(rows) == 30
    assert rows[0]["pos"] == 0
    assert isinstance(rows[0]["value"], float)


def test_unknown_track_raises_key_error(demo_image):
    with pytest.raises(KeyError):
        logring.open(str(demo_image)).track("no-such-track")


def test_to_numpy(demo_image):
    numpy = pytest.importorskip("numpy")
    arrays = logring.open(str(demo_image)).track("sensor_0").to_numpy()
    assert isinstance(arrays["value"], numpy.ndarray)
    assert arrays["value"].shape == (30,)
