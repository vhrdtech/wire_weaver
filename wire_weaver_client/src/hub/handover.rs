//! The asker's side of a polite handover (CLT-43): a program that wants a device another program holds directly asks
//! that holder, on the socket its lock file names, to hand the device to the ww hub, then reaches it through the hub.
//! The holder's side is `holders::handover`.

use std::path::Path;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tracing::debug;
use wire_weaver::shrink_wrap::{DeserializeShrinkWrapOwned, SerializeShrinkWrapOwned};
use ww_local_socket::{HandoverAnswer, HandoverRequest, HandoverStatus, Op};

use crate::client_meta::ClientMetaOwned;
use crate::holders::{self, Holder, Mode};

/// The holder must answer within this time: it lets go of the device first (its own limit is 5 s).
const ANSWER_TIMEOUT: Duration = Duration::from_secs(8);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// What asking the holder of a device gave.
#[derive(Debug)]
pub(crate) enum Asked {
    /// The holder let go of the device: the hub can open it now.
    Released(Box<Holder>),
    /// The holder doesn't hand its device over; what it says about itself, if it said it.
    Refused(Box<Holder>, Option<Box<ClientMetaOwned>>),
    /// The holder couldn't be asked or didn't let go.
    Failed(Box<Holder>, String),
}

/// The program holding `key` directly (the lock file's key may carry the USB interface number) and offering a
/// handover, if any. A holder without a socket (a build before CLT-43, or without `local_socket`) isn't asked: the
/// open then fails as busy, naming it.
pub(crate) fn direct_holder(key: &str) -> Option<Holder> {
    holders::list()
        .into_iter()
        .find(|(k, h)| super::route::same_device(k, key) && h.mode == Mode::Direct)
        .map(|(_, h)| h)
        .filter(|h| !h.handover.is_empty())
}

/// Asks `holder` to hand its device to the hub; `me` goes along for its log.
pub(crate) async fn ask(holder: Holder, me: Option<&ClientMetaOwned>) -> Asked {
    let holder = Box::new(holder);
    match ask_inner(Path::new(&holder.handover), me).await {
        Ok(answer) => {
            let meta = ClientMetaOwned::from_ww_bytes_owned(&answer.meta)
                .ok()
                .map(Box::new);
            match answer.status {
                HandoverStatus::Released => Asked::Released(holder),
                HandoverStatus::Refused => Asked::Refused(holder, meta),
                HandoverStatus::Failed => {
                    Asked::Failed(holder, "it didn't let go of the device in time".into())
                }
                HandoverStatus::Other(s) => {
                    Asked::Failed(holder, format!("it answered status {s}"))
                }
            }
        }
        Err(e) => Asked::Failed(holder, e),
    }
}

async fn ask_inner(socket: &Path, me: Option<&ClientMetaOwned>) -> Result<HandoverAnswer, String> {
    let (stream, _) = tokio::time::timeout(CONNECT_TIMEOUT, ww_local_socket::connect(socket))
        .await
        .map_err(|_| "its socket doesn't answer".to_string())?
        .map_err(|e| format!("its socket: {e}"))?;
    let (mut rx, mut tx) = interprocess::local_socket::traits::tokio::Stream::split(stream);
    let meta = me
        .and_then(|m| m.to_ww_bytes_owned().ok())
        .unwrap_or_default();
    tx.write_all(&HandoverRequest::new(Op::HandToHub, meta).encode())
        .await
        .map_err(|e| format!("asking it: {e}"))?;
    let answer = tokio::time::timeout(ANSWER_TIMEOUT, HandoverAnswer::read(&mut rx))
        .await
        .map_err(|_| format!("no answer within {ANSWER_TIMEOUT:?}"))?
        .map_err(|e| format!("its answer: {e}"))?;
    debug!("holder at {}: {:?}", socket.display(), answer.status);
    Ok(answer)
}

/// The error for a holder that refused: who it is, and why it keeps the device.
pub(crate) fn refused(key: &str, holder: &Holder, meta: Option<&ClientMetaOwned>) -> String {
    let kind = match meta {
        Some(m) => format!("{:?}", m.kind),
        None => holder.kind.clone(),
    };
    let why = if kind.is_empty() || kind == "Unknown" {
        "it doesn't hand it over".to_string()
    } else {
        format!("it doesn't hand it over (kind {kind})")
    };
    format!(
        "{key} is {holder}; asked to hand the device to the ww hub, {why}: close it, or let it hand over \
         (ClientConfig::allow_handover(true))"
    )
}
