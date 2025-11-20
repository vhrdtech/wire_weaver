mod common;

use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;
use wire_weaver::prelude::*;
use wire_weaver_client_common::{CommandSender, StreamEvent};

#[ww_trait]
trait Streams {
    stream!(plain_stream: u8);
    sink!(plain_sink: u8);
    stream!(vec_stream: [u8]);
    fn finish();
}

#[derive(Default)]
struct SharedTestData {
    plain_sink_rx: Vec<u8>,
}

mod no_std_sync_server {
    use super::*;
    use ww_client_server::{StreamSidebandCommand, StreamSidebandEvent};

    pub struct NoStdSyncServer {
        pub data: Arc<RwLock<SharedTestData>>,
    }

    impl NoStdSyncServer {
        fn plain_stream_sideband(
            &mut self,
            _cmd: StreamSidebandCommand,
        ) -> Option<StreamSidebandEvent> {
            println!("plain stream sideband: {_cmd:?}");
            None
        }

        fn plain_sink_sideband(
            &mut self,
            _cmd: StreamSidebandCommand,
        ) -> Option<StreamSidebandEvent> {
            println!("plain sink sideband: {_cmd:?}");
            None
        }

        fn plain_sink_write(&mut self, value: u8) {
            println!("got plain sink write: {value}");
            self.data.write().unwrap().plain_sink_rx.push(value);
        }

        fn vec_stream_sideband(
            &mut self,
            _cmd: StreamSidebandCommand,
        ) -> Option<StreamSidebandEvent> {
            println!("vec stream sideband: {_cmd:?}");
            None
        }

        fn finish(&mut self) {}

        pub fn send_updates(&mut self, stream_number: usize) -> Vec<Vec<u8>> {
            println!("sending updates");
            let mut updates = vec![];
            let mut s1 = [0u8; 128];
            let mut s2 = [0u8; 128];
            match stream_number {
                0 => {
                    updates.push(
                        api_impl::plain_stream_data_ser(&0xAA, &mut s1, &mut s2)
                            .unwrap()
                            .to_vec(),
                    );
                }
                1 => {
                    updates.push(
                        api_impl::vec_stream_data_ser(
                            &RefVec::new_bytes(&[0xAA, 0xBB, 0xCC]),
                            &mut s1,
                            &mut s2,
                        )
                        .unwrap()
                        .to_vec(),
                    );
                }
                _ => {}
            }
            updates
        }
    }

    ww_api!(
        "streams.rs" as tests::Streams for NoStdSyncServer,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        property_model = "_=get_set",
        debug_to_file = "../target/tests_streams_server.rs"
    );
}

mod std_async_client {
    use super::*;
    use std::time::Duration;
    use wire_weaver_client_common::CommandSender;

    pub struct StdAsyncClient {
        pub args_scratch: [u8; 512],
        pub cmd_tx: CommandSender,
        pub timeout: Duration,
    }

    ww_api!(
        "streams.rs" as tests::Streams for StdAsyncClient,
        client = "async_worker",
        no_alloc = false,
        use_async = true,
        debug_to_file = "../target/tests_streams_client.rs"
    );
}

// mod no_std_raw_client {
//     use super::*;
//
//     pub struct RawClient {}
//
//     ww_api!(
//         "properties.rs" as tests::Properties for RawClient,
//         client = "raw",
//         no_alloc = true,
//         use_async = false,
//     );
// }

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn std_async_client_driving_no_std_sync_server() {
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
    let (notify_tx, mut notify_rx) = mpsc::unbounded_channel::<usize>();
    let data = Arc::new(RwLock::new(SharedTestData::default()));

    let data_clone = data.clone();
    let handle = tokio::spawn(async move {
        let mut server = no_std_sync_server::NoStdSyncServer { data: data_clone };
        let mut state = common::State::default();
        let mut s1 = [0u8; 512];
        let mut s2 = [0u8; 512];
        let mut se = [0u8; 128];

        loop {
            tokio::select! {
                cmd = cmd_rx.recv() => {
                    let Some(cmd) = cmd else { break };
                    let (bytes, done_tx) = common::ser_command(cmd, &mut state, &mut s1);
                    let r = server.process_request_bytes(&bytes, &mut s1, &mut s2, &mut se);
                    common::send_response(r, done_tx, &mut state);
                }
                notify = notify_rx.recv() => {
                    let Some(n) = notify else { break };
                    for bytes in server.send_updates(n) {
                        common::send_response(Ok(&bytes), None, &mut state);
                    }
                }
            }
        }
        println!("async: exiting");
    });

    let mut client = std_async_client::StdAsyncClient {
        args_scratch: [0u8; 512],
        cmd_tx: CommandSender::new(cmd_tx),
        timeout: Duration::from_millis(100),
    };

    let mut rx = client
        .root()
        .plain_stream_sub()
        .expect("successful stream open");
    notify_tx.send(0).unwrap();
    let stream_data = rx.recv().await.unwrap();
    assert_eq!(stream_data, StreamEvent::Data(vec![0xAA]));

    client.root().plain_sink_pub(1).unwrap();
    client.root().plain_sink_pub(2).unwrap();

    client.root().finish().await.unwrap();
    assert_eq!(data.read().unwrap().plain_sink_rx, vec![1, 2]);

    let mut rx2 = client
        .root()
        .vec_stream_sub()
        .expect("successful stream open");
    notify_tx.send(1).unwrap();
    let stream_data = rx2.recv().await.unwrap();
    assert_eq!(stream_data, StreamEvent::Data(vec![0xAA, 0xBB, 0xCC]));

    handle.abort();
}
