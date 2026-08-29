use crate::config::ClientConfig;
use anyhow::Result;

pub struct PreparedConnection {
    options: ClientConfig,
}

impl PreparedConnection {
    pub fn new(options: ClientConfig) -> Self {
        Self { options }
    }

    pub async fn connect(self) -> Result<()> {
        let options = self.options.validate()?;
        Ok(())
    }

    pub fn connect_blocking(self) {}

    pub fn connect_promise(self) {}
}
