#![no_std]

pub enum Kind {
    Data,
    Reserved1,
    Reserved2,

    /// See [send_nop()](WireWeaverUsbLink::send_nop) docs for explanation as to why this is needed.
    Nop = 0,

    /// Sent from host to device to get device info.
    GetDeviceInfo = 1,
    /// Sent from device to host in response to GetDeviceInfo.
    /// Sent in one packet with [DeviceInfo](DeviceInfo) struct following, since link is not up yet and ShrinkWrap uses buffer till its end.
    DeviceInfo = 2,

    /// Sent from host to device with its link, client server, and user versions.
    /// Sent in one packet with [LinkSetup](LinkSetup) struct following, since link is not up yet and ShrinkWrap uses buffer till its end.
    LinkSetup = 3,
    /// Sent from device to host to let it know that it received LinkSetup and that protocol version is compatibly.
    /// Otherwise, Disconnect with DisconnectReason::IncompatibleVersion is sent.
    /// Guard against host starting to send before device received LinkSetup to avoid losing messages.
    LinkReady = 4,

    /// Sent periodically when there are no data messages from both host and device sides
    Ping = 5,

    /// Sent from host to device, if requested by user
    GetStats = 6,
    /// Sent in response to GetStats from device side
    Stats = 7,

    /// Used to test hardware and software stack by sending lots of data back and forth.
    /// This command is followed by two u32's in LE and then test data till the end of a packet.
    /// | repeat | seq | data ... |
    ///
    /// repeat:
    /// * 0 - only count incoming packets, do not answer (used to measure host->device speed)
    /// * 1 and up - send one or more copies back (1 used to test the link integrity, more than 1 to test device-> host speed).
    Loopback = 8,

    /// Sent from host to device to let it know that driver or application is stopping.
    /// Sent from device to host to let it know that it is rebooting, e.g. to perform fw update.
    Disconnect = 15,
}
