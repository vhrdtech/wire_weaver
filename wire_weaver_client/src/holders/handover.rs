//! The holder's side of a polite handover (CLT-43): while a program holds a device directly, it listens on
//! `<runtime>/ww/holders/<key>.sock` for another program asking it to hand the device to the ww hub
//! ([ww_local_socket::HandoverRequest]). One that allows it ([ClientConfig::allow_handover](crate::ClientConfig::allow_handover))
//! has its event loop let go of the device (the link exits with `HandedToHub`), answers `Released` once its claim on
//! the lock file is gone, and reconnects through the hub (CLT-40's path). One that doesn't answers `Refused` with its
//! client metadata, so the asker's error names it, and never releases.
//!
//! The socket is a [ww_local_socket::listen]ing one: user-only (0600 in the 0700 runtime dir, a Windows pipe for the
//! current user only, remote logons denied), and the peer's user is checked on accept.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::sync::{oneshot, watch};
use tracing::{debug, info, warn};
use wire_weaver::shrink_wrap::{DeserializeShrinkWrapOwned, SerializeShrinkWrapOwned};
use ww_local_socket::{HandoverAnswer, HandoverRequest, HandoverStatus, Listener, Op};

use crate::client_meta::ClientMetaOwned;

/// An asker must send its request within this time, or it is dropped.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
/// The event loop must let go of the device within this time after a request, or the asker hears `Failed`.
const RELEASE_TIMEOUT: Duration = Duration::from_secs(5);
/// Unix socket paths are limited to ~104-108 bytes: a longer one is named after a hash of the key.
const MAX_SOCKET_PATH: usize = 100;

/// Offered by a [Claim](super::Claim): the listener task runs while it lives.
#[derive(Debug)]
pub(crate) struct Offer {
    pub(crate) socket: PathBuf,
    requested: watch::Receiver<bool>,
    /// Dropped with the claim, after its lock: the device is released, the listener answers a waiting asker and ends
    _released: oneshot::Sender<()>,
}

impl Offer {
    /// Listens for askers of `key` next to its lock file in `dir`. Needs a tokio runtime (the event loop's).
    pub(crate) fn start(
        dir: &Path,
        key: &str,
        allowed: bool,
        meta: Option<&ClientMetaOwned>,
    ) -> io::Result<Offer> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|e| io::Error::new(io::ErrorKind::Unsupported, e.to_string()))?;
        let socket = socket_path(dir, key);
        // we hold the device's lock: a socket file left there is a crashed holder's
        let listener = {
            let _guard = runtime.enter();
            ww_local_socket::listen(&socket, true)?
        };
        let meta = meta
            .and_then(|m| m.to_ww_bytes_owned().ok())
            .unwrap_or_default();
        let (requested_tx, requested) = watch::channel(false);
        let (released, released_rx) = oneshot::channel();
        runtime.spawn(serve(
            listener,
            key.to_string(),
            allowed,
            meta,
            requested_tx,
            released_rx,
        ));
        Ok(Offer {
            socket,
            requested,
            _released: released,
        })
    }

    /// Returns once an asker wants the device handed to the hub (and this holder allows it); never otherwise.
    /// Cancel-safe.
    pub(crate) async fn requested(&self) {
        let mut requested = self.requested.clone();
        loop {
            if *requested.borrow_and_update() {
                return;
            }
            if requested.changed().await.is_err() {
                // the listener is gone: nobody asks any more
                std::future::pending::<()>().await;
            }
        }
    }
}

/// `<dir>/<key>.sock`, or `<dir>/h-<hash>.sock` when that would be too long for a Unix socket path. The lock file
/// names it either way (`handover = <path>`).
fn socket_path(dir: &Path, key: &str) -> PathBuf {
    let path = dir.join(format!("{}.sock", super::sanitize_file_name(key)));
    if path.as_os_str().len() <= MAX_SOCKET_PATH {
        return path;
    }
    // FNV-1a: stable across builds, unlike std's hasher
    let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    dir.join(format!("h-{hash:016x}.sock"))
}

async fn serve(
    listener: Listener,
    key: String,
    allowed: bool,
    meta: Vec<u8>,
    requested: watch::Sender<bool>,
    mut released: oneshot::Receiver<()>,
) {
    loop {
        let (mut stream, peer) = tokio::select! {
            // the claim is gone without anyone asking
            _ = &mut released => return,
            accepted = listener.accept() => match accepted {
                Ok(accepted) => accepted,
                Err(e) => {
                    warn!("{key}: holder socket: {e}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            },
        };
        let request =
            match tokio::time::timeout(REQUEST_TIMEOUT, HandoverRequest::read(&mut stream)).await {
                Ok(Ok(request)) => request,
                Ok(Err(e)) => {
                    debug!("{key}: holder socket: bad request from {peer:?}: {e}");
                    continue;
                }
                Err(_) => continue,
            };
        let asker = asker(&request.meta, peer.pid);
        let status = if request.op != Op::HandToHub {
            HandoverStatus::Failed
        } else if !allowed {
            info!(
                "{key}: {asker} asked to hand it to the ww hub, refused (ClientConfig::allow_handover)"
            );
            HandoverStatus::Refused
        } else {
            info!("{key}: handing it to the ww hub, asked by {asker}");
            requested.send_replace(true);
            match tokio::time::timeout(RELEASE_TIMEOUT, &mut released).await {
                Ok(_) => HandoverStatus::Released,
                Err(_) => {
                    warn!("{key}: not released within {RELEASE_TIMEOUT:?} for the ww hub");
                    HandoverStatus::Failed
                }
            }
        };
        let answer = HandoverAnswer::new(status, meta.clone()).encode();
        if let Err(e) = stream.write_all(&answer).await {
            debug!("{key}: holder socket: answering {asker}: {e}");
        }
        if status == HandoverStatus::Released {
            return;
        }
    }
}

/// Who asked, for the log.
fn asker(meta: &[u8], pid: Option<u32>) -> String {
    match ClientMetaOwned::from_ww_bytes_owned(meta) {
        Ok(m) => format!("{} (pid {})", m.program, m.pid),
        Err(_) => match pid {
            Some(pid) => format!("pid {pid}"),
            None => "another program".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_keys_get_a_hashed_socket_name() {
        let dir = Path::new("/run/user/1000/ww/holders");
        assert_eq!(
            socket_path(dir, "usb:c0de:cafe:42:0"),
            dir.join("usb_c0de_cafe_42_0.sock")
        );
        let long = format!("ipc:/tmp/{}", "x".repeat(100));
        let p = socket_path(dir, &long);
        assert!(p.as_os_str().len() <= MAX_SOCKET_PATH, "{p:?}");
        assert_eq!(p, socket_path(dir, &long), "stable");
        assert_ne!(p, socket_path(dir, &format!("{long}y")));
    }
}
