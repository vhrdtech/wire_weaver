//! WebSocket [Transport]: one [ww_link] message per binary WebSocket message, `[kind: u8][payload ..]`.
//!
//! WebSocket has its own framing and runs over a reliable stream, so [ww_framer] is not used: messages are never
//! split, and there is nothing to re-synchronize on. Messages are buffered until [MessageTx::flush], so the ones
//! accumulated by the event loop go out in as few TCP segments as possible.
//!
//! Link setup, version checks, pings and timeouts are the same as over USB or RTT, see [ww_link].

use std::time::Duration;

use anyhow::bail;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{Bytes, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::{debug, trace};

use crate::config::{ConfigPiece, ValidatedConfig};
use crate::event_loop::DeviceHandle;
use crate::event_loop::command::Command;
use crate::event_loop::transport::{MessageRx, MessageTx, Opened, Selected, Transport};

/// Closing handshake is not waited for longer than this, the other end might be gone already.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(1);

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub(crate) fn try_connect(
    c: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected, anyhow::Error> {
    let Some(url) = c.pieces.iter().rev().find_map(|p| match p {
        ConfigPiece::WebSocketUrl { url } => Some(url.clone()),
        _ => None,
    }) else {
        // no discovery yet, nothing to list
        return Ok(Selected::NotFound { unmatched: vec![] });
    };
    let Some(cmd_rx) = cmd_rx.take() else {
        bail!("Internal error: cmd_rx is None");
    };
    tokio::spawn(async move {
        debug!("ws worker started");
        crate::event_loop::core::worker(cmd_rx, WsTransport).await;
        debug!("ws worker exited");
    });
    Ok(Selected::Device {
        info: Box::new(device_info(&url)),
        handle: Box::new(url),
    })
}

fn device_info(url: &str) -> crate::DeviceInfo {
    crate::DeviceInfo {
        location: format!("ws {url}"),
        manufacturer: String::new(),
        product: String::new(),
        serials: vec![],
        user_label: String::new(),
        api: None,
        usb: None,
    }
}

struct WsTransport;

impl Transport for WsTransport {
    type Tx = WsTx;
    type Rx = WsRx;

    async fn connect(&mut self, handle: DeviceHandle) -> Result<Opened<WsTx, WsRx>, String> {
        let url = handle
            .downcast::<String>()
            .map_err(|_| "expected WebSocket URL".to_string())?;
        let (ws, _response) = connect(&url).await.map_err(|e| format!("{url}: {e}"))?;
        let (tx, rx) = ws.split();
        Ok(Opened {
            tx: WsTx(tx),
            rx: WsRx {
                rx,
                message: Bytes::new(),
            },
        })
    }
}

async fn connect(
    url: &str,
) -> Result<
    (
        Ws,
        tokio_tungstenite::tungstenite::handshake::client::Response,
    ),
    tokio_tungstenite::tungstenite::Error,
> {
    // + kind byte, messages above what the host advertised in LinkSetup are an error anyway
    let max = crate::DEFAULT_MAX_MESSAGE_SIZE + 1;
    let config = WebSocketConfig::default()
        .max_message_size(Some(max))
        .max_frame_size(Some(max));
    // messages are buffered until flush already, Nagle would only add latency on top of that
    tokio_tungstenite::connect_async_with_config(url, Some(config), true).await
}

struct WsTx(SplitSink<Ws, Message>);

impl MessageTx for WsTx {
    async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), String> {
        let mut bytes = Vec::with_capacity(1 + message.len());
        bytes.push(kind);
        bytes.extend_from_slice(message);
        // feed: buffered, written out on flush (or when the write buffer is full)
        self.0
            .feed(Message::Binary(bytes.into()))
            .await
            .map_err(|e| e.to_string())
    }

    async fn flush(&mut self) -> Result<(), String> {
        self.0.flush().await.map_err(|e| e.to_string())
    }

    async fn close(mut self) {
        // sends Close and flushes, the reply is read (and dropped) by the rx half if it is still running
        match tokio::time::timeout(CLOSE_TIMEOUT, self.0.close()).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => trace!("ws close: {e}"),
            Err(_) => trace!("ws close timed out"),
        }
    }
}

struct WsRx {
    rx: SplitStream<Ws>,
    /// Last received message, returned borrowed
    message: Bytes,
}

