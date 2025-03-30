use strum_macros::FromRepr;
use wire_weaver::ww_repr;

#[ww_repr(u4)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, FromRepr)]
enum OpCode {
    DatagramNumber = 0,

    Message = 1,

    GetDeviceInfo = 2,
    DeviceInfo = 3,

    LinkSetup = 4,
    LinkSetupResult = 5,

    Disconnect = 15,
}
