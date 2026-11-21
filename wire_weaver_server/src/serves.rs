//! What a server generated with `ww_codegen!(.., serve_trait = "..")` forwards to (SRV-49).

use crate::Pumps;

/// A server whose handlers forward to the API's local trait: `ww_codegen!(api :: Api for Server, server = true,
/// serve_trait = "api::Name")` generates them, each calling `Name` on [service](Self::service).
///
/// The service is usually one `Arc` that every connection's server shares, the same value in-process callers use;
/// the pumps are the connection's ([Connection::pumps](crate::Connection::pumps)), where stream opens add the
/// trait's item streams.
///
/// ```ignore
/// pub struct CounterServer {
///     service: Arc<LocalCounter>,
///     pumps: Pumps,
/// }
///
/// impl Serves for CounterServer {
///     type Service = LocalCounter;
///
///     fn service(&self) -> &LocalCounter {
///         &self.service
///     }
///
///     fn pumps(&self) -> &Pumps {
///         &self.pumps
///     }
/// }
///
/// impl Backend for CounterServer {}
/// ```
pub trait Serves {
    /// What the handlers call: implements the API's local trait.
    type Service: ?Sized;

    /// The service of this connection's server.
    fn service(&self) -> &Self::Service;

    /// The connection's pumps.
    fn pumps(&self) -> &Pumps;
}
