//! embedded-hal I2C over WireWeaver: a stock `embedded-hal` 1.0 driver, blocking or async, runs unchanged on the
//! host against the I2C bus of a remote device serving `ww_i2c::I2c` (a USB adapter, a dev board, an instrument).
//!
//! ```rust,ignore
//! let device = MyAdapter::new().connect().await?;
//! let mut i2c = I2cMaster::new(device.i2c().attachment()).await?.urgent();
//! let mut sensor = tmp1x2::Tmp1x2::new(i2c, tmp1x2::SlaveAddr::default());
//! let t = sensor.read_temperature().await?;
//! ```
//!
//! # One transaction, one round trip
//!
//! Every `embedded-hal` call (`write`, `read`, `write_read`, `transaction`) is one `ww_i2c` call, so one round trip
//! to the device. `embedded-hal` runs contiguous operations of the same direction without a repeated start, while
//! `ww_i2c` puts one before every operation (`I2c::i2c_transaction`), so [I2cMaster] merges them first: writes into
//! one write, reads into one read whose bytes are split again from the answer. Then it picks the call:
//!
//! | Operations after merging | Call                                             |
//! |--------------------------|--------------------------------------------------|
//! | one write                | `i2c_write`                                      |
//! | one read                 | `i2c_read`, `I2cReadKind::Plain`                 |
//! | a write, then a read     | `i2c_read`, `I2cReadKind::RepeatedStart`, while the write fits [I2cLimits::max_write_read_len] |
//! | anything else            | `i2c_transaction`                                |
//!
//! A transaction the device doesn't take in one call (more merged operations than [I2cLimits::max_ops], a write
//! or the reads together longer than it takes) is refused with [Error::TooManyOps], [Error::WriteTooLong] or
//! [Error::ReadTooLong] before anything is sent, never split: a split would put a stop and a new start where the
//! driver expects none. The limits are the device's `I2c::i2c_capabilities`, read once by [I2cMaster::new].
//!
//! # Priority
//!
//! A latency-bound driver (a USB PD sink answering within tReceiverResponse) sets its requests urgent:
//! [I2cMaster::urgent] or [I2cMaster::set_priority]. An urgent request leaves the host at once and the device sends
//! its answer at once, instead of after the accumulation window of the link (CLT-50,
//! `docs/transport/ww_framer.md`). The priority is the handle's own: other handles on the same client keep theirs.
//!
//! # Errors
//!
//! [Error] implements `embedded_hal::i2c::Error`: `I2cError::Nack` is `NoAcknowledge` of the address
//! (`at: Some(0)`), of data (`at: Some(n)`) or unknown (`at: None`); `Arbitration` is `ArbitrationLoss`; `Bus` and
//! `BusStuck` are `Bus`; `Overrun` is `Overrun`; everything else (`ClockStretch`, `Timeout`, a refused
//! transaction, a lost link) is `Other`. The full error is kept: `Error::I2c` has the device's `I2cErrorOwned`,
//! with `Nack::op` counting the merged operations.

mod plan;
mod ww;

use embedded_hal::i2c::{
    ErrorKind, ErrorType, NoAcknowledgeSource, Operation, SevenBitAddress, TenBitAddress,
};
use plan::Plan;
use wire_weaver::Priority;
use wire_weaver::prelude::*;
use wire_weaver_client::{Attachment, CallError};
use ww::I2cClient;
pub use ww_i2c;
use ww_i2c::{
    I2cAddress, I2cCapabilitiesOwned, I2cErrorOwned, I2cReadKindOwned, I2cTransactionTimeRange,
    MAX_OPS, MAX_READ_LEN, MAX_WRITE_LEN, MAX_WRITE_READ_LEN,
};

/// What a device takes in one call, from `I2c::i2c_capabilities` (see [I2cMaster::new]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct I2cLimits {
    /// Longest write.
    pub max_write_len: u16,
    /// Most bytes read in one call, all reads of a transaction together.
    pub max_read_len: u16,
    /// Longest write part of a register read (`I2cReadKind::RepeatedStart`); a longer one goes as a transaction.
    pub max_write_read_len: u16,
    /// Most operations of one `I2c::i2c_transaction`, after merging.
    pub max_ops: u16,
}

impl Default for I2cLimits {
    /// The largest the `ww_i2c` wire format carries ([MAX_WRITE_LEN], ...): what a device takes at most.
    fn default() -> Self {
        I2cLimits {
            max_write_len: MAX_WRITE_LEN as u16,
            max_read_len: MAX_READ_LEN as u16,
            max_write_read_len: MAX_WRITE_READ_LEN as u16,
            max_ops: MAX_OPS as u16,
        }
    }
}

