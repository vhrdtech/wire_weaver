use tokio::sync::mpsc::UnboundedReceiver;
use wire_weaver::prelude::*;
use wire_weaver_client_common::{Command, DeviceInfoBundle};
use ww_client_server::{Event, EventKind, Request};

pub struct DummyTx;

impl MessageSink for DummyTx {
    fn send(&mut self, _message: &[u8]) -> impl Future<Output = Result<(), ()>> {
        core::future::ready(Ok(()))
    }
}

pub trait TestProcessEvents {
    fn process_request_bytes<'a>(
        &mut self,
        bytes: &[u8],
        scratch_args: &'a mut [u8],
        scratch_event: &'a mut [u8],
        scratch_err: &'a mut [u8],
        msg_tx: &mut impl MessageSink,
    ) -> Result<&'a [u8], ShrinkWrapError>;
}

pub async fn test_event_loop(
    mut cmd_rx: UnboundedReceiver<Command>,
    mut server: impl TestProcessEvents,
    mut msg_tx: impl MessageSink,
) {
    let mut s1 = [0u8; 512];
    let mut s2 = [0u8; 512];
    let mut se = [0u8; 128];

    let mut seq = 1;
    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            Command::Connect { connected_tx, .. } => {
                if let Some(tx) = connected_tx {
                    tx.send(Ok(DeviceInfoBundle::empty())).unwrap();
                }
                continue;
            }
            Command::SendMessage { mut bytes, done_tx } => {
                Request::set_seq(&mut bytes, seq);
                seq += 1;
                let r = server
                    .process_request_bytes(&bytes, &mut s1, &mut s2, &mut se, &mut msg_tx)
                    .expect("process_request");
                if r.is_empty() {
                    continue;
                }
                let event = Event::from_ww_bytes(r).unwrap();
                let r = match event.result {
                    Ok(event_kind) => {
                        let data = match event_kind {
                            EventKind::ReturnValue { data } => data.as_slice().to_vec(),
                            EventKind::ReadValue { data } => data.as_slice().to_vec(),
                            _ => vec![],
                        };
                        Ok(data)
                    }
                    Err(e) => Err(wire_weaver_client_common::Error::RemoteError(
                        e.make_owned(),
                    )),
                };
                if let Some((done_tx, _timeout)) = done_tx {
                    done_tx.send(r).unwrap();
                }
            }
            Command::OnStreamEvent { .. } => {
                // TODO: stream support in tests
            }
            _ => panic!("not supported command"),
        }
    }
}
