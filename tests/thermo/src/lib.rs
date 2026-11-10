//! A host driver for every wire line of `thermo_api` (EVO-28, EVO-42): line 1 (`wire_v1`, w1.2), line 2 (`wire_v2`,
//! w2.1, a wire break) and line 3 (`wire_v3`, w3.0, another break), one generated client each, behind one facade,
//! [Thermo].
//!
//! - The lines are roots of one API crate, `thermo_api` 4.0.0, one module each (EVO-37); their clients are one module
//!   each here (`v1.rs`, `v2.rs`, `v3.rs`). What lines share is one type in `thermo_api::common`, converted once
//!   (`common.rs`).
//! - [Thermo] is an enum over the three clients, made by `wire_weaver_client::facade!` (CLT-64) from the operations
//!   below: each line implements its trait in [line](mod@line) for its client; `self_test`, which line 1 lacks, is
//!   [Error::Unsupported] there.
//! - The connection speaks every line (`ClientConfig::speaks`): link setup takes a device on any of them, and
//!   `Thermo::try_from_cmd` picks the client of the line the device reported. After `disconnect().keep_streams()`
//!   or a lost link, `Thermo::reconnect()` picks the client again (CLT-63): a DFU may bring the device back on
//!   another line this facade also speaks.
//! - [model] is the driver's own version-free types; each line converts to and from it.
//! - `ww_indication` is a trait of `ww_stdlib` every line has: one client type, [IndicationClient], generated once and
//!   returned by every line's client (CLT-68), reached by its path in the device's line ([Thermo::indication]).
//!
//! Tests against simulated firmware of each line (`thermo_devices`) are in `tests/`.

mod common;
pub mod error;
pub mod model;
pub mod v1;
pub mod v2;
pub mod v3;

pub use error::{Error, Result};

use model::{Command, Gain, Mode, Offset, Rate, Sample, Uptime};

wire_weaver_client::facade! {
    /// A `thermo_api` device on any line this driver knows. Connect with
    /// `Thermo::config(|c| c.usb().serial_eq(serial)).connect()`.
    pub enum Thermo {
        V3(v3::Client),
        V2(v2::Client),
        V1(v1::Client),
    }

    /// The operations of [Thermo], a trait per line, implemented by that line's client.
    pub mod line;

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
    async fn observe_gain(&self) -> stream GainUpdates<Gain>;
    /// Samples as the device takes them; line 3 sends a batch, given here one sample at a time.
    async fn samples(&self) -> stream Samples<Sample>;
    async fn commands(&self) -> sink Commands<Command>;
    async fn channel_enabled(&self, channel: u32) -> bool;
    async fn set_channel_enabled(&self, channel: u32, enabled: bool);
    async fn channel_last(&self, channel: u32) -> Sample;
    /// From w1.1 on line 1, which takes 0.01 K steps: a finer offset is [Error::NotRepresentable].
    async fn calibrate(&self, offset: Offset);
    /// From w1.2 on line 1; whole seconds on lines 1 and 2, milliseconds on line 3.
    async fn uptime(&self) -> Uptime;
    /// From w2.1, lines 2 and 3.
    #[unsupported(V1, needs = "line 2 from w2.1")]
    async fn self_test(&self);
}

impl Thermo {
    /// The status LED: `ww_indication::Indication`, the same trait and types on every line, one client type.
    pub fn indication(&self) -> IndicationClient<'_> {
        match self {
            Thermo::V3(c) => c.indication(),
            Thermo::V2(c) => c.indication(),
            Thermo::V1(c) => c.indication(),
        }
    }
}

mod indication_client {
    wire_weaver::ww_codegen!(ww_indication :: Indication for IndicationClient, client = "sub_level");
}
/// `ww_indication::Indication`, generated once for every line: each line's client returns it, reached by the path the
/// trait has in that line's tree (CLT-68).
pub use indication_client::IndicationClient;
