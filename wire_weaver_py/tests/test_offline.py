"""API loaded from its crate source, without a device: tree, signatures, value conversion.

Device communication is covered by the Rust `tests/dynamic` crate, which runs the same dynamic client against a
generated server.
"""

from pathlib import Path

import pytest
import wire_weaver as ww

API_CRATE = Path(__file__).parents[2] / "tests" / "dynamic_api"

# `everything()` from tests/dynamic serialized by the code #[derive_shrink_wrap] generates
EVERYTHING_HEX = (
    "ce4aaab2abefbeefbeaddefefffffffffffffffbd4fe90eefeff01000000000000800000c03f00000000000002c068656c6c6f"
    "01020301000200ffff07a086010080036e6f74650400400010050020010272656374049003000a0009d01093141635"
)

EVERYTHING = dict(
    flag=True,
    small=9,
    signed=-7,
    nib=0xA,
    v=1234,
    a=0xAB,
    b=0xBEEF,
    c=0xDEAD_BEEF,
    d=2**64 - 2,
    e=-5,
    f=-300,
    g=-70_000,
    h=-(2**63) + 1,
    x=1.5,
    y=-2.25,
    name="hello",
    bytes=b"\x01\x02\x03",
    arr=[1, 2, 0xFFFF],
    pair=(7, 100_000),
    opt=dict(id=3, note="note"),
    res={"Err": "Missing"},
    shapes=["Empty", {"Circle": 5}, {"Rect": dict(w=1, h=2, label="rect")}, "Far"],
    range=(3, 10),
    fixed=dict(a=9, b=True),
    last=5,
)


@pytest.fixture(scope="module")
def api():
    return ww.load_api(str(API_CRATE), "Dynamic")


def test_tree(api):
    assert sorted(dir(api)) == ["add", "channel", "check", "echo", "everything", "flagged", "no_args", "speed"]
    assert "add(a: u32, b: i16) -> i64" in repr(api)
    assert api.add.signature == "add(a: u32, b: i16) -> i64"
    assert api.speed.signature == "rw speed: u16"
    assert api.channel[2].gain.path == "channel[2].gain"
    assert api["speed"].signature == api.speed.signature


def test_same_bytes_as_derived(api):
    data = api.everything.encode(EVERYTHING)
    assert data.hex() == EVERYTHING_HEX
    assert api.everything.decode(data) == EVERYTHING


def test_relocated_flags(api):
    # `Flagged` from tests/dynamic serialized by the code #[derive_shrink_wrap] generates
    value = dict(a=5, early="hi", res={"Ok": 7}, tagged={"Pair": dict(first=1, second=None)}, late=300)
    assert api.flagged.encode(value).hex() == "bc686907a0012c0122"
    assert api.flagged.decode(api.flagged.encode(value)) == value
    # missing Option fields are None, their flags too
    value = dict(a=0, res={"Err": "TooBig"}, tagged={"Pair": dict(second=2)})
    assert api.flagged.encode(value).hex() == "0000c00221"
    assert api.flagged.decode(bytes.fromhex("0000c00221")) == dict(value, early=None, late=None, tagged={"Pair": dict(first=None, second=2)})


def test_accepted_forms(api):
    from dataclasses import dataclass

    @dataclass
    class Inner:
        id: int
        note: str

    alt = dict(EVERYTHING, bytes=[1, 2, 3], arr=(1, 2, 0xFFFF), range=range(3, 10), opt=Inner(3, "note"))
    assert api.everything.encode(alt).hex() == EVERYTHING_HEX


def test_args(api):
    assert api.add.encode_args(40, -2) == api.add.encode_args(b=-2, a=40)
    assert api.add.encode_args(40, -2) == (40).to_bytes(4, "little") + (-2).to_bytes(2, "little", signed=True)
    assert api.no_args.encode_args() == b""
    assert api.check.decode_return(b"\x80\x05") == {"Ok": 5}
    assert api.check.decode_return(b"\x00\x00\x01") == {"Err": "TooBig"}


