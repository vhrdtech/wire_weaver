//! A WireWeaver connection on a byte stream: the opened stream with its preamble answered, its message halves, and
//! what a refused preamble means for this program.

use ww_byte_stream::{Answer, MessageReader, MessageWriter, Status, Version};
use ww_local_socket::{ReadHalf, WriteHalf};

use crate::event_loop::transport::{MessageRx, MessageTx, SharedClaim, handover_requested};

impl Tunnel {
    /// Its message halves, sharing `claim`, taking messages up to `max_message_len`.
    #[cfg_attr(
        not(feature = "hub"),
        allow(dead_code, reason = "only used with the `hub` feature")
    )]
    pub(crate) fn into_halves(
        self,
        claim: SharedClaim,
        max_message_len: usize,
    ) -> (LocalTx, LocalRx) {
        halves((self.rx, self.tx), claim, max_message_len)
    }
}

/// The message halves of an opened connection, sharing `claim`, taking messages up to `max_message_len`.
pub(crate) fn halves(
    (rx, tx): (ReadHalf, WriteHalf),
    claim: SharedClaim,
    max_message_len: usize,
) -> (LocalTx, LocalRx) {
    (
        LocalTx(MessageWriter::new(tx), claim.clone()),
        // + kind byte, messages above what the host advertised in LinkSetup are an error anyway
        LocalRx(MessageReader::new(rx, max_message_len), claim),
    )
}

/// A connection whose preamble was answered `Ok`: ready to carry the link.
pub(crate) struct Tunnel {
    pub(crate) rx: ReadHalf,
    pub(crate) tx: WriteHalf,
    /// The socket, for messages and the tap
    pub(crate) shown: String,
    /// What the hub (or the serving process) says its version is
    #[cfg_attr(
        not(feature = "hub"),
        allow(dead_code, reason = "only used with the `hub` feature")
    )]
    pub(crate) hub_version: String,
    /// Held for a device that takes one host at a time ([ClientConfig::ipc_exclusive](
    /// crate::ClientConfig::ipc_exclusive)), for as long as the link lives
    pub(crate) claim: Option<crate::holders::Claim>,
}

/// Why [open] failed.
#[derive(Debug)]
pub(crate) enum OpenError {
    /// Nothing answers there, or the connection broke
    Io(String),
    /// The other side answered, with a status but `Ok`
    Refused { shown: String, answer: Answer },
}

impl OpenError {
    /// As the client's error: a refusal is [Error::HubRefused](crate::Error::HubRefused).
    pub(crate) fn into_error(self) -> anyhow::Error {
        match self {
            OpenError::Io(e) => anyhow::anyhow!(e),
            OpenError::Refused { shown, answer } => refusal(shown, answer).into(),
        }
    }
}

/// What a refusal means for this program: for versions the hub doesn't serve (SRV-30), both sides' and the fix.
pub(crate) fn refusal(target: String, answer: Answer) -> crate::Error {
    let why = match answer.status {
        Status::Versions => versions_not_served(&answer.hub_version),
        status => status.to_string(),
    };
    crate::Error::HubRefused {
        target,
        status: answer.status.to_u8(),
        why,
        hub_version: answer.hub_version,
    }
}

/// The hub `hub_version` doesn't serve this program's versions: which side is older, and what to do.
fn versions_not_served(hub_version: &str) -> String {
    let (link, api_model) = own_versions();
    let ours = env!("CARGO_PKG_VERSION");
    let fix = match (
        semver::Version::parse(hub_version),
        semver::Version::parse(ours),
    ) {
        (Ok(hub), Ok(ours)) if hub < ours => format!(
            "the hub is older than this program (WireWeaver {ours}): stop it (`ww hub stop`) and let a newer one \
             start (install `ww` {ours} or newer: cargo install wire_weaver_cli), or connect directly with \
             WW_HUB=off while it is stopped"
        ),
        _ => format!(
            "rebuild this program with WireWeaver {hub_version} or newer (it is built with {ours}), or stop the \
             hub (`ww hub stop`) and connect directly with WW_HUB=off while it is stopped"
        ),
    };
    format!(
        "the ww hub {hub_version} doesn't serve this program's ww_link {link} and ww_client_server {api_model}: {fix}"
    )
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::Io(e) => f.write_str(e),
            OpenError::Refused { shown, answer } => write!(
                f,
                "{shown}: {} (served by version {})",
                answer.status, answer.hub_version
            ),
        }
    }
}

/// This program's `ww_link` and `ww_client_server` versions, for the preamble (SRV-30).
pub(crate) fn own_versions() -> (Version, Version) {
    let v = |v: ww_version::CompactVersion| Version::new(v.major.0, v.minor.0, v.patch.0);
    (
        v(ww_link::LINK_VERSION),
        v(ww_client_server::COMPACT_VERSION),
    )
}

/// This program's generations, for the preamble (EVO-23): the same bytes as in its link hello.
pub(crate) fn own_generations() -> ww_byte_stream::Generations {
    let g = crate::event_loop::core::GENERATIONS;
    ww_byte_stream::Generations {
        framer: g.framer,
        link: g.link,
        shrink_wrap: g.shrink_wrap,
        api_model: g.api_model,
    }
}

/// The claim is shared by both halves: released once both closed the socket.
pub(crate) struct LocalTx(MessageWriter<WriteHalf>, SharedClaim);

impl MessageTx for LocalTx {
    async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), String> {
        self.0.push(kind, message);
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), String> {
        self.0.flush().await.map_err(|e| e.to_string())
    }

    async fn handover_requested(&mut self) {
        handover_requested(&self.1).await
    }

    async fn close(mut self) {
        let _ = self.0.flush().await;
        let _ = self.0.close().await;
    }
}

pub(crate) struct LocalRx(
    MessageReader<ReadHalf>,
    // held, never read: the claim goes with the last half
    #[allow(
        dead_code,
        reason = "held, never read: the claim goes with the last half"
    )]
    SharedClaim,
);

impl MessageRx for LocalRx {
    async fn read_message(&mut self) -> Result<(u8, &[u8]), String> {
        // cancel-safe: a partly received message stays in the reader
        self.0.read_message().await.map_err(|e| match e.kind() {
            std::io::ErrorKind::UnexpectedEof => {
                "local socket closed by the other side".to_string()
            }
            _ => e.to_string(),
        })
    }
}
