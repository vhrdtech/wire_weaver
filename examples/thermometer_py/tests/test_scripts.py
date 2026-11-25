"""The book's Python scripts against the simulated thermometer: what each prints is what the book shows, in
docs/generated/thermometer/<script>_py.txt. A stale file fails; WW_BOOK_OUTPUTS=write (`just book-outputs`) writes
them again.
"""

import contextlib
import importlib
import io
import os
import sys

import pytest
from conftest import HERE, ROOT

GENERATED = ROOT / "docs" / "generated" / "thermometer"

sys.path.insert(0, str(HERE))


@pytest.mark.parametrize("script", ["properties", "explore", "values", "streams", "errors"])
def test_script_prints_what_the_book_shows(sim_url, script):
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        importlib.import_module(script).main(sim_url)
    text = f"$ python {script}.py\n" + out.getvalue()
    output = GENERATED / f"{script}_py.txt"
    if os.environ.get("WW_BOOK_OUTPUTS") == "write":
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(text)
    assert output.read_text() == text, f"{output} is stale: run `just book-outputs`"