impl From<&I2cCapabilitiesOwned> for I2cLimits {
    /// The device's limits, never above what the wire format carries.
    fn from(caps: &I2cCapabilitiesOwned) -> Self {
        let wire = I2cLimits::default();
        I2cLimits {
            max_write_len: caps.max_write_len.min(wire.max_write_len),
            max_read_len: caps.max_read_len.min(wire.max_read_len),
            max_write_read_len: caps.max_write_read_len.min(wire.max_write_read_len),
            max_ops: caps.max_ops.min(wire.max_ops),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// The device answered with an error.
    #[error("ww_i2c: {0:?}")]
    I2c(I2cErrorOwned),
    #[error("'{resource}' is not implemented by the device")]
    Unimplemented { resource: String },
    #[error("client error: '{0:?}'")]
    Client(wire_weaver_client::Error),
    #[error("expected ww_i2c::I2c trait, got: '{0}'")]
    IncompatibleTrait(String),
    /// A 7-bit address above `0x7F` or a 10-bit one above `0x3FF`.
    #[error("I2C address {0:#x} out of range")]
    InvalidAddress(u16),
    /// More operations after merging than the device takes in one transaction; not sent.
    #[error("{ops} operations after merging, the device takes {max} in one transaction")]
    TooManyOps { ops: usize, max: u16 },
    /// A write (after merging) longer than the device takes; not sent.
    #[error("a write of {len} bytes, the device takes {max}")]
    WriteTooLong { len: usize, max: u16 },
    /// More bytes read in one transaction than the device takes; not sent.
    #[error("{len} bytes read in one transaction, the device takes {max}")]
    ReadTooLong { len: usize, max: u16 },
    /// The device answered another number of bytes than asked for.
    #[error("the device read {got} bytes, {expected} were asked for")]
    ShortRead { got: usize, expected: usize },
}

impl From<CallError<I2cErrorOwned>> for Error {
    fn from(e: CallError<I2cErrorOwned>) -> Self {
        match e {
            CallError::Device(e) => Error::I2c(e),
            CallError::Unimplemented { resource } => Error::Unimplemented { resource },
            CallError::Client(e) => Error::Client(e),
        }
    }
}

impl From<wire_weaver_client::Error> for Error {
    fn from(e: wire_weaver_client::Error) -> Self {
        Error::Client(e)
    }
}

impl embedded_hal::i2c::Error for Error {
    fn kind(&self) -> ErrorKind {
        match self {
            Error::I2c(e) => match e {
                I2cErrorOwned::Nack { at, .. } => ErrorKind::NoAcknowledge(match at {
                    Some(0) => NoAcknowledgeSource::Address,
                    Some(_) => NoAcknowledgeSource::Data,
                    None => NoAcknowledgeSource::Unknown,
                }),
                I2cErrorOwned::Arbitration => ErrorKind::ArbitrationLoss,
                I2cErrorOwned::Bus | I2cErrorOwned::BusStuck { .. } => ErrorKind::Bus,
                I2cErrorOwned::Overrun => ErrorKind::Overrun,
                _ => ErrorKind::Other,
            },
            _ => ErrorKind::Other,
        }
    }
}

/// The I2C master of a remote device as an `embedded_hal::i2c::I2c` and `embedded_hal_async::i2c::I2c`, for 7-bit
/// and 10-bit addresses. See the [crate] docs for how transactions map to calls.
pub struct I2cMaster {
    client: I2cClient,
    limits: I2cLimits,
    last: Option<I2cTransactionTimeRange>,
}

impl I2cMaster {
    /// Reads the device's limits (`I2c::i2c_capabilities`, one round trip) and returns the master.
    ///
    /// Get the [Attachment] from a client of an API that implements `ww_i2c::I2c`:
    /// `my_client.i2c().attachment()`.
    pub async fn new(attachment: Attachment) -> Result<Self, Error> {
        let client = Self::client(attachment)?;
        let caps = client.i2c_capabilities().call().await?;
        Ok(Self::with_limits_of(client, I2cLimits::from(&caps)))
    }

    /// Blocking variant of [Self::new]; don't call it from an async task.
    pub fn new_blocking(attachment: Attachment) -> Result<Self, Error> {
        let client = Self::client(attachment)?;
        let caps = client.i2c_capabilities().blocking_call()?;
        Ok(Self::with_limits_of(client, I2cLimits::from(&caps)))
    }

    /// Without asking the device: `limits` as known from elsewhere. A transaction past the device's real limits
    /// gets its `BufferTooBig` instead of being refused here.
    pub fn with_limits(attachment: Attachment, limits: I2cLimits) -> Result<Self, Error> {
        Ok(Self::with_limits_of(Self::client(attachment)?, limits))
    }

    fn client(attachment: Attachment) -> Result<I2cClient, Error> {
        if attachment.trait_name() != "I2c" || attachment.source_crate().crate_id != "ww_i2c" {
            return Err(Error::IncompatibleTrait(format!(
                "{}::{}",
                attachment.source_crate().crate_id,
                attachment.trait_name()
            )));
        }
        Ok(I2cClient {
            cmd: attachment.cmd_tx_take(),
        })
    }

    fn with_limits_of(client: I2cClient, limits: I2cLimits) -> Self {
        I2cMaster {
            client,
            limits,
            last: None,
        }
    }

    /// The limits transactions are checked against.
    pub fn limits(&self) -> &I2cLimits {
        &self.limits
    }

    /// Priority of this master's requests from now on, [Priority::NORMAL] unless the client was configured with
    /// another default. [Priority::URGENT] and above go out at once and are answered at once.
    pub fn set_priority(&mut self, priority: Priority) {
        self.client.cmd.set_default_priority(priority);
    }

    /// Priority of this master's requests.
    pub fn priority(&self) -> Priority {
        self.client.cmd.default_priority()
    }

    /// This master with every request urgent ([Priority::URGENT]): for latency-bound drivers.
    pub fn urgent(mut self) -> Self {
        self.set_priority(Priority::URGENT);
        self
    }

    /// When the last transaction that reached the bus started and ended, in device time: `None` before the first
    /// one and after a transaction with no bytes to move.
    pub fn last_transaction_time(&self) -> Option<I2cTransactionTimeRange> {
        self.last
    }

    fn addr7(address: SevenBitAddress) -> Result<I2cAddress, Error> {
        I2cAddress::seven(address).ok_or(Error::InvalidAddress(address as u16))
    }

    fn addr10(address: TenBitAddress) -> Result<I2cAddress, Error> {
        I2cAddress::ten(address).ok_or(Error::InvalidAddress(address))
    }

    async fn run(
        &mut self,
        addr: I2cAddress,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Error> {
        let plan = plan::plan(operations, &self.limits)?;
        let expected = plan.read_len();
        let c = &self.client;
        let (time, read) = match plan {
            Plan::Nothing => {
                self.last = None;
                return Ok(());
            }
            Plan::Write(w) => (
                c.i2c_write(addr, MaxLen::new_unchecked(w), None)
                    .call()
                    .await?,
                Vec::new(),
            ),
            Plan::Read(len) => {
                let r = c
                    .i2c_read(addr, I2cReadKindOwned::Plain, len, None)
                    .call()
                    .await?;
                (r.duration, r.data.into_inner())
            }
            Plan::WriteRead { write, read } => {
                let kind = I2cReadKindOwned::RepeatedStart {
                    write: MaxLen::new_unchecked(write),
                };
                let r = c.i2c_read(addr, kind, read, None).call().await?;
                (r.duration, r.data.into_inner())
            }
            Plan::Transaction(ops) => {
                let r = c
                    .i2c_transaction(addr, MaxLen::new_unchecked(ops))
                    .call()
                    .await?;
                (r.duration, r.read.into_inner())
            }
        };
        self.finish(time, &read, expected, operations)
    }

    fn run_blocking(
        &mut self,
        addr: I2cAddress,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Error> {
        let plan = plan::plan(operations, &self.limits)?;
        let expected = plan.read_len();
        let c = &self.client;
        let (time, read) = match plan {
            Plan::Nothing => {
                self.last = None;
                return Ok(());
            }
            Plan::Write(w) => (
                c.i2c_write(addr, MaxLen::new_unchecked(w), None)
                    .blocking_call()?,
                Vec::new(),
            ),
            Plan::Read(len) => {
                let r = c
                    .i2c_read(addr, I2cReadKindOwned::Plain, len, None)
                    .blocking_call()?;
                (r.duration, r.data.into_inner())
            }
            Plan::WriteRead { write, read } => {
                let kind = I2cReadKindOwned::RepeatedStart {
                    write: MaxLen::new_unchecked(write),
                };
                let r = c.i2c_read(addr, kind, read, None).blocking_call()?;
                (r.duration, r.data.into_inner())
            }
            Plan::Transaction(ops) => {
                let r = c
                    .i2c_transaction(addr, MaxLen::new_unchecked(ops))
                    .blocking_call()?;
                (r.duration, r.read.into_inner())
            }
        };
        self.finish(time, &read, expected, operations)
    }

    fn finish(
        &mut self,
        time: I2cTransactionTimeRange,
        read: &[u8],
        expected: usize,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Error> {
        self.last = Some(time);
        if read.len() != expected {
            return Err(Error::ShortRead {
                got: read.len(),
                expected,
            });
        }
        plan::scatter(read, operations);
        Ok(())
    }
}

impl ErrorType for I2cMaster {
    type Error = Error;
}

impl embedded_hal::i2c::I2c<SevenBitAddress> for I2cMaster {
    fn transaction(
        &mut self,
        address: SevenBitAddress,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        self.run_blocking(Self::addr7(address)?, operations)
    }
}

impl embedded_hal::i2c::I2c<TenBitAddress> for I2cMaster {
    fn transaction(
        &mut self,
        address: TenBitAddress,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        self.run_blocking(Self::addr10(address)?, operations)
    }
}

impl embedded_hal_async::i2c::I2c<SevenBitAddress> for I2cMaster {
    async fn transaction(
        &mut self,
        address: SevenBitAddress,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        self.run(Self::addr7(address)?, operations).await
    }
}

impl embedded_hal_async::i2c::I2c<TenBitAddress> for I2cMaster {
    async fn transaction(
        &mut self,
        address: TenBitAddress,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        self.run(Self::addr10(address)?, operations).await
    }
}
