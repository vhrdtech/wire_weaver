"""Common errors (STD-16) from Python: a device answers any call, read or write with one of `ww_error::CommonError`
(`Busy`, `InUse`, ...), whatever the item's own error type, and Python raises `RemoteError` with it in `common`.
`tests/hub_pytest_device` hosts `tests/common_error`'s server (`inproc:errors-device`) beside the hub, as
`test_hub.py`'s fixture starts it.
"""

import pytest

import wire_weaver as ww
from test_hub import hub_device  # noqa: F401, the fixture

DEVICE_KEY = "inproc:errors-device"


@pytest.fixture
def dev(hub_device, monkeypatch):  # noqa: F811
    monkeypatch.setenv("WW_RUNTIME_DIR", hub_device["WW_RUNTIME_DIR"])
    dev = ww.connect(hub_device=DEVICE_KEY)
    yield dev
    dev.disconnect()


def test_common_error_of_a_result_method(dev):
    # `read(fail) -> Result<u16, SensorError>`: a common error comes through the protocol, not as the `Err`
    assert dev.read(0) == 1234
    with pytest.raises(ww.RemoteError, match="In use by USART2$") as e:
        dev.read(1)
    assert e.value.common == {"InUse": {"by": "USART2"}}
    assert e.value.value is None
    with pytest.raises(ww.RemoteError, match="Hardware fault 0x2a$") as e:
        dev.read(2)
    assert e.value.common == {"Fault": {"code": 42}}


def test_own_error(dev):
    # the API's own error: in `value`, no common part, its doc line as the message
    with pytest.raises(ww.RemoteError, match="The sensor did not answer$") as e:
        dev.read(3)
    assert e.value.value == "Nack"
    assert e.value.common is None


def test_common_error_of_a_plain_method(dev):
    # `calibrate()` has no error type: a common error is the only error it can return
    with pytest.raises(ww.RemoteError, match="Busy with another operation, try again later$") as e:
        dev.calibrate()
    assert e.value.common == "Busy"
    assert e.value.value is None
