"""The wire-format conformance matrix (TEST-23) for Python: every case of tests/conformance/corpus.json encoded and
decoded through the API model of tests/conformance_api, with the rules of the Rust runner (tests/conformance):
a case Python can't run carries a gap mark, a missing or stale mark fails. See docs/serdes/conformance.md.
"""

import json
from pathlib import Path

import pytest
import wire_weaver as ww

ROOT = Path(__file__).parents[2]
CORPUS = json.loads((ROOT / "tests" / "conformance" / "corpus.json").read_text())
CASES = CORPUS["cases"]


@pytest.fixture(scope="module")
def api():
    return ww.load_api(str(ROOT / "tests" / "conformance_api"), "Conformance")


def expected(case):
    """The corpus value as Python gives it: integers beyond 64 bits are strings in the corpus."""
    value = case.get("value")
    if case["type"] in ("u128", "i128") and isinstance(value, str):
        return int(value)
    return value


def plain(value):
    """A decoded value in the corpus form: bytes and tuples as lists, a `ww.Quantity` as the struct it is on the wire
    (prefix and the stored number; a `Fixed` only the stored number)."""
    if isinstance(value, ww.Quantity):
        if value.fixed:
            return {"value": value.value}
        return {"prefix": value.prefix, "value": {value.storage: value.value}}
    if isinstance(value, (bytes, bytearray, tuple, list)):
        return [plain(v) for v in value]
    if isinstance(value, dict):
        return {k: plain(v) for k, v in value.items()}
    return value


def same(a, b):
    """Equal and of the same kind: True is not 1, 1.0 is not 1."""
    if type(a) is not type(b):
        return False
    if isinstance(a, list):
        return len(a) == len(b) and all(same(x, y) for x, y in zip(a, b))
    if isinstance(a, dict):
        return a.keys() == b.keys() and all(same(a[k], b[k]) for k in a)
    return a == b


def golden(case):
    return bytes.fromhex("".join(case.get("bytes", "").split()))


def attempt(api, case):
    """None when Python has no form for the case's type, else (ok, message)."""
    try:
        node = api[case["type"]]
    except Exception:
        return None
    direction = case.get("direction", "both")
    data = golden(case)
    try:
        if direction in ("both", "reject_value"):
            try:
                encoded = node.encode(expected(case))
            except Exception as e:
                if direction == "reject_value":
                    return True, ""
                return False, f"encode failed: {e}"
            if direction == "reject_value":
                return False, f"encoded a value that must be rejected: {encoded.hex()}"
            if encoded != data:
                at = next((i for i, (x, y) in enumerate(zip(encoded, data)) if x != y), min(len(encoded), len(data)))
                return False, f"encode: bytes differ at byte {at}: got {encoded.hex()}, golden {data.hex()}"
        try:
            decoded = node.decode(data)
        except Exception as e:
            if direction == "reject":
                return True, ""
            return False, f"decode failed: {e}"
        if direction == "reject":
            return False, f"decoded bytes that must be rejected: {decoded!r}"
        if not same(plain(decoded), expected(case)):
            return False, f"decode: decoded {decoded!r}, expected {expected(case)!r}"
        return True, ""
    except Exception as e:  # anything else is a failure of this consumer, with its message
        return False, f"{type(e).__name__}: {e}"


@pytest.mark.parametrize("case", CASES, ids=[c["name"] for c in CASES])
def test_case(api, case):
    gap = case.get("gaps", {}).get("python")
    result = attempt(api, case)
    if gap is not None:
        assert result is None or not result[0], f"passes now: remove its gap mark ({gap}) and close what it names"
    else:
        assert result is not None, "can't be run: give it a gap mark with the reason"
        ok, message = result
        assert ok, message


REQUANTIZE = CORPUS.get("requantize", [])


def number(j):
    """A corpus number: integers beyond 64 bits are strings."""
    return int(j) if isinstance(j, str) else j


def convert(api, case):
    """(ok, message) of `ww.requantize` on the case, the factor as the dynamic client has it: the corpus form
    encoded through the API model's `magnitude` property, those bytes the Magnitude."""
    try:
        factor = api["magnitude"].encode(case["factor"])
        got = ww.requantize(number(case["value"]), factor, case["to"], case["rounding"], case["overflow"])
    except Exception as e:
        return False, f"{type(e).__name__}: {e}"
    want = number(case["result"])
    if same(got, want):
        return True, ""
    return False, f"gave {got!r}, expected {want!r}"


@pytest.mark.parametrize("case", REQUANTIZE, ids=[c["name"] for c in REQUANTIZE])
def test_requantize(api, case):
    """A stored number converted to another scale and storage (STD-63) by `ww.requantize`, with the gap rules of
    the cases above."""
    gap = case.get("gaps", {}).get("python")
    ok, message = convert(api, case)
    if gap is not None:
        assert not ok, f"passes now: remove its gap mark ({gap}) and close what it names"
    else:
        assert ok, message
