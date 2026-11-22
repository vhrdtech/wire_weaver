//! A keyed array whose key borrows (API-72) through the local trait: the generated client implements `Dissect`, and a
//! `serve_trait` server serves that client again, so every key crosses the trait as its owned form.

/// `Dissect` and `FlowOps`, the local traits of the Dissector and its flows.
pub mod traits {
    wire_weaver::ww_codegen!(keyed_api::Dissector, local_trait = "Dissect");
}

/// The generated client, implementing [traits::Dissect].
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
            keyed_api :: Dissector for super::Client,
            client = "std_client",
            impl_local_trait = "crate::local::traits::Dissect",
            debug_to_file = "../../target/tests_keyed_local_client.rs"
        );
    }
}

/// A server forwarding every request to a [client::Client] of another device (`serve_trait`).
pub mod server {
    use super::client::Client;
    use std::sync::Arc;
    use wire_weaver_server::{Backend, Connection, Pumps, Serves};

    /// One connection's server.
    pub struct Server {
        upstream: Arc<Client>,
        pumps: Pumps,
    }

    impl Server {
        /// The server of `connection`, forwarding to `upstream`.
        pub fn new(upstream: Arc<Client>, connection: &Connection) -> Self {
            Server {
                upstream,
                pumps: connection.pumps.clone(),
            }
        }
    }

    impl Serves for Server {
        type Service = Client;
        type Pumps = Pumps;

        fn service(&self) -> &Client {
            &self.upstream
        }

        fn pumps(&self) -> &Pumps {
            &self.pumps
        }
    }

    impl Backend for Server {}

    /// What a client checks in link setup.
    pub fn link_config() -> ww_device::LinkConfig<'static> {
        ww_device::LinkConfig::new(
            keyed_api::DISSECTOR_FULL_GID,
            keyed_api::DISSECTOR_WIRE_VERSION,
            api_impl::api_hash(),
            api_impl::image_facts(),
            ww_client_server::COMPACT_VERSION,
        )
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            keyed_api :: Dissector for super::Server,
            server = true,
            serve_trait = "crate::local::traits::Dissect",
            debug_to_file = "../../target/tests_keyed_serve_trait.rs"
        );
    }
}
