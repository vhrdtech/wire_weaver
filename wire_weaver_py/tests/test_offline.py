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
    assert sorted(dir(api)) == [
        "add", "channel", "check", "check_listed", "echo", "everything", "flagged", "no_args", "speed", "sub_byte",
        "tuples",
    ]
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


def test_tuple_structs(api):
    # SW-59: a tuple struct is a tuple, a unit struct None; bytes from tests/dynamic `tuple_structs_same_bytes_as_derived`
    data = api.tuples.encode_args((7, "hi"), None)
    assert data.hex() == "078068690205"
    assert api.tuples.encode_args(pair=(7, "hi"), unit=None) == data
    assert api.tuples.decode_return(bytes([9, 4, 3, 3])) == (9, 4, 3)
    with pytest.raises(ValueError, match="expected 2 fields, got 3"):
        api.tuples.encode_args((7, "hi", 1), None)


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
    api = ww.load_api(str(API_CRATE.parent / "series_api"), "Sequences")
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


# replies from tests/common_error (`reply_bytes_for_python`): `read()` returning `Err(Common(InUse { by: "USART2" }))`
# and `Err(Nack)`, `calibrate()` returning `Err(Busy)` (a `CommonError` alone)
READ_IN_USE_HEX = "000020555341525432060929"
NACK_HEX = "001001"
BUSY_HEX = "003001"


@pytest.fixture(scope="module")
def errors_api():
    return ww.load_api(str(API_CRATE.parent / "common_error_api"), "CommonErrors")


def test_common_error(errors_api):
    data = bytes.fromhex(READ_IN_USE_HEX)
    in_use = {"InUse": dict(by="USART2")}
    assert errors_api.read.decode_return(data) == {"Err": {"Common": in_use}}
    with pytest.raises(ww.RemoteError, match=r"read\(\) returned an error: In use by USART2$") as e:
        errors_api.read.decode_return(data, raise_err=True)
    assert e.value.value == {"Common": in_use}
    assert e.value.common == in_use


def test_common_error_alone(errors_api):
    with pytest.raises(ww.RemoteError, match="Busy with another operation, try again later$") as e:
        errors_api.calibrate.decode_return(bytes.fromhex(BUSY_HEX), raise_err=True)
    assert e.value.value == "Busy"
    assert e.value.common == "Busy"


def test_own_error(errors_api):
    # an error of the API's own: no common part, its doc line as the message
    with pytest.raises(ww.RemoteError, match="The sensor did not answer$") as e:
        errors_api.read.decode_return(bytes.fromhex(NACK_HEX), raise_err=True)
    assert e.value.value == "Nack"
    assert e.value.common is None


def test_error_lists():
    # API-32: a method's #[errors(..)] in its docs (help()), each entry with its variant's doc line
    api = ww.load_api(str(API_CRATE.parent / "methods_api"), "Methods")
    assert api.listed_errors.doc.endswith(
        "which are not in the list\n\nErrors:\n"
        "    Common(Busy): Busy with another operation, try again later\n"
        "    Common(InUse): In use by {by}\n"
        "    Nack: The sensor did not answer"
    )
    assert api.all_common.doc.endswith("Errors:\n    Common: any CommonError")
    assert "Errors:" not in api.fallible_absent.doc


def test_observable_properties():
    # TOOL-11: introspection alone (no device), `plain` is left out (no `+observe`)
    api = ww.load_api(str(API_CRATE.parent / "observe_api"), "Observe")
    assert api.observable_properties() == {"temp": "f32", "mode": "u8", "label": "String"}
    assert api.motor.observable_properties() == {
        "speed": "u32",
        "current": "u32",
        "position": "u32",
    }


