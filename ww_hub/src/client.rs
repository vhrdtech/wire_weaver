//! A typed client of the hub's API (`ww_hub_api`), for `ww hub`, tests and programs that look at the hub.
//!
//! ```ignore
//! let hub = HubClient::connect().await?;
//! let info = hub.info().call().await?;
//! ```

use wire_weaver_client::{ClientConfig, Commander, WwClient};

pub struct HubClient {
    cmd: Commander,
}

impl WwClient for HubClient {
    /// The hub of this user, at its socket in the runtime dir.
    fn default_config() -> ClientConfig {
        ClientConfig::new().hub_api()
    }

    fn from_cmd(cmd: Commander) -> Self {
        HubClient { cmd }
    }
}

impl HubClient {
    pub fn cmd(&self) -> &Commander {
        &self.cmd
    }
}

mod api_client {
    wire_weaver::ww_codegen!(
        ww_hub_api :: Hub for crate::client::HubClient,
        client = "std_client",
    );
}
