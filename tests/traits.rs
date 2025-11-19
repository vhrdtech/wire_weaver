mod common;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;
use wire_weaver::prelude::*;
use wire_weaver_client_common::CommandSender;

#[ww_trait]
trait Traits {
    ww_impl!(g1: Subgroup);
    ww_impl!(gpio[]: Gpio);
    ww_impl!(periph[]: Peripheral);

    // TODO: print proper error when implementing same trait twice?
    // trait from crates
    // trait addressing
}

#[ww_trait]
trait Subgroup {
    fn m1();
}

#[ww_trait]
trait Gpio {
    fn set_high();
}

#[ww_trait]
trait Peripheral {
    ww_impl!(channel[]: Channel);
}

#[ww_trait]
trait Channel {
    property!(gain: f32);
}

#[derive(Default)]
struct SharedTestData {
    subgroup_m1_called: bool,
    gpio_used_indices: Vec<u32>,
    set_gain: HashMap<[UNib32; 2], f32>,
}

mod no_std_sync_server {
    use super::*;

    pub struct NoStdSyncServer {
        pub data: Arc<RwLock<SharedTestData>>,
    }

    impl NoStdSyncServer {
        fn subgroup_m1(&mut self) {
            self.data.write().unwrap().subgroup_m1_called = true;
        }

        fn gpio_set_high(&mut self, index: [UNib32; 1]) {
            self.data
                .write()
                .unwrap()
                .gpio_used_indices
                .push(index[0].0);
        }

        fn set_gain(&mut self, index: [UNib32; 2], gain: f32) {
            self.data.write().unwrap().set_gain.insert(index, gain);
        }

        fn get_gain(&self, index: [UNib32; 2]) -> f32 {
            self.data
                .read()
                .unwrap()
                .set_gain
                .get(&index)
                .copied()
                .unwrap_or(0.0)
        }
    }

    ww_api!(
        "traits.rs" as tests::Traits for NoStdSyncServer,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        property_model = "_=get_set",
        debug_to_file = "../target/tests_traits_server.rs"
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
        "traits.rs" as tests::Traits for StdAsyncClient,
        client = "async_worker",
        no_alloc = false,
        use_async = true,
        debug_to_file = "../target/tests_traits_client.rs"
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

    client.root().g1().m1().await.unwrap();
    assert!(data.read().unwrap().subgroup_m1_called);

    client.root().gpio(0).set_high().await.unwrap();
    assert!(data.read().unwrap().gpio_used_indices.contains(&0));

    client.root().gpio(123).set_high().await.unwrap();
    assert!(data.read().unwrap().gpio_used_indices.contains(&123));

    client
        .root()
        .periph(3)
        .channel(7)
        .write_gain(10.0)
        .await
        .unwrap();
    assert_eq!(
        data.read().unwrap().set_gain.get(&[UNib32(3), UNib32(7)]),
        Some(&10.0)
    );
    let value = client
        .root()
        .periph(3)
        .channel(7)
        .read_gain()
        .await
        .unwrap();
    assert!(value == 10.0);
}
