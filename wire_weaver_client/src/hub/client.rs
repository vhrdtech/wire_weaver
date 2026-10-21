//! A typed client of the hub's own API (`ww_hub_api`), for `connect()`'s routing, `ww hub` and programs that look
//! at the hub.
//!
//! ```ignore
//! let hub = HubClient::connect().await?;
//! let info = hub.info().call().await?;
//! let devices = hub.registry().devices().call().await?;
//! ```

use crate::{ClientConfig, Commander, WwClient};

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
    /// Connects to the user's hub, which must run already (see [super::ensure_running] to start one).
    pub async fn connect() -> anyhow::Result<HubClient> {
        HubClient::new().connect().await
    }

    pub fn cmd(&self) -> &Commander {
        &self.cmd
    }
}

mod api_client {
    wire_weaver::ww_codegen!(
        ww_hub_api :: Hub for crate::hub::HubClient,
        client = "std_client",
    );
}
