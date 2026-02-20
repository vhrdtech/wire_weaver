#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_trait]
pub trait ApiRoot {
    // impl_!(FirmwareUpdate);
    // impl_!(FirmwareInfo);
    // impl_!(BoardInfo);
    // impl_!(IndicationControl);
    // impl_!(Counters);
    // impl_!(LogDefmt);

    // property!(config, Config);

    // TODO: use u7 addresses

    /// Query I2C bus capabilities supported by the adapter.
    fn i2c_capabilities() -> I2cCapabilities;

    /// Configure I2C bus with the specified mode and IO level voltage.
    fn i2c_configure(mode: I2cMode, io_level_mv: u16) -> Result<u32, I2cError>;

    /// Perform I2C write transaction with the specified address and data.
    ///
    /// If delay_after is not zero, then following transactions or cycle reads to the same device will wait.
    /// Transactions to other addresses will be executed, but no look-ahead is performed on the command queue:
    /// if a transaction to a device with delay set is encountered, then all other transactions will wait and
    /// only cycle reads will be executed to other non-busy devices in the meantime.
    fn i2c_write(
        addr: u8,
        data: Vec<u8>,
        delay_after: u32,
    ) -> Result<I2cTransactionTimeRange, I2cError>;

    /// Perform I2C read or write-read (repeated start) transaction.
    ///
    /// If delay_after is not zero, then following transactions or cycle reads to the same device will wait.
    /// Transactions to other addresses will be executed, but no look-ahead is performed on the command queue:
    /// if a transaction to a device with delay set is encountered, then all other transactions will wait and
    /// only cycle reads will be executed to other non-busy devices in the meantime.
    fn i2c_read(
        addr: u8,
        kind: I2cReadKind,
        len: u16,
        delay_after: u32,
    ) -> Result<I2cReadEnvelope, I2cError>;

    // TODO: SMBus commands

    /// Scan for devices on the bus and return address of devices that ACKed 1 byte read transaction.
    fn i2c_scan() -> Result<Vec<u8>, I2cError>;

    // Perform several read transactions before condition is met or timeout occurs.
    // Can be used to speed up communications by avoiding repeated command-response cycles.
    // fn i2c_poll()

    // Return I2C IO state, both SCL and SDA must be high if no transactions are performed.
    // Can be low if e.g. some device is stuck or there is no pull up resistors or if power to them is off.
    // fn i2c_lines_state() -> I2cLinesState;

    // Output count clocks on SCL line, can potentially be used to get a slave device out from a weird state,
    // and make it release the bus.
    // fn i2c_wiggle_clock(count: u32);

    // Manually control I2C IO lines
    // fn i2c_io_control(io_mode: I2cIoMode);

    // Start a periodic read process with results streamed on the i2c_cycle_read stream.
    // Cycle reads are performed when there are no commands to execute. TODO: low priority commands?
    // Cycle reads to a particular device are paused for delay_after if it was passed with command.
    // fn i2c_cycle_read(
    //     addr: u8,
    //     kind: I2cReadKind,
    //     period_us: u32,
    // ) -> Result<I2cCycleSlot, I2cError>;

    // Modify a period of a previously started cycle read.
    // fn i2c_cycle_modify(slot: I2cCycleSlot, new_period_us: u32) -> Result<(), I2cError>;

    // Stop a previously started cycle read.
    // fn i2c_cycle_cancel(slot_idx: I2cCycleSlot) -> Result<(), I2cError>;

    // Stream of events from cycle reads.
    // stream_up!(i2c_cycle_read, I2cCycleRead);
}

pub struct RawTimestamp {
    pub ticks: u32,
}

pub struct I2cTransactionTimeRange {
    pub start: RawTimestamp,
    pub end: RawTimestamp,
}

pub struct I2cReadEnvelope {
    pub duration: I2cTransactionTimeRange,
    pub data: Vec<u8>,
}

pub enum I2cReadKind {
    Plain,
    RepeatedStart { write: Vec<u8> },
}

pub struct I2cCycleSlot {
    pub slot_idx: u16,
}

pub struct I2cCycleRead {
    pub slot: I2cCycleSlot,
    pub result: Result<I2cReadEnvelope, I2cError>,
}

pub enum I2cError {
    /// Returned by i2c_configure if mode is not supported
    UnsupportedMode,
    /// Returned by i2c_cycle_read if I2C is not a master
    WrongMode,
    UnsupportedSpeed,
    OutOfCycleSlots,
    InvalidCycleSlot,
    MalformedRequest,
    BufferTooBig,
    UnsupportedIoLevel,

    /// Bus error
    Bus,
    /// Arbitration lost
    Arbitration,
    /// ACK not received (either to the address or to a data byte)
    Nack,
    /// Timeout
    Timeout,
    /// CRC error
    Crc,
    /// Overrun error
    Overrun,
    /// Zero-length transfers are not allowed.
    ZeroLengthTransfer,
}

pub enum I2cMode {
    /// Disable I2C controller, disconnect it from IO pins and set IOs as high-Z inputs
    Disabled,
    /// Init I2C controller in master mode with specified frequency, optionally enable monitoring
    Master { speed_hz: u32, monitor: bool },
    /// Init I2C controller in slave mode with specified address, optionally enable monitoring
    Slave { addr: u8, monitor: bool },
    /// Init I2C monitor to spy on the bus traffic without affecting it
    MonitorOnly,
    // SMBus,
    // PMBus,
}

pub struct I2cCapabilities {
    /// Supported discrete IO levels, e.g. 1V8, 3V3
    pub supported_io_levels_mv: Vec<u16>,
    /// Supported IO levels range, if e.g. adjustable LDO is available
    pub supported_io_range_mv_min: Option<u16>, // TODO: replace with tuple when supported
    pub supported_io_range_mv_max: Option<u16>,

    pub max_write_buffer_len: u32,
    pub max_read_buffer_len: u32,

    pub min_frequency_hz: u32,
    pub max_frequency_hz: u32,

    pub cycle_slots_count: u32,

    pub driver_name: String,

    pub master_supported: bool,
    pub slave_supported: bool,
    pub monitor_supported: bool,
}

pub struct I2cLinesState {
    pub is_scl_high: bool,
    pub is_sda_high: bool,
}
