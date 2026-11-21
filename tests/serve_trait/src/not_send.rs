//! A service whose state is `Rc` / `RefCell`, implementing [LocalServed], the trait without `Send` (SRV-73), served
//! with [LocalPumps](wire_weaver_server::LocalPumps) and called through the generated client.

use futures_lite::{StreamExt, stream};
use serve_trait_api::{Gate, How, LocalServed, ServeError, Tick};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use wire_weaver::CallContext;
use wire_weaver::call::{CallError, Error, LocalItemStream, local_item_stream};

/// The property, what happened in order and how many stream items were made, none of it `Send`.
#[derive(Default)]
pub struct LocalService {
    mode: RefCell<Option<Gate>>,
    log: RefCell<Vec<String>>,
    /// Shared with the item streams, so they are not `Send` either.
    ticks_made: Rc<Cell<u32>>,
}

impl LocalService {
    /// What happened, in order.
    pub fn log(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    /// How many stream items were made, by every stream.
    pub fn ticks_made(&self) -> u32 {
        self.ticks_made.get()
    }
}

impl LocalServed for LocalService {
    async fn wait(&self, _cx: &CallContext, gate: Gate) -> Result<Gate, Error> {
        // an `Rc` held across an await: the future is not `Send`
        let made = self.ticks_made.clone();
        futures_lite::future::yield_now().await;
        self.log
            .borrow_mut()
            .push(format!("wait {} after {} ticks", gate.0, made.get()));
        Ok(gate)
    }

    async fn fail(&self, _cx: &CallContext, how: How) -> Result<Gate, CallError<ServeError>> {
        crate::failed(how)
    }

    async fn read_mode(&self, _cx: &CallContext) -> Result<Gate, Error> {
        let mode = self.mode.borrow().unwrap_or(Gate(0));
        self.log.borrow_mut().push(format!("read {}", mode.0));
        Ok(mode)
    }

    async fn write_mode(&self, _cx: &CallContext, mode: Gate) -> Result<(), Error> {
        *self.mode.borrow_mut() = Some(mode);
        self.log.borrow_mut().push(format!("write {}", mode.0));
        Ok(())
    }

    async fn ticks(
        &self,
        _cx: &CallContext,
        count: Tick,
    ) -> Result<Result<(Tick, LocalItemStream<Tick>), ServeError>, Error> {
        if count.0 == 0 {
            return Ok(Err(ServeError::Refused));
        }
        let made = self.ticks_made.clone();
        let items = stream::iter(0..count.0).map(move |n| {
            made.set(made.get() + 1);
            Ok(Tick(n))
        });
        Ok(Ok((count, local_item_stream(items))))
    }
}

/// The server: every handler generated, forwarding to the shared [LocalService].
pub mod server {
    use super::LocalService;
    use std::rc::Rc;
    use wire_weaver_server::{Backend, LocalConnection, LocalPumps, Serves};

    /// One connection's server.
    pub struct Server {
        service: Rc<LocalService>,
        pumps: LocalPumps,
    }

    impl Server {
        /// The server of `connection`, forwarding to `service`.
        pub fn new(service: Rc<LocalService>, connection: &LocalConnection) -> Self {
            Server {
                service,
                pumps: connection.pumps.clone(),
            }
        }
    }

    impl Serves for Server {
        type Service = LocalService;
        type Pumps = LocalPumps;

        fn service(&self) -> &LocalService {
            &self.service
        }

        fn pumps(&self) -> &LocalPumps {
            &self.pumps
        }
    }

    impl Backend for Server {}

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            serve_trait_api :: ServeTrait for super::Server,
            server = true,
            serve_trait = "serve_trait_api::LocalServed",
        );
    }
}

/// The generated client, implementing [LocalServed].
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
            impl_local_trait = "serve_trait_api::LocalServed",
            local_trait_send = false,
        );
    }
}
