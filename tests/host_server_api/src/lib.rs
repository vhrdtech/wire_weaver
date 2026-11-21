//! API of the host server tests (SRV-12, `tests/host_server`).
use ww_api::prelude::*;

#[ww_api_root]
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
