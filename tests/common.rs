use tokio::sync::oneshot::Sender;
use wire_weaver::prelude::{BufWriter, RefVec, SerializeShrinkWrap};
use wire_weaver_client_common::{Command, Error};
use ww_client_server::{Request, RequestKind};

pub fn ser_command(
    command: Command,
    scratch: &mut [u8],
) -> (Vec<u8>, Option<Sender<Result<Vec<u8>, Error>>>) {
    let mut wr = BufWriter::new(scratch);
    match command {
        // Command::SendCall { .. } => return None,
        Command::SendWrite {
            path_kind,
            value_bytes,
            done_tx,
            ..
        } => {
            let request = Request {
                seq: 0,
                path_kind: path_kind.as_ref(),
                kind: RequestKind::Write {
                    data: RefVec::new_bytes(&value_bytes),
                },
            };
            request.ser_shrink_wrap(&mut wr).unwrap();
            let bytes = wr.finish_and_take().unwrap().to_vec();
            (bytes, done_tx)
        }
        Command::SendRead {
            path_kind, done_tx, ..
        } => {
            let request = Request {
                seq: 0,
                path_kind: path_kind.as_ref(),
                kind: RequestKind::Read,
            };
            request.ser_shrink_wrap(&mut wr).unwrap();
            let bytes = wr.finish_and_take().unwrap().to_vec();
            (bytes, done_tx)
        }
        // Command::Subscribe { .. } => return None,
        _ => unimplemented!(),
    }
}