@pytest.mark.parametrize(
    "call, error, message",
    [
        (lambda a: a.add.encode_args(-1, 0), ValueError, "argument 'a': -1 is out of range for u32"),
        (lambda a: a.add.encode_args(1), TypeError, "missing argument 'b: i16'"),
        (lambda a: a.add.encode_args(1, 2, 3), TypeError, "takes 2 arguments, got 3"),
        (lambda a: a.add.encode_args(1, c=3), TypeError, "unexpected keyword argument 'c'"),
        (lambda a: a.add.encode_args("x", 1), TypeError, "expected int (u32), got 'x'"),
        (lambda a: a.speed.encode(True), TypeError, "expected int (u16)"),
        (lambda a: a.everything.encode(dict(EVERYTHING, shapes=["Nope"])), ValueError, "no variant 'Nope'"),
        (lambda a: a.everything.encode(dict(EVERYTHING, small=16)), ValueError, "16 is out of range for u4"),
        (lambda a: a.everything.encode(dict(EVERYTHING, nope=1)), ValueError, "has no field 'nope'"),
        (lambda a: a.everything.encode({k: v for k, v in EVERYTHING.items() if k != "a"}), TypeError, "missing field 'a'"),
        (lambda a: a.nope, AttributeError, "has no resource 'nope'"),
        (lambda a: a.channel.gain, AttributeError, None),
        (lambda a: a.add(1, 2), ww.WireWeaverError, "loaded offline"),
    ],
)
def test_errors(api, call, error, message):
    with pytest.raises(error) as e:
        call(api)
    if message:
        assert message in str(e.value)


def test_max_len():
    # MaxLen<T, N> (SW-19): same bytes as T, longer values are an error
    api = ww.load_api(str(API_CRATE.parent / "bounded_api"), "Bounded")
    assert api.label.signature == "rw label: MaxLen<String, 6>"
    assert api.label.decode(api.label.encode("abcdef")) == "abcdef"
    with pytest.raises(ValueError, match="at most 6"):
        api.label.encode("abcdefg")
    assert api.write.encode_args(1, True, b"\x01" * 8)
    with pytest.raises(ValueError, match="at most 8"):
        api.write.encode_args(1, True, b"\x01" * 9)


def test_series():
    # Delta / DeltaOfDelta / XorFloat (SW-50): lists of numbers, written with their codec
    api = ww.load_api(str(API_CRATE.parent / "series_api"), "Series")
    assert api.stamps.signature == "rw stamps: DeltaOfDelta<u64>"
    stamps = [1_700_000_000_000, 1_700_000_000_250, 1_700_000_000_500]
    encoded = api.stamps.encode(stamps)
    assert len(encoded) < 8 * len(stamps)
    assert api.stamps.decode(encoded) == stamps
    assert api.stamps.decode(api.stamps.encode(range(3))) == [0, 1, 2]
    with pytest.raises(ValueError):
        api.stamps.encode([-1])
    with pytest.raises(TypeError):
        api.stamps.encode("abc")
    assert api.scale.encode_args([1.0, 2.5], 2.0)


def test_credit_sinks():
    api = ww.load_api(str(Path(__file__).parents[2] / "tests" / "credit_sink_api"), "CreditSinks")
    assert api.plan.credit
    assert not api.plain.credit
    assert api.plan.signature == "credit sink plan: Vec<u8>"
    assert api.plan.progress is None


def test_optional_items():
    # API-16: optional items are marked, a device's model also says which ones it leaves out (tests/traits)
    api = ww.load_api(str(Path(__file__).parents[2] / "tests" / "traits_api"), "Traits")
    assert api.i2c.i2c_write.signature == "i2c_write(data: u8) -> u8"
    assert api.i2c.i2c_wiggle_clock.signature == "i2c_wiggle_clock(count: u8) -> u8 (optional)"
    assert "rw i2c_pullups: bool (optional)" in repr(api.i2c)
