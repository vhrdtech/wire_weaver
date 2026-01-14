use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;
use wire_weaver::prelude::*;
use wire_weaver::ww_version::{FullVersionOwned, VersionOwned};
use wire_weaver_client_common::rx_dispatcher::DispatcherMessage;
use wire_weaver_client_common::{Command, CommandSender, DeviceFilter, OnError};

#[ww_trait]
trait Properties {
    property!(rw plain: u8);

    // changes pub sub
    // const ro wo
    // () [u8]
    // user-defined
    // arrays
}

#[derive(Default)]
struct SharedTestData {
    plain: u8,
}

mod no_std_sync_server {
    use super::*;

    pub struct NoStdSyncServer {
        pub data: Arc<RwLock<SharedTestData>>,
    }

    impl NoStdSyncServer {
        fn set_plain(&mut self, value: u8) {
            self.data.write().unwrap().plain = value;
        }

        fn get_plain(&mut self) -> u8 {
            self.data.read().unwrap().plain
        }
    }

    ww_api!(
        "properties.rs" as tests::Properties for NoStdSyncServer,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        property_model = "_=get_set",
        // debug_to_file = "../target/tests_properties_server.rs" // uncomment if you want to see the resulting AST and generated code
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
        "properties.rs" as tests::Properties for StdAsyncClient,
        client = "async_worker",
        no_alloc = false,
        use_async = true,
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
    tracing_subscriber::fmt::init();
    let (transport_cmd_tx, mut transport_cmd_rx) = mpsc::unbounded_channel();
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

        while let Some(cmd) = transport_cmd_rx.recv().await {
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

    let value = client.root().read_plain().await.unwrap();
    assert_eq!(value, 0);

    client.root().write_plain(0xAA).await.unwrap();
    assert_eq!(data.read().unwrap().plain, 0xAA);

    let value = client.root().read_plain().await.unwrap();
    assert_eq!(value, 0xAA);
}
