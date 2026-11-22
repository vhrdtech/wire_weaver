//! A server generated with `serve_trait` (SRV-49): [Service] implements the local trait [Served], [server::Server]
//! has no handler written by hand, [client::Client] is the generated client implementing [Served] too. [not_send]
//! is the same without `Send` (SRV-73): state in `Rc` / `RefCell`, served on one thread.

use futures_lite::stream;
use serve_trait_api::{Gate, How, ServeError, Served, Tick};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::Notify;
use wire_weaver::CallContext;
use wire_weaver::call::{CallError, Error, ItemStream, item_stream};
use wire_weaver::ww_api::ww_error::CommonErrorOwned;

/// Gates calls wait for, the property and what happened, in order.
#[derive(Default)]
pub struct Service {
    open: Mutex<Vec<Gate>>,
    opened: Notify,
    waiting: AtomicU32,
    mode: Mutex<Option<Gate>>,
    log: Mutex<Vec<String>>,
}

impl Service {
    /// Let the calls waiting for `gate` answer.
    pub fn open(&self, gate: Gate) {
        locked(&self.open).push(gate);
        self.opened.notify_waiters();
    }

    /// How many calls wait for a gate now.
    pub fn waiting(&self) -> u32 {
        self.waiting.load(Ordering::Acquire)
    }

    /// What happened, in order.
    pub fn log(&self) -> Vec<String> {
        locked(&self.log).clone()
    }

    fn note(&self, what: String) {
        locked(&self.log).push(what);
    }
}

/// Takes `m`; a poisoned lock panics, as the thread that poisoned it did.
fn locked<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // spirit-level: allow(lock-unwrap) the crate's one place that decides poison: it panics
    m.lock()
        .expect("lock poisoned: a thread panicked while holding it")
}

/// What `fail` answers for `how`, in both forms of the trait.
pub fn failed(how: How) -> Result<Gate, CallError<ServeError>> {
    match how {
        How::Not => Ok(Gate(0)),
        How::Api => Err(CallError::Api(ServeError::Refused)),
        How::Denied => Err(Error::denied("not you").into()),
        How::Internal => Err(Error::internal("broken").into()),
        How::Busy => Err(Error::common(CommonErrorOwned::Busy).into()),
        How::Unimplemented | How::Unknown(_) => Err(Error::unimplemented("not here").into()),
    }
}

/// Counts a call in `waiting` while it waits, also when the server drops it.
struct Waiting<'a>(&'a AtomicU32);

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Served for Service {
    async fn wait(&self, _cx: &CallContext, gate: Gate) -> Result<Gate, Error> {
        self.waiting.fetch_add(1, Ordering::AcqRel);
        let _waiting = Waiting(&self.waiting);
        loop {
            let opened = self.opened.notified();
            tokio::pin!(opened);
            opened.as_mut().enable();
            if locked(&self.open).contains(&gate) {
                break;
            }
            opened.await;
        }
        self.note(format!("wait {} done", gate.0));
        Ok(gate)
    }

    async fn fail(&self, _cx: &CallContext, how: How) -> Result<Gate, CallError<ServeError>> {
        failed(how)
    }

    async fn read_mode(&self, _cx: &CallContext) -> Result<Gate, Error> {
        let mode = locked(&self.mode).unwrap_or(Gate(0));
        self.note(format!("read {}", mode.0));
        Ok(mode)
    }

    async fn write_mode(&self, _cx: &CallContext, mode: Gate) -> Result<(), Error> {
        *locked(&self.mode) = Some(mode);
        self.note(format!("write {}", mode.0));
        Ok(())
    }

    async fn ticks(
        &self,
        _cx: &CallContext,
        count: Tick,
    ) -> Result<Result<(Tick, ItemStream<Tick>), ServeError>, Error> {
        if count.0 == 0 {
            return Ok(Err(ServeError::Refused));
        }
        let items = stream::iter((0..count.0).map(|n| Ok(Tick(n))));
        Ok(Ok((count, item_stream(items))))
    }
}

/// The server: every handler generated, forwarding to the shared [Service].
pub mod server {
    use super::Service;
    use std::sync::Arc;
    use wire_weaver_server::{Backend, Connection, Pumps, Serves};

    /// One connection's server.
    pub struct Server {
        service: Arc<Service>,
        pumps: Pumps,
    }

    impl Server {
        /// The server of `connection`, forwarding to `service`.
        pub fn new(service: Arc<Service>, connection: &Connection) -> Self {
            Server {
                service,
                pumps: connection.pumps.clone(),
            }
        }
    }

    impl Serves for Server {
        type Service = Service;
        type Pumps = Pumps;

        fn service(&self) -> &Service {
            &self.service
        }

        fn pumps(&self) -> &Pumps {
            &self.pumps
        }
    }

    impl Backend for Server {}

    /// What a client checks in link setup.
    pub fn link_config() -> ww_device::LinkConfig<'static> {
        ww_device::LinkConfig::new(
            serve_trait_api::SERVE_TRAIT_FULL_GID,
            serve_trait_api::SERVE_TRAIT_WIRE_VERSION,
            api_impl::api_hash(),
            api_impl::image_facts(),
            ww_client_server::COMPACT_VERSION,
        )
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            serve_trait_api :: ServeTrait for super::Server,
            server = true,
            serve_trait = "serve_trait_api::Served",
        );
    }
}

/// The generated client, implementing [Served].
pub mod client {
    use wire_weaver_client::{ClientConfig, Commander, WwClient};

    pub struct Client {
        pub cmd: Commander,
    }

    impl WwClient for Client {
        fn default_config() -> ClientConfig {
            ClientConfig::default()
        }

        fn try_from_cmd(cmd: Commander) -> Result<Self, wire_weaver_client::Error> {
            Ok(Self { cmd })
        }
    }

    mod api_client {
        wire_weaver::ww_codegen!(
            serve_trait_api :: ServeTrait for super::Client,
            client = "std_client",
            impl_local_trait = "serve_trait_api::Served",
        );
    }
}

pub mod levels;
pub mod not_send;

#[cfg(test)]
mod levels_tests;
#[cfg(test)]
mod not_send_tests;
#[cfg(test)]
mod tests;
