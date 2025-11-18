mod common;

use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;
use wire_weaver::prelude::*;
use wire_weaver_client_common::CommandSender;

#[ww_trait]
trait Methods {
    fn no_args();
    fn one_plain_arg(value: u8);
    fn plain_return() -> u8;

    // user-defined
    // ()
    // array of methods
    // evolve args
    // evolve return from plain to struct
}

#[derive(Default)]
struct SharedTestData {
    no_args_called: bool,
    one_plain_arg: u8,
}

mod no_std_sync_server {
    use super::*;

    pub struct NoStdSyncServer {
        pub data: Arc<RwLock<SharedTestData>>,
    }

    impl NoStdSyncServer {
        fn no_args(&mut self) {
            self.data.write().unwrap().no_args_called = true;
        }

        fn one_plain_arg(&mut self, value: u8) {
            self.data.write().unwrap().one_plain_arg = value;
        }

        fn plain_return(&mut self) -> u8 {
            0xAA
        }
    }

    ww_api!(
        "methods.rs" as tests::Methods for NoStdSyncServer,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        property_model = "_=get_set",
        // debug_to_file = "../target/tests_methods_server.rs" // uncomment if you want to see the resulting AST and generated code
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
        "methods.rs" as tests::Methods for StdAsyncClient,
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
    let data = Arc::new(RwLock::new(SharedTestData::default()));

    let data_clone = data.clone();
    tokio::spawn(async move {
        let mut server = no_std_sync_server::NoStdSyncServer { data: data_clone };
        let mut state = common::State::default();
        let mut s1 = [0u8; 512];
        let mut s2 = [0u8; 512];
        let mut se = [0u8; 128];

        while let Some(cmd) = cmd_rx.recv().await {
            let (bytes, done_tx) = common::ser_command(cmd, &mut state, &mut s1);
            let r = server.process_request_bytes(&bytes, &mut s1, &mut s2, &mut se);
            common::send_response(r, done_tx, &mut state);
        }
    });

    let mut client = std_async_client::StdAsyncClient {
        args_scratch: [0u8; 512],
        cmd_tx: CommandSender::new(cmd_tx),
        timeout: Duration::from_millis(100),
    };

    client.root().no_args().await.unwrap();
    assert!(data.read().unwrap().no_args_called);

    client.root().one_plain_arg(0xCC).await.unwrap();
    assert_eq!(data.read().unwrap().one_plain_arg, 0xCC);

    let value = client.root().plain_return().await.unwrap();
    assert_eq!(value, 0xAA);
}
