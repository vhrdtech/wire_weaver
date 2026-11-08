//! A host driver for every wire line of `thermo_api` (EVO-28): line 1 (`thermo_api` 1.2.0 at w1.2) and line 2
//! (2.1.0 at w2.1, a wire break), one generated client each, behind one facade, [Thermo].
//!
//! - Both clients are of the same crate at two versions, as Cargo renamed dependencies (`thermo_api_w1`,
//!   `thermo_api_w2`), one module each (`v1.rs`, `v2.rs`).
//! - The connection speaks both lines ([ClientConfig::speaks]): link setup takes a device on either, and
//!   [Thermo::from_cmd] picks the client of the line the device reported. One handshake, no reconnect.
//! - [model] is the driver's own version-free types; each line converts to and from it.
//! - `ww_indication` is a trait of `ww_stdlib` both lines implement: one trait client over global paths serves both,
//!   with no per-line code ([Thermo::indication]).
//!
//! Tests against simulated firmware of each line are in `tests/`.

mod backend;
pub mod error;
pub mod model;
mod v1;
mod v2;

pub use backend::{Commands, Updates};
pub use error::{Error, Result};

use backend::Backend;
use model::{Gain, Mode, Offset, Rate, Sample, Uptime};
use wire_weaver::ww_version::WireVersion;
use wire_weaver_client::{ClientConfig, Commander, PreparedConnection, WwClient};

/// A `thermo_api` device on any line this driver knows.
pub struct Thermo {
    backend: Box<dyn Backend>,
}

impl WwClient for Thermo {
    /// Every line this driver has a client of, the newest first.
    fn default_config() -> ClientConfig {
        ClientConfig::default()
            .speaks(v2::Client::client_line())
            .speaks(v1::Client::client_line())
    }

    fn from_cmd(cmd: Commander) -> Self {
        let wire = cmd.device_api_info().user_api_wire;
        let backend: Box<dyn Backend> = match wire.map(|w| w.line.0) {
            Some(1) => Box::new(v1::Client::from_cmd(cmd)),
            Some(2) => Box::new(v2::Client::from_cmd(cmd)),
            // link setup refuses a device on any other line, see `default_config`
            _ => unreachable!("a thermo_api device at wire {wire:?} got through link setup"),
        };
        Thermo { backend }
    }
}

impl Thermo {
    /// Connect to a device of any line this driver knows; pass the device selection in `c`, e.g.
    /// `Thermo::config(|c| c.usb().serial_eq(serial))`.
    pub fn config(c: impl Fn(ClientConfig) -> ClientConfig) -> PreparedConnection<Self> {
        PreparedConnection::new(c(Self::default_config()))
    }

    /// The device's wire version: which line, which revision.
    pub fn wire(&self) -> WireVersion {
        self.backend.wire()
    }

    pub fn cmd(&self) -> &Commander {
        self.backend.cmd()
    }

    /// Sets rate and mode. Line 1 has no single call for it: the rate first, then the mode.
    pub async fn configure(&self, rate: Rate, mode: Mode) -> Result<()> {
        self.backend.configure(rate, mode).await
    }

    pub async fn read_once(&self) -> Result<Sample> {
        self.backend.read_once().await
    }

    pub async fn mode(&self) -> Result<Mode> {
        self.backend.mode().await
    }

    /// [Mode::Triggered] is line 1 only: [Error::Unsupported] on line 2.
    pub async fn set_mode(&self, mode: Mode) -> Result<()> {
        self.backend.set_mode(mode).await
    }

    pub async fn gain(&self) -> Result<Gain> {
        self.backend.gain().await
    }

    /// Line 1 takes whole steps only: a fraction is [Error::NotRepresentable].
    pub async fn set_gain(&self, gain: Gain) -> Result<()> {
        self.backend.set_gain(gain).await
    }

    /// Gain as the device changes it, on change.
    pub async fn observe_gain(&self) -> Result<Box<dyn Updates<Gain>>> {
        self.backend.observe_gain().await
    }

    pub async fn samples(&self) -> Result<Box<dyn Updates<Sample>>> {
        self.backend.samples().await
    }

    pub async fn commands(&self) -> Result<Box<dyn Commands>> {
        self.backend.commands().await
    }

    pub async fn channel_enabled(&self, channel: u32) -> Result<bool> {
        self.backend.channel_enabled(channel).await
    }

    pub async fn set_channel_enabled(&self, channel: u32, enabled: bool) -> Result<()> {
        self.backend.set_channel_enabled(channel, enabled).await
    }

    pub async fn channel_last(&self, channel: u32) -> Result<Sample> {
        self.backend.channel_last(channel).await
    }

    /// From w1.1 on line 1, which takes 0.01 K steps: a finer offset is [Error::NotRepresentable].
    pub async fn calibrate(&self, offset: Offset) -> Result<()> {
        self.backend.calibrate(offset).await
    }

    /// From w1.2 on line 1.
    pub async fn uptime(&self) -> Result<Uptime> {
        self.backend.uptime().await
    }

    /// From w2.1, line 2 only.
    pub async fn self_test(&self) -> Result<()> {
        self.backend.self_test().await
    }

    /// The status LED: `ww_indication::Indication`, the same trait and types on both lines.
    pub fn indication(&self) -> Indication {
        Indication {
            cmd: self.cmd().clone(),
        }
    }
}

/// `ww_indication::Indication` wherever the device has it, through global paths: one client for every line.
pub struct Indication {
    cmd: Commander,
}

mod indication_client {
    use super::Indication;
    wire_weaver::ww_codegen!(
        ww_indication :: Indication for Indication,
        client = "trait_client",
        no_alloc = false,
        use_async = true,
    );
}