def test_generic_types():
    # generic types (API-43): definitions once, each use with its arguments, concrete for encode / decode
    api = ww.load_api(str(API_CRATE.parent / "generics_api"), "Generics")
    assert api.swap.signature == "swap(pair: Pair<u8, u16>) -> Pair<u16, u8>"
    assert api.outer.signature == "rw outer: Outer<3>"
    # `Ampere` is `ww_si::Quantity<units::Ampere>` (STD-38), named by its definition (alias names are not in ww_self)
    assert api.latest.signature == "rw latest: Timed<Quantity<Ampere>>"
    assert api.readings.signature == "stream readings: Reading<i16>"
    outer = dict(id=7, window=dict(samples=[1, 2, 0xFFF]))
    assert api.outer.decode(api.outer.encode(outer)) == outer
    with pytest.raises(ValueError):
        api.outer.encode(dict(id=7, window=dict(samples=[1, 2])))
    latest = dict(t=dict(prefix="Milli", value={"U32": 1500}), value=dict(prefix="Micro", value={"I16": -250}))
    # quantities decode as ww.Quantity (CLT-31), the plain dict form still encodes
    decoded = api.latest.decode(api.latest.encode(latest))
    assert (str(decoded["t"]), str(decoded["value"])) == ("1500 ms", "-250 µA")
    assert api.latest.encode(decoded) == api.latest.encode(latest)
    assert api.swap.encode_args(dict(a=1, b=300))


# `readings(6)` from tests/series, as the generated server writes it (`READINGS_6_HEX` there)
READINGS_6_HEX = (
    "6609884dce2671b3919cace6671040618880628080618880620800860863616c696272617465640291841063616c696272617465647761"
    "726d4292808000860863616c696272617465640291841063616c696272617465647761726d429204062510093141c212810800"
)


def test_series_columns():
    # CLT-29: a Series<T> as items and as numpy columns, leaves under a variant or an Option masked
    np = pytest.importorskip("numpy")
    api = ww.load_api(str(API_CRATE.parent / "series_api"), "Sequences")
    assert api.readings.signature == "readings(n: u8) -> Series<Reading>"
    data = bytes.fromhex(READINGS_6_HEX)

    items = api.readings.decode_return(data)
    assert len(items) == 6
    assert items[2] == dict(
        scale="Milli", value=3300, label="b", notes=["calibrated", "warm"], status={"Busy": 2}, ok=True
    )

    cols = api.readings.decode_columns(data)
    assert sorted(cols) == ["label", "notes", "ok", "scale", "status", "status.Busy.0", "status.Fault.code", "value"]
    assert cols["value"].dtype == np.int32
    assert cols["value"].tolist() == [3298, 3299, 3300, 3301, 3302, 3298]
    assert cols["scale"].tolist() == ["Milli"] * 4 + ["Micro"] * 2
    assert cols["label"].tolist() == ["a", "a", "b", "a", "a", "b"]
    assert cols["notes"].tolist() == [[], ["calibrated"], ["calibrated", "warm"], [], ["calibrated"], ["calibrated", "warm"]]
    assert cols["status"].tolist() == ["Idle", "Busy", "Busy", "Fault", "Fault", "Idle"]
    busy = cols["status.Busy.0"]
    assert busy.dtype == np.uint8
    assert busy.tolist() == [None, 1, 2, None, None, None]
    assert cols["status.Fault.code"].tolist() == [None, None, None, 7, 7, None]
    assert cols["ok"].dtype == np.bool_
    assert cols["ok"].tolist() == [None, False, True, False, None, False]
    # every item has a value: a plain array, not a masked one
    assert not isinstance(cols["value"], np.ma.MaskedArray)

    with pytest.raises(ww.WireWeaverError):
        api.readings.decode_columns(data[:10])
    with pytest.raises(ValueError):
        api.sum.decode_columns(b"")


def test_batched_streams():
    # API-41: a batched stream's model item carries batch_item_ty (the Timed<T> instance its wire item needs);
    # `load_api` already runs generics::prepare, so the tree sees it resolved, same as any other generic use
    # (test_generic_types). `recv_batch()`'s numpy columns need an open stream (a device), covered by
    # tests/streams_batched's dyn_client tests instead, same split as the rest of this file.
    api = ww.load_api(str(Path(__file__).parents[2] / "tests" / "streams_batched_api"), "StreamsBatched")
    assert repr(api.plain).startswith("<stream plain stream plain: Sample>")
    assert repr(api.counted).startswith("<stream counted stream counted: Sample>")
    assert repr(api.reliable).startswith("<stream reliable stream reliable: Sample>")
    assert repr(api.frames).startswith("<stream counted stream frames: Frame>")


