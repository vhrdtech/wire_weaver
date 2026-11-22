//! API of the host server tests (SRV-12, `tests/host_server`).
use ww_api::prelude::*;

#[ww_api_root]
#[effect(edit)]
trait HostServer {
    /// Answers `x`
    fn echo(x: u32) -> u32;
    /// Answers `gate` once the test opened it; runs beside other calls
    fn wait(gate: u8) -> u32;
    /// Answers `gate` once the test opened it; runs alone
    fn hold(gate: u8) -> u32;
    /// The id of the connection the call came on
    fn whoami() -> u32;
    /// The connection id the handler reads from its `CallContext`
    fn call_connection() -> u32;
    /// The uid of the peer the handler reads from its `CallContext`, 0 if it has none
    fn call_uid() -> u32;
    /// What was left of the caller's deadline when the handler started, from its `CallContext` (SRV-47); `None`
    /// without one
    fn call_deadline() -> Option<ww_si::Millis<u32>>;
    /// The first byte of the caller's trace id, from its `CallContext` (SRV-47): a raw byte of the W3C id, `None`
    /// without a trace
    fn call_trace() -> Option<u8>;
    /// Sends 0..`count` on `numbers`, then answers `count`
    fn burst(count: u32) -> u32;
    /// `len` bytes
    fn big(len: u32) -> RefVec<'i, u8>;
    /// See `burst`
    stream!(numbers: u32);
    /// What the service tells every client that listens
    stream!(news: u32);
    /// One value for all clients: a write by one reaches the observers among the others
    property!(rw+observe mode: u8);
}
