//! API of the host server tests (SRV-12, `tests/host_server`).
use ww_api::prelude::*;

#[ww_api_root]
trait HostServer {
    /// Answers `x`
    fn echo(x: u32) -> u32;
    /// Answers `gate` once the test opened it
    fn wait(gate: u8) -> u32;
    /// The id of the connection the call came on
    fn whoami() -> u32;
    /// Sends 0..`count` on `numbers`, then answers `count`
    fn burst(count: u32) -> u32;
    /// `len` bytes
    fn big(len: u32) -> RefVec<'i, u8>;
    /// See `burst`
    stream!(numbers: u32);
}
