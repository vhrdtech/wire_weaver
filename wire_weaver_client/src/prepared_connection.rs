use crate::options::Options;
use anyhow::Result;

pub struct PreparedConnection {
    options: Options,
}

impl PreparedConnection {
    pub fn new(options: Options) -> Self {
        Self { options }
    }

    pub async fn connect(self) -> Result<()> {
        let options = self.options.validate()?;
        Ok(())
    }

    pub fn connect_blocking(self) {}

    pub fn connect_promise(self) {}
}
