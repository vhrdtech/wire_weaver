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

    /// Who started the hub and when it would exit: the numbers behind `ww hub status`.
    fn status() -> HubStatus<'i>;

    /// Exit now: clients see their tunnels close, devices are released.
    fn shutdown();

    /// Stay up at least `seconds` from now even without clients (a bench), `0` back to the idle timeout.
    fn keep(seconds: u32);

    /// Devices, clients and what happens to them.
    ww_impl!(registry: Registry);

    /// Resources of a device one program has to itself for a while (SRV-34).
    ww_impl!(claims: Claims);

    /// How the hub keeps its devices' clocks on UTC (SRV-31).
    ww_impl!(sync: ClockSync);

    /// Live link handover from a program holding a device directly (CLT-44).
    ww_impl!(adopt: Adopt);
}

/// A program holding a device directly hands its live link to the hub (CLT-44, `docs/transport/hub.md`): the hub goes
/// on with the same link, so the device never sees a reconnect, and the program goes on through the hub. In two steps,
/// so that either side dying on the way leaves one working holder: until the commit the program still has the device.
#[ww_trait]
pub trait Adopt {
    /// Prepares to adopt the link `snapshot` describes (`wire_weaver_client::adopt::LinkSnapshotOwned`, internal state
    /// of the same `wire_weaver_client` major.minor): checks it, keeps others from opening the device meanwhile and
    /// reserves the program's tunnel. Answers the id for [Adopt::commit] and the tunnel's preamble. Forgotten when
    /// this connection closes before the commit.
    fn prepare(snapshot: RefVec<'i, u8>) -> Result<u64, AdoptError<'i>>;

    /// The program let go of the device: the hub opens it and goes on with the link. The program's tunnel (preamble
    /// `Device(key)` with the id, `ww_local_socket::TAG_ADOPT`) must come within 5 s, or the hub takes the program as
    /// gone.
    fn commit(id: u64) -> Result<(), AdoptError<'i>>;
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

/// Claims on resources of devices (SRV-34): while a program holds one, the hub answers other programs' calls and
/// property writes at or under the claimed path itself, with `Common(InUse)` naming the holder, without reaching the
/// device; their reads, streams and observing stay open (taken too for a `#[claim(observe)]` resource). Which
/// resources can be claimed, and how, is in the device's API (`#[claim(..)]`, `docs/api/metadata.md`); a device's
/// root always can, exclusively.
///
/// A claim belongs to the program's client instance (the instance id of its `ClientMeta`, CLT-41) as the OS user it
/// runs as: every connection of that instance, to the hub's API and to the device, may use the resource. It is
/// released by [Claims::release], when the instance's last connection closes (a crash frees it), or after its ttl.
///
/// Device-wide exclusive access (SRV-32: firmware updates, settings writes) is a claim on the root path: every other
/// program's calls and writes are refused, and the hub pauses its clock steering of the device (SRV-31) while it is
/// held. [Claims::lend] goes one step further for a tool that isn't a WireWeaver program (probe-rs, dfu-util).
#[ww_trait]
pub trait Claims {
    /// Claims the resource at `path` (resource ids from the device's root, array indices included, as in a request)
    /// of the device `key` for `purpose` (shown to others), for `ttl_s` seconds or until released.
    fn claim(
        key: &'i str,
        path: RefVec<'i, UNib32>,
        purpose: &'i str,
        ttl_s: Option<u32>,
    ) -> Result<u32, ClaimError<'i>>;

    /// Releases a claim this client instance holds. A lent device ([Claims::lend]) is returned: the hub opens it
    /// again if it held it before.
    fn release(id: u32) -> Result<(), ReleaseError>;

    /// Claims held on the device `key`.
    fn claims(key: &'i str) -> RefVec<'i, ClaimEntry<'i>>;

    /// Lends the device `key` to a program outside the hub (`ww hub lend`, SRV-32): a root claim, and the hub closes
    /// the device (its tunnels see a disconnect with `ww_local_socket::DISCONNECT_LENT`) and doesn't open it again
    /// until the claim is released, answering new tunnels `Status::Lent` meanwhile. Returns once the device is
    /// closed, so the other program can open it right after.
    fn lend(key: &'i str, purpose: &'i str, ttl_s: Option<u32>) -> Result<u32, ClaimError<'i>>;
}

/// Clock sync of the hub's devices (SRV-31): the hub steers every device it holds open that serves
/// `ww_timebase::Clock` with the `host_sync` group, wherever in its API, onto this PC's UTC (`ww_clock_sync`), and
/// keeps such a device open for [ClockSync::linger] after its last client left, so short commands don't leave it
/// coasting. Programs see the steering here instead of steering themselves (`ww sync` with a hub).
#[ww_trait]
pub trait ClockSync {
    /// Every device the hub holds open, with how its clock is steered.
    fn devices() -> RefVec<'i, SyncEntry<'i>>;

    /// Steering on (the default) or off, for the device `key` or, with `None`, for every device (a device's own
    /// setting is then dropped). A device whose steering goes off coasts in holdover.
    fn set_enabled(key: Option<&'i str>, on: bool);

    /// How long a steered device stays open after its last client left, in seconds: `Some(0)` only while clients
    /// are connected, `None` forever (a bench). The hub's default is 30 min (`WW_HUB_SYNC_LINGER`).
    property!(rw linger: Option<u32>);

    /// Each device's [SyncEntry] whenever it changes, a few times a second at most per device.
    stream!(stats: SyncEntry<'i>);
}

/// How one device's clock is steered, see [ClockSync::devices].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct SyncEntry<'i> {
    /// The device's key, as in [DeviceEntry::key].
    pub key: &'i str,
    pub phase: SyncPhase<'i>,
    /// The device's sync state at the last check (`Acquiring`, `Locked`, `Holdover`, ...), `None` before one.
    pub state: Option<ww_timebase::SyncState>,
    /// The source the device follows, as it answered the last steer point.
    pub source: Option<ww_timebase::SyncSource>,
    /// The host whose steer points the device follows, by its own word: the hub itself, or a program on another PC
    /// or with another link to the device; `None` when the device doesn't tell.
    pub steerer: Option<ww_timebase::Steerer<'i>>,
    /// Another steerer the device only notes.
    pub noted: Option<ww_timebase::Steerer<'i>>,
    /// The device follows the hub's points; `None` when it doesn't tell.
    pub followed: Option<bool>,
    /// The device clock's rate against this PC's, in ppb (negative: it runs fast).
    pub rate_ppb: Option<i32>,
    /// The rate came from the rate cache (this PC measured it before), not yet from this connection's readings.
    pub rate_from_cache: bool,
    /// Shortest round trip of the readings in the fit, ns.
    pub min_rtt_ns: Option<u64>,
    /// The device's shared time minus this PC's UTC at the last check (`shared_now`), ns, good to half of
    /// `check_rtt_ns`.
    pub residual_ns: Option<i64>,
    /// Round trip of the last check, ns.
    pub check_rtt_ns: Option<u64>,
    /// The device's own bound of its error against UTC, µs.
    pub error_us: Option<u32>,
    /// Offset the device still slews in, µs.
    pub slew_us: Option<i32>,
    /// Readings in the fit's window.
    pub samples: u32,
    /// Steer points the device answered.
    pub steers: u64,
    /// The device restarted (its clock went backwards) this often while steered.
    pub restarts: u32,
    /// The last error of a call, `None` once a call works again.
    pub error: Option<&'i str>,
}

/// What the hub does with a device's clock, see [SyncEntry::phase].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq), ww_repr = unib32)]
pub enum SyncPhase<'i> {
    /// Looking at the device's API
    Starting,
    /// Steering it
    Steering,
    /// Steering is off for it (`ClockSync::set_enabled`, `WW_HUB_SYNC=off`)
    Off,
    /// Paused while a program has the whole device to itself (SRV-32)
    Paused,
    /// The device has no `ww_timebase::Clock` with `host_sync`
    Unsupported,
    /// Steering failed to start
    Failed { why: &'i str },
}

/// Why [Adopt::prepare] or [Adopt::commit] failed: the program goes on with the device itself, or hands it over by
/// releasing it (CLT-43).
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq), ww_repr = unib32)]
pub enum AdoptError<'i> {
    /// The hub is built with another `wire_weaver_client` minor (its version)
    Versions { hub: &'i str },
    /// The snapshot can't be adopted
    Snapshot { why: &'i str },
    /// The hub holds the device already, it is lent, or another adoption of it is under way
    Busy,
    /// No adoption with this id was prepared on this connection
    UnknownId,
    /// The hub couldn't open the device
    Open { why: &'i str },
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

/// See [Hub::status].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct HubStatus<'i> {
    /// Program (and pid) that started the hub, as the starter said (`WW_HUB_STARTED_BY`) or the parent process's name;
    /// empty if unknown.
    pub started_by: &'i str,
    /// Idle exit, seconds without connections.
    pub idle_exit_s: u32,
    /// Seconds until the hub exits if nothing connects, `None` while a program is connected or the hub is kept.
    pub exits_in_s: Option<u32>,
    /// Seconds left of `Hub::keep`, `None` if not kept.
    pub keep_left_s: Option<u32>,
    /// Open connections of any kind (programs on the hub API and on devices).
    pub connections: u32,
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
    /// Where it is connected, e.g. `usb 3-1.2 c0de:cafe`; `None` if unknown (a device known from its lock file only).
    #[default = None]
    pub location: Option<&'i str>,
    /// User assigned label, `None` if not set.
    #[default = None]
    pub label: Option<&'i str>,
    /// Truncated API hash without docs, `None` if not reported.
    #[default = None]
    pub api_hash: Option<&'i str>,
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

/// What a claim takes from other programs, as the device's API says (`#[claim(..)]`).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Clone, Copy, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum ClaimKind {
    /// One holder: others' calls and writes are refused
    Exclusive,
    /// Several holders together: calls and writes of programs without a claim are refused
    Shared,
    /// One holder: others' calls, writes, reads, streams and observing are refused
    Observe,
}

/// One claim, see [Claims::claims].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct ClaimEntry<'i> {
    pub id: u32,
    pub key: &'i str,
    pub path: RefVec<'i, UNib32>,
    pub kind: ClaimKind,
    /// The resource's `#[label]`, empty without one.
    pub label: &'i str,
    pub purpose: &'i str,
    /// The connection that claimed it (its instance's other connections share the claim).
    pub holder: ClientEntry<'i>,
    /// Claimed at, ms since the Unix epoch.
    pub since_ms: u64,
    /// Released by itself then, ms since the Unix epoch.
    pub expires_ms: Option<u64>,
    /// The device is lent out with it ([Claims::lend]): closed by the hub until the claim is released.
    #[default = false]
    pub lent: bool,
}

/// Why [Claims::claim] failed.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq), ww_repr = unib32)]
pub enum ClaimError<'i> {
    /// No device with this key, or it can't be opened
    UnknownDevice,
    /// The device's API isn't known (introspection disabled or failed): only its root can be claimed
    ApiUnknown,
    /// The path names no resource of the device's API
    BadPath,
    /// The resource isn't marked `#[claim]` in the device's API
    NotClaimable,
    /// Another program holds a claim on this resource
    InUse { claim: ClaimEntry<'i> },
    /// Another program holds a claim on a resource above or below this one, or one sharing its pins or peripherals
    Overlap { claim: ClaimEntry<'i> },
}

/// Why [Claims::release] failed.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Clone, Copy, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum ReleaseError {
    /// No such claim, or it was released already
    UnknownClaim,
    /// The claim belongs to another program
    NotHolder,
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
    /// A resource was claimed (SRV-34)
    Claimed {
        key: &'i str,
        id: u32,
    },
    /// A claim was released, by its holder, its holder leaving or its ttl
    Released {
        key: &'i str,
        id: u32,
    },
    /// The device was lent to a program outside the hub with the claim `id` (SRV-32), and closed
    Lent {
        key: &'i str,
        id: u32,
    },
    /// A lent device was returned: the hub opens it again on demand (at once if it held it before)
    Returned {
        key: &'i str,
    },
}
