use embassy_time::Duration;

pub struct UsbTimings {
    /// Use USB Bulk instead of Interrupt endpoints, results in faster transfer speeds at the cost of fewer
    /// guarantees. Interrupt endpoints have guaranteed, reserved bandwidth.
    pub use_bulk_endpoints: bool,
    /// USB packet is not sent immediately to avoid sending a lot of small packets.
    /// The host is told to use the same value.
    pub packet_accumulation_time: Duration,
    /// Used to determine if the host driver is stopped and no longer receiving data
    pub packet_send_timeout: Duration,
}

const PACKET_SEND_TIMEOUT: Duration = Duration::from_millis(250);

// TODO: tune this better across 3 OSes and FS/HS
impl UsbTimings {
    pub fn fs_higher_speed() -> Self {
        Self {
            use_bulk_endpoints: true,
            packet_accumulation_time: Duration::from_micros(1000),
            packet_send_timeout: PACKET_SEND_TIMEOUT,
        }
    }

    pub fn fs_lower_latency() -> Self {
        Self {
            use_bulk_endpoints: false,
            packet_accumulation_time: Duration::from_micros(450),
            packet_send_timeout: PACKET_SEND_TIMEOUT,
        }
    }

    pub fn hs_higher_speed() -> Self {
        Self {
            use_bulk_endpoints: true,
            packet_accumulation_time: Duration::from_micros(250),
            packet_send_timeout: PACKET_SEND_TIMEOUT,
        }
    }

    pub fn hs_lower_latency() -> Self {
        Self {
            use_bulk_endpoints: false,
            packet_accumulation_time: Duration::from_micros(125), // 125μs seem to be t0o small and results in many small packets
            packet_send_timeout: PACKET_SEND_TIMEOUT,
        }
    }

    pub(crate) fn accumulation_time(&self) -> core::time::Duration {
        core::time::Duration::from_micros(self.packet_accumulation_time.as_micros())
    }
}