def test_recursive_types():
    # self-referential types: an expression tree through RefBox, a tree through a list of itself
    api = ww.load_api(str(API_CRATE.parent / "recursive_api"), "Recursive")
    assert api.eval.signature == "eval(expr: Expr) -> Option<i32>"
    assert api.tree.signature == "rw tree: Tree"
    expr = {"Mul": dict(a={"Add": ({"Lit": 1}, {"Lit": 2})}, b={"Neg": {"Lit": 3}})}
    assert api.eval.encode_args(expr)
    tree = dict(label="root", children=[dict(label="a", children=[]), dict(label="b", children=[])])
    assert api.tree.decode(api.tree.encode(tree)) == tree
    # as deep as the object: bounded, also for one that contains itself
    deep = {"Lit": 7}
    for _ in range(1000):
        deep = {"Neg": deep}
    with pytest.raises(ValueError, match="nested deeper"):
        api.eval.encode_args(deep)
    looped = dict(label="loop", children=[])
    looped["children"].append(looped)
    with pytest.raises(ValueError, match="nested deeper"):
        api.tree.encode(looped)


QUANTITIES_CRATE = Path(__file__).parents[2] / "tests" / "quantities_api"


def test_quantities():
    api = ww.load_api(str(QUANTITIES_CRATE), "Quantities")
    # Volt::milli(3300u16), as the derived type writes it
    q = api.voltage.decode(bytes.fromhex("12e40c"))
    assert isinstance(q, ww.Quantity)
    assert str(q) == "3300 mV"
    assert repr(q) == "Quantity(3300 mV U16)"
    assert (q.value, q.prefix, q.exponent, q.storage) == (3300, "Milli", -3, "U16")
    assert (q.symbol, q.unit) == ("V", "Volt")
    assert q.dim == {"kg": 1, "m": 2, "s": -3, "A": -1}
    assert float(q) == 3.3
    assert q.to_si() == 3.3
    assert api.voltage.encode(q).hex() == "12e40c"
    # a number in the unit itself
    v = api.voltage.decode(api.voltage.encode(1.5))
    assert (str(v), v.storage, v.prefix) == ("1.5 V", "F64", "Unit")
    assert api.voltage.decode(api.voltage.encode(5)).value == 5

    # compound unit
    b = api.damping.decode(api.damping.encode(1.2e-4))
    assert (b.symbol, b.unit) == ("N·m/(rad/s)", "Per<Torque, AngularVelocity>")
    assert b.dim == {"kg": 1, "m": 2, "s": -1}
    assert str(b) == "0.00012 N·m/(rad/s)"
    with pytest.raises(ValueError, match="in Volt where one in Per<Torque, AngularVelocity> is expected"):
        api.damping.encode(q)

    # a unit marker ww_si doesn't know: by name and docs
    s = api.speed.decode(api.speed.encode(1500))
    assert (str(s), s.symbol, s.unit, s.dim, s.docs) == ("1500 Rpm", "Rpm", "Rpm", None, ["Revolutions per minute"])
    with pytest.raises(ValueError, match="not known to ww_si"):
        s.to_si()

    # a value with its unit (AnyQuantity, STD-37): AnyQuantity::from(Volt::milli(3300u16)) as the derived type writes it
    any_volt = "12e40c" + "00566f6c7456d21f0000000114" + "59"
    a = api.sensor.decode(bytes.fromhex(any_volt))
    assert isinstance(a, ww.Quantity)
    # the same quantity as the typed property's
    assert a == q
    assert (str(a), a.symbol, a.unit, a.dim) == ("3300 mV", "V", "Volt", {"kg": 1, "m": 2, "s": -3, "A": -1})
    assert api.sensor.encode(a).hex() == any_volt
    # any quantity goes in with its unit, the compound one too
    assert api.sensor.decode(api.sensor.encode(b)) == b
    assert str(api.sensor.decode(api.sensor.encode(b))) == "0.00012 N·m/(rad/s)"
    with pytest.raises(ValueError, match="no dimension and scale to send"):
        api.sensor.encode(s)
