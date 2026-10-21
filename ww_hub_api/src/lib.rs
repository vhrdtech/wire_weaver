#![cfg_attr(not(feature = "std"), no_std)]
//! API of the ww hub (SRV-13, SRV-28): the per-user process that holds devices (USB, RTT, ...) and serves every
//! program of the user over a local socket (`ww_local_socket`). The hub serves it like any device serves its API, so
//! introspection, versioning, the dynamic client and Python work on it unchanged.
//!
//! The crate's version is the API's identity. [Hub::info] and [Hub::handover] are frozen: a client of any version
//! uses them to find out what hub runs and to have an older one replaced (SRV-30). Everything else evolves by the
//! usual rules (`docs/evolution/rules.md`), area by area in its own sub-trait.

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait Hub {
    /// What hub runs: versions, process, socket. Frozen.
    fn info() -> HubInfo<'i>;

    /// Hand the devices over to a newer hub listening on `socket`, then exit (SRV-30). Frozen.
    fn handover(socket: &'i str) -> Result<(), HandoverError>;

    /// Exit now: clients see their tunnels close, devices are released.
    fn shutdown();

    /// Stay up at least `seconds` from now even without clients (a bench), `0` back to the idle timeout.
    fn keep(seconds: u32);

    /// Devices, clients and what happens to them.
    ww_impl!(registry: Registry);
}

/// Devices the hub knows and the programs connected to it.
#[ww_trait]
pub trait Registry {
    /// Every device seen, open or not.
    fn devices() -> RefVec<'i, DeviceEntry<'i>>;

    /// Every connected program.
    fn clients() -> RefVec<'i, ClientEntry<'i>>;

    /// Open the device now and keep it (`ww hub take`), so programs reach it through the hub from their first call.
    fn take(key: &'i str) -> Result<(), TakeError<'i>>;

    /// Plug, open, close and client changes, as they happen.
    stream!(events: Event<'i>);
}

/// See [Hub::info].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct HubInfo<'i> {
    /// `ww_hub` crate version: newer replaces older (SRV-30).
    pub hub_version: &'i str,
    /// `wire_weaver` version the hub is built with.
    pub ww_version: &'i str,
    pub pid: u32,
    /// Executable the hub runs from.
    pub exe: &'i str,
    /// Start time, ms since the Unix epoch.
    pub started_ms: u64,
    /// Socket path (Windows: the pipe's path in the runtime dir it is named after).
    pub socket: &'i str,
}

/// Why [Hub::handover] was refused.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Clone, Copy, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum HandoverError {
    /// The new hub isn't newer than this one
    NotNewer,
    /// Nothing answers on the socket given
    Unreachable,
}

/// One device, see [Registry::devices].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct DeviceEntry<'i> {
    /// Stable key: `usb:<vid>:<pid>:<serial>[:<iface>]`, `rtt:<probe serial>:<chip>`, ...; the preamble's target.
    pub key: &'i str,
    pub product: &'i str,
    pub serial: &'i str,
    /// The device's API, `crate@version`, once known.
    pub api: Option<&'i str>,
    pub state: DeviceState<'i>,
    /// Connections to it through the hub.
    pub tunnels: u32,
}

/// See [DeviceEntry::state].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq), ww_repr = unib32)]
pub enum DeviceState<'i> {
    /// Known from before, not plugged in now
    Absent,
    /// Plugged in, not opened by the hub
    Present,
    /// Opened by the hub
    Open,
    /// Another program holds it directly
    HeldElsewhere { holder: Holder<'i> },
    /// The hub's task for it failed
    Faulted { why: &'i str },
}

/// A program holding a device, from its lock file (CLT-38).
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct Holder<'i> {
    pub pid: u32,
    pub program: &'i str,
    pub exe: &'i str,
    /// `direct` or `hub`
    pub mode: &'i str,
    /// Since when, ms since the Unix epoch.
    pub since_ms: u64,
}

/// Why [Registry::take] failed.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq), ww_repr = unib32)]
pub enum TakeError<'i> {
    /// No device with this key is plugged in
    UnknownDevice,
    /// Another program holds it directly
    HeldElsewhere { holder: Holder<'i> },
    /// Opening it failed
    Failed { why: &'i str },
}

/// One connected program, see [Registry::clients]: what its link setup said about it (`ww_link::ClientMeta`, CLT-41)
/// and what the OS says.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct ClientEntry<'i> {
    /// Hub-wide id of the connection
    pub id: u32,
    /// Process id the OS reports for the other end (Linux, Windows), else the one the program sent, `0` if neither.
    pub pid: u32,
    /// Program name, as the program sent it.
    pub program: &'i str,
    /// `Cli`, `Gui`, `Notebook`, `Service`, `Hub`, `Test`, empty if not sent.
    pub kind: &'i str,
    pub user: &'i str,
    pub host: &'i str,
    /// Shared by every connection of one client object.
    pub instance_id: u64,
    /// Connected since, ms since the Unix epoch.
    pub since_ms: u64,
}

/// See [Registry::events].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq), ww_repr = unib32)]
pub enum Event<'i> {
    Plugged {
        key: &'i str,
    },
    Unplugged {
        key: &'i str,
    },
    Opened {
        key: &'i str,
    },
    Closed {
        key: &'i str,
    },
    ClientJoined {
        id: u32,
        program: &'i str,
    },
    ClientLeft {
        id: u32,
    },
    /// A program outside the hub took or released a device
    HolderChanged {
        key: &'i str,
    },
    /// This hub hands over to a newer one: reconnect (SRV-30)
    Handover,
}
