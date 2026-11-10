// Two hand-written line clients and a driver error for the `facade!` UI tests: `facade!` needs only `LineClient`.

macro_rules! line {
    ($m:ident) => {
        pub mod $m {
            use wire_weaver_client::{ClientLine, Commander, Error, WwClient, facade::LineClient};

            pub struct Client {
                cmd: Commander,
            }

            impl WwClient for Client {
                fn default_config() -> wire_weaver_client::ClientConfig {
                    wire_weaver_client::ClientConfig::default()
                }

                fn try_from_cmd(cmd: Commander) -> Result<Self, Error> {
                    Ok(Self { cmd })
                }
            }

            impl LineClient for Client {
                fn cmd(&self) -> &Commander {
                    &self.cmd
                }

                fn line() -> ClientLine {
                    unimplemented!("not connected in a UI test")
                }
            }
        }
    };
}

line!(v1);
line!(v2);

#[derive(Debug)]
pub enum Error {
    Unsupported(wire_weaver_client::facade::Unsupported),
    Stream(wire_weaver_client::StreamError),
}

impl From<wire_weaver_client::facade::Unsupported> for Error {
    fn from(e: wire_weaver_client::facade::Unsupported) -> Self {
        Error::Unsupported(e)
    }
}

impl From<wire_weaver_client::StreamError> for Error {
    fn from(e: wire_weaver_client::StreamError) -> Self {
        Error::Stream(e)
    }
}
