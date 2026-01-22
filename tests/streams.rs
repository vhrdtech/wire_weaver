use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;
use wire_weaver::prelude::*;
use wire_weaver::ww_version::{FullVersionOwned, VersionOwned};
use wire_weaver_client_common::rx_dispatcher::DispatcherMessage;
use wire_weaver_client_common::{Command, CommandSender, DeviceFilter, OnError, StreamEvent};

#[ww_trait]
trait Streams {
    stream!(plain_stream: u8);
    sink!(plain_sink: u8);
    stream!(vec_stream: [u8]);
    stream!(array_of_streams[]: [u8]);
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

        fn array_of_streams_sideband(
            &mut self,
            _idx: [UNib32; 1],
            _cmd: StreamSidebandCommand,
        ) -> Option<StreamSidebandEvent> {
            println!("array_of_streams sideband: {_idx:?} {_cmd:?}");
            None
        }

        fn finish(&mut self) {
            println!("finish called");
        }

        pub fn send_updates(&mut self, stream_number: usize) -> Vec<Vec<u8>> {
            println!("sending updates");
            let mut updates = vec![];
            let mut s1 = [0u8; 128];
            let mut s2 = [0u8; 128];
            match stream_number {
                0 => {
                    updates.push(
                        api_impl::stream_data_ser()
                            .plain_stream(&0xAA, &mut s1, &mut s2)
                            .unwrap()
                            .to_vec(),
                    );
                }
                1 => {
                    updates.push(
                        api_impl::stream_data_ser()
                            .vec_stream(&RefVec::new_bytes(&[0xAA, 0xBB, 0xCC]), &mut s1, &mut s2)
                            .unwrap()
                            .to_vec(),
                    );
                }
                2 => {
                    updates.push(
                        api_impl::stream_data_ser()
                            .array_of_streams(
                                0,
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
    use wire_weaver_client_common::CommandSender;

    pub struct StdAsyncClient {
        pub args_scratch: [u8; 512],
        pub cmd_tx: CommandSender,
        pub timeout: Duration,
    }

    mod api_client {
        use super::*;
        ww_api!(
            "streams.rs" as crate::Streams for StdAsyncClient,
            client = "full_client",
            no_alloc = false,
            use_async = true,
            debug_to_file = "../target/tests_streams_client.rs"
        );
    }
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
    tracing_subscriber::fmt::init();
    let (transport_cmd_tx, mut transport_cmd_rx) = mpsc::unbounded_channel();
    let (notify_tx, mut notify_rx) = mpsc::unbounded_channel::<usize>();
    let (dispatcher_msg_tx, dispatcher_msg_rx) = mpsc::unbounded_channel();
    dispatcher_msg_tx
        .send(DispatcherMessage::Connected)
        .unwrap();
    let data = Arc::new(RwLock::new(SharedTestData::default()));

    let data_clone = data.clone();
    tokio::spawn(async move {
        let mut server = no_std_sync_server::NoStdSyncServer { data: data_clone };
        let mut s1 = [0u8; 512];
        let mut s2 = [0u8; 512];
        let mut se = [0u8; 128];

        loop {
            tokio::select! {
                cmd = transport_cmd_rx.recv() => {
                    let Some(cmd) = cmd else {
                        break;
                    };
                    let bytes = match cmd {
                        Command::Connect { connected_tx, .. } => {
                            if let Some(tx) = connected_tx {
                                tx.send(Ok(())).unwrap();
                            }
                            continue;
                        }
                        Command::SendMessage { bytes } => bytes,
                        _ => continue,
                    };
                    let r = server
                        .process_request_bytes(&bytes, &mut s1, &mut s2, &mut se)
                        .expect("process_request");
                    tokio::time::sleep(Duration::from_millis(1)).await; // rx_dispatcher sometimes receive event before cmd
                    dispatcher_msg_tx
                        .send(DispatcherMessage::MessageBytes(r.to_vec()))
                        .expect("send_message");
                    }
                notify = notify_rx.recv() => {
                    let Some(n) = notify else { break };
                    for bytes in server.send_updates(n) {
                        dispatcher_msg_tx
                            .send(DispatcherMessage::MessageBytes(bytes))
                            .expect("send_message");
                    }
                }
            }
        }
    });

    let mut cmd_tx = CommandSender::new(transport_cmd_tx, dispatcher_msg_rx);
    cmd_tx
        .connect(
            DeviceFilter::vhrd_usb_can(),
            FullVersionOwned::new("test".into(), VersionOwned::new(0, 1, 0)),
            OnError::ExitImmediately,
        )
        .await
        .expect("connect");
    let mut client = std_async_client::StdAsyncClient {
        args_scratch: [0u8; 512],
        cmd_tx,
        timeout: Duration::from_millis(1000),
    };
    tokio::time::sleep(Duration::from_millis(10)).await;

    let mut rx = client.plain_stream_sub().expect("successful stream open");
    notify_tx.send(0).unwrap();
    let connected = rx.recv().await.unwrap();
    assert_eq!(connected, StreamEvent::Connected);
    let stream_data = rx.recv().await.unwrap();
    assert_eq!(stream_data, StreamEvent::Data(vec![0xAA]));

    client.plain_sink_pub(1).unwrap();
    client.plain_sink_pub(2).unwrap();

    client.finish().call().await.unwrap();
    assert_eq!(data.read().unwrap().plain_sink_rx, vec![1, 2]);

    let mut rx2 = client.vec_stream_sub().expect("successful stream open");
    notify_tx.send(1).unwrap();
    let connected = rx2.recv().await.unwrap();
    assert_eq!(connected, StreamEvent::Connected);
    let stream_data = rx2.recv().await.unwrap();
    let StreamEvent::Data(data) = stream_data else {
        panic!("wrong stream event");
    };
    let value: Vec<u8> = DeserializeShrinkWrap::from_ww_bytes(&data[..]).unwrap();
    assert_eq!(value, vec![0xAA, 0xBB, 0xCC]);

    let mut rx_arr0 = client
        .array_of_streams_sub(0)
        .expect("subscribe to stream array 0");
    notify_tx.send(2).unwrap();
    let connected = rx_arr0.recv().await.unwrap();
    assert_eq!(connected, StreamEvent::Connected);
    let stream_data = rx_arr0.recv().await.unwrap();
    let StreamEvent::Data(data) = stream_data else {
        panic!("wrong stream event");
    };
    let value: Vec<u8> = DeserializeShrinkWrap::from_ww_bytes(&data[..]).unwrap();
    assert_eq!(value, vec![0xAA, 0xBB, 0xCC]);

    tokio::time::sleep(Duration::from_millis(10)).await;
}
