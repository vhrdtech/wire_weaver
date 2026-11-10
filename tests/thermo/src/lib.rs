//! A host driver for every wire line of `thermo_api` (EVO-28): line 1 (`thermo_api` 1.2.0 at w1.2), line 2 (2.1.0 at
//! w2.1, a wire break) and line 3 (3.0.0 at w3.0, another break), one generated client each, behind one facade,
//! [Thermo].
//!
//! - The clients are of the same crate at three versions, as Cargo renamed dependencies (`thermo_api_w1`,
//!   `thermo_api_w2`, `thermo_api_w3`), one module each (`v1.rs`, `v2.rs`, `v3.rs`).
//! - [Thermo] is an enum over the three clients, made by `wire_weaver_client::facade!` (CLT-64) from the operations
//!   below: each line implements [Line] for its client, an operation it doesn't write is [Error::Unsupported].
//! - The connection speaks every line (`ClientConfig::speaks`): link setup takes a device on any of them, and
//!   `Thermo::try_from_cmd` picks the client of the line the device reported. One handshake, no reconnect.
//! - [model] is the driver's own version-free types; each line converts to and from it.
//! - `ww_indication` is a trait of `ww_stdlib` every line implements: one trait client over global paths serves all,
//!   with no per-line code ([Thermo::indication]).
//!
//! Tests against simulated firmware of each line (`thermo_devices`) are in `tests/`.

pub mod error;
pub mod model;
pub mod v1;
pub mod v2;
pub mod v3;

pub use error::{Error, Result};

use model::{Command, Gain, Mode, Offset, Rate, Sample, Uptime};
use wire_weaver_client::Commander;

/// A stream or an observed property of any line, its items in the model's types.
pub type Updates<T> = Box<dyn wire_weaver_client::facade::Updates<T, Error>>;
/// The command sink of any line, taking the model's commands.
pub type Commands = Box<dyn wire_weaver_client::facade::Sends<Command, Error>>;

wire_weaver_client::facade! {
    /// A `thermo_api` device on any line this driver knows. Connect with
    /// `Thermo::config(|c| c.usb().serial_eq(serial)).connect()`.
    pub enum Thermo {
        V3(v3::Client),
        V2(v2::Client),
        V1(v1::Client),
    }

    /// The operations of [Thermo], implemented by each line's client.
    pub trait Line;

    error = Error;

    /// Sets rate and mode. Line 1 has no single call for it: the rate first, then the mode.
    async fn configure(&self, rate: Rate, mode: Mode);
    async fn read_once(&self) -> Sample;
    async fn mode(&self) -> Mode;
    /// [Mode::Triggered] is line 1 only: [Error::Unsupported] on lines 2 and 3.
    async fn set_mode(&self, mode: Mode);
    async fn gain(&self) -> Gain;
    /// Line 1 takes whole steps only: a fraction is [Error::NotRepresentable].
    async fn set_gain(&self, gain: Gain);
    /// Gain as the device changes it, on change.
    async fn observe_gain(&self) -> Updates<Gain>;
    async fn samples(&self) -> Updates<Sample>;
    async fn commands(&self) -> Commands;
    async fn channel_enabled(&self, channel: u32) -> bool;
    async fn set_channel_enabled(&self, channel: u32, enabled: bool);
    async fn channel_last(&self, channel: u32) -> Sample;
    /// From w1.1 on line 1, which takes 0.01 K steps: a finer offset is [Error::NotRepresentable].
    async fn calibrate(&self, offset: Offset);
    /// From w1.2 on line 1; whole seconds on lines 1 and 2, milliseconds on line 3.
    async fn uptime(&self) -> Uptime;
    /// From w2.1, lines 2 and 3.
    #[unsupported(needs = "line 2 from w2.1")]
    async fn self_test(&self);
}

impl Thermo {
    /// The status LED: `ww_indication::Indication`, the same trait and types on every line.
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
