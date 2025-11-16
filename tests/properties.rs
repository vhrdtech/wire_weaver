mod common;

use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;
use wire_weaver::prelude::*;
use wire_weaver_client_common::CommandSender;

#[ww_trait]
trait Properties {
    property!(rw plain: u8);

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
        // debug_to_file = "../target/tests_properties.rs" // uncomment if you want to see the resulting AST and generated code
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
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
    let data = SharedTestData::default();
    let data = Arc::new(RwLock::new(data));

    let data_clone = data.clone();
    tokio::spawn(async move {
        let mut server = no_std_sync_server::NoStdSyncServer { data: data_clone };
        let mut scratch_args = [0u8; 512];
        let mut scratch_event = [0u8; 512];
        let mut scratch_err = [0u8; 128];

        while let Some(cmd) = cmd_rx.recv().await {
            let (bytes, done_tx) = common::ser_command(cmd, &mut scratch_args);
            println!("serving request: {bytes:02x?}");
            let r = server.process_request_bytes(
                &bytes,
                &mut scratch_args,
                &mut scratch_event,
                &mut scratch_err,
            );
            match r {
                Ok(response_bytes) => {
                    println!("response: {response_bytes:02x?}");
                    if let Some(tx) = done_tx {
                        println!("sending");
                        tx.send(Ok(response_bytes.to_vec())).unwrap();
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
    });

    let mut client = std_async_client::StdAsyncClient {
        args_scratch: [0u8; 512],
        cmd_tx: CommandSender::new(cmd_tx),
        timeout: Duration::from_millis(100),
    };

    let value = client.root().read_plain().await.unwrap();
    assert_eq!(value, 0);

    client.root().write_plain(0xAA).await.unwrap();
    assert_eq!(data.read().unwrap().plain, 0xAA);

    let value = client.root().read_plain().await.unwrap();
    assert_eq!(value, 0xAA);
}