impl MessageRx for WsRx {
    async fn read_message(&mut self) -> Result<(u8, &[u8]), String> {
        loop {
            // cancel-safe: a partially received message is kept in the WebSocketStream, not in the future
            let message = match self.rx.next().await {
                Some(Ok(message)) => message,
                Some(Err(e)) => return Err(e.to_string()),
                None => return Err("WebSocket closed".into()),
            };
            match message {
                Message::Binary(bytes) => {
                    if bytes.is_empty() {
                        return Err("empty WebSocket message, expected at least a kind byte".into());
                    }
                    self.message = bytes;
                    return Ok((self.message[0], &self.message[1..]));
                }
                Message::Close(frame) => {
                    return Err(match frame {
                        Some(f) => format!("WebSocket closed by device: {} {}", f.code, f.reason),
                        None => "WebSocket closed by device".into(),
                    });
                }
                // replied to by tungstenite itself
                Message::Ping(_) | Message::Pong(_) => {}
                Message::Text(text) => {
                    return Err(format!("unexpected WebSocket text message: {text}"));
                }
                Message::Frame(_) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Host event loop against [ww_device::Server] over a real WebSocket on localhost.
    use super::*;
    use crate::event_loop::device_e2e_tests::{DEV_MAX_MESSAGE, serve, talk};
    use tokio::net::TcpListener;
    use ww_device::LinkEvent;

    /// Tests send `()` as a handle
    struct To(String);

    impl Transport for To {
        type Tx = WsTx;
        type Rx = WsRx;
        async fn connect(&mut self, _: DeviceHandle) -> Result<Opened<WsTx, WsRx>, String> {
            WsTransport.connect(Box::new(self.0.clone())).await
        }
    }

    // Device side: same message format, over an accepted connection

    type DevWs = WebSocketStream<TcpStream>;

    struct DevTx(SplitSink<DevWs, Message>);

    impl ww_device::MessageTx for DevTx {
        type Error = ();
        async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), ()> {
            let bytes: Vec<u8> = [kind].into_iter().chain(message.iter().copied()).collect();
            self.0
                .feed(Message::Binary(bytes.into()))
                .await
                .map_err(|_| ())
        }
        async fn flush(&mut self) -> Result<(), ()> {
            self.0.flush().await.map_err(|_| ())
        }
        fn reset(&mut self) {}
    }

    struct DevRx {
        rx: SplitStream<DevWs>,
        message: Option<Bytes>,
        connected: bool,
    }

    impl ww_device::MessageRx for DevRx {
        type Error = ();
        async fn wait_message(&mut self) -> Result<(), ()> {
            while self.message.is_none() {
                match self.rx.next().await {
                    Some(Ok(Message::Binary(bytes))) if !bytes.is_empty() => {
                        self.message = Some(bytes)
                    }
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                    _ => return Err(()),
                }
            }
            Ok(())
        }
        fn message(&self) -> Option<(u8, &[u8])> {
            self.message.as_ref().map(|b| (b[0], &b[1..]))
        }
        fn consume(&mut self) {
            self.message = None;
        }
        async fn wait_connected(&mut self) {
            // one connection per test
            if self.connected {
                core::future::pending::<()>().await
            }
            self.connected = true;
        }
        fn reset(&mut self) {
            self.message = None;
        }
        fn max_message_len(&self) -> usize {
            DEV_MAX_MESSAGE
        }
    }

    async fn start_ws() -> (
        mpsc::Sender<Command>,
        mpsc::UnboundedReceiver<LinkEvent>,
        tokio::task::JoinHandle<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/ww", listener.local_addr().unwrap());
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let dev = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let (tx, rx) = ws.split();
            let rx = DevRx {
                rx,
                message: None,
                connected: false,
            };
            serve(DevTx(tx), rx, events_tx).await
        });
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
        let host = tokio::spawn(crate::event_loop::core::worker(cmd_rx, To(url)));
        (cmd_tx, events_rx, dev, host)
    }

    #[tokio::test]
    async fn host_and_device_talk_over_ws() {
        talk(start_ws().await).await
    }

    #[tokio::test]
    async fn connection_refused_fails_connect() {
        // bound and dropped: nothing listens there
        let addr = TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap()
            .local_addr()
            .unwrap();
        let mut transport = WsTransport;
        let r = transport.connect(Box::new(format!("ws://{addr}/ww"))).await;
        let Err(e) = r else {
            panic!("connected to nothing")
        };
        assert!(e.contains(&addr.to_string()), "{e}");
    }
}
