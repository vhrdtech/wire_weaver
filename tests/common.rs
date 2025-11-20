use std::collections::HashMap;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot::Sender;
use wire_weaver::prelude::{BufWriter, DeserializeShrinkWrap, RefVec, SerializeShrinkWrap};
use wire_weaver::shrink_wrap;
use wire_weaver::shrink_wrap::UNib32;
use wire_weaver_client_common::{Command, Error, StreamEvent};
use ww_client_server::{
    Event, EventKind, PathKindOwned, Request, RequestKind, StreamSidebandCommand,
};

type DoneTx = Option<Sender<Result<Vec<u8>, Error>>>;

#[derive(Default)]
pub struct State {
    streams: HashMap<Vec<UNib32>, Vec<UnboundedSender<StreamEvent>>>,
}

pub fn ser_command(command: Command, state: &mut State, scratch: &mut [u8]) -> (Vec<u8>, DoneTx) {
    let mut wr = BufWriter::new(scratch);
    match command {
        Command::SendCall {
            path_kind,
            args_bytes,
            done_tx,
            ..
        } => {
            let request = Request {
                seq: 1,
                path_kind: path_kind.as_ref(),
                kind: RequestKind::Call {
                    args: RefVec::new_bytes(&args_bytes),
                },
            };
            request.ser_shrink_wrap(&mut wr).unwrap();
            let bytes = wr.finish_and_take().unwrap().to_vec();
            println!("call: {bytes:02x?} {request:?}");
            (bytes, done_tx)
        }
        Command::SendWrite {
            path_kind,
            value_bytes,
            done_tx,
            ..
        } => {
            let request = Request {
                seq: 1,
                path_kind: path_kind.as_ref(),
                kind: RequestKind::Write {
                    data: RefVec::new_bytes(&value_bytes),
                },
            };
            request.ser_shrink_wrap(&mut wr).unwrap();
            let bytes = wr.finish_and_take().unwrap().to_vec();
            println!("write: {bytes:02x?} {request:?}");
            (bytes, done_tx)
        }
        Command::SendRead {
            path_kind, done_tx, ..
        } => {
            let request = Request {
                seq: 1,
                path_kind: path_kind.as_ref(),
                kind: RequestKind::Read,
            };
            request.ser_shrink_wrap(&mut wr).unwrap();
            let bytes = wr.finish_and_take().unwrap().to_vec();
            println!("read: {bytes:02x?} {request:?}");
            (bytes, done_tx)
        }
        Command::StreamOpen {
            path_kind,
            stream_data_tx,
        } => {
            let request = Request {
                seq: 1,
                path_kind: path_kind.as_ref(),
                kind: RequestKind::StreamSideband {
                    sideband_cmd: StreamSidebandCommand::Open,
                },
            };
            println!("{request:?}");
            request.ser_shrink_wrap(&mut wr).unwrap();
            let bytes = wr.finish_and_take().unwrap().to_vec();
            println!("stream open: {bytes:02x?} {request:?}");
            let PathKindOwned::Absolute { path } = path_kind else {
                panic!("only absolute paths are supported for now");
            };
            state.streams.entry(path).or_default().push(stream_data_tx);
            (bytes, None)
        }
        _ => unimplemented!(),
    }
}

pub fn send_response(r: Result<&[u8], shrink_wrap::Error>, done_tx: DoneTx, state: &mut State) {
    match r {
        Ok(response_bytes) => {
            if response_bytes.is_empty() {
                println!("ignoring empty response");
                return;
            }
            let event = match Event::from_ww_bytes(response_bytes) {
                Ok(event) => event,
                Err(e) => {
                    println!("failed to deserialize Event: {response_bytes:02x?}");
                    panic!("{:?}", e);
                }
            };
            println!("response: {response_bytes:02x?} {event:?}");
            match event.result {
                Ok(event_kind) => match event_kind {
                    EventKind::ReturnValue { data } | EventKind::ReadValue { data } => {
                        if let Some(tx) = done_tx {
                            tx.send(Ok(data.to_vec())).unwrap();
                        }
                    }
                    EventKind::Written => {
                        if let Some(tx) = done_tx {
                            tx.send(Ok(vec![])).unwrap();
                        }
                    }
                    EventKind::StreamData { path, data } => {
                        let path: Vec<_> = path.iter().map(|n| n.unwrap()).collect();
                        let Some(subscribers) = state.streams.get_mut(&path) else {
                            println!("no subscribers for {path:?}");
                            return;
                        };
                        let data = data.to_vec();
                        subscribers.retain_mut(|sub| {
                            let still_alive = sub.send(StreamEvent::Data(data.clone())).is_ok();
                            still_alive
                        });
                    }
                    u => unimplemented!("{u:?}"),
                },
                Err(e) => {
                    if let Some(tx) = done_tx {
                        tx.send(Err(Error::RemoteError(e))).unwrap();
                    }
                }
            }
        }
        Err(e) => {
            if let Some(tx) = done_tx {
                println!("sending err: {e:?}");
                tx.send(Err(e.into())).unwrap();
            } else {
                panic!("process_request_bytes failed: {e:?}");
            }
        }
    }
}
