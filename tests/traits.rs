use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;
use wire_weaver::MessageSink;
use wire_weaver::prelude::*;
use wire_weaver::ww_version::{FullVersionOwned, VersionOwned};
use wire_weaver_client_common::rx_dispatcher::DispatcherMessage;
use wire_weaver_client_common::{Command, CommandSender, DeviceFilter, OnError};

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
    fn run();
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
        fn g1_m1(&mut self, _msg_tx: &mut impl MessageSink) {
            self.data.write().unwrap().subgroup_m1_called = true;
        }

        fn gpio_set_high(&mut self, _msg_tx: &mut impl MessageSink, index: [UNib32; 1]) {
            self.data
                .write()
                .unwrap()
                .gpio_used_indices
                .push(index[0].0);
        }

        fn set_periph_channel_gain(&mut self, index: [UNib32; 2], gain: f32) {
            self.data.write().unwrap().set_gain.insert(index, gain);
        }

        fn get_periph_channel_gain(&self, index: [UNib32; 2]) -> f32 {
            self.data
                .read()
                .unwrap()
                .set_gain
                .get(&index)
                .copied()
                .unwrap_or(0.0)
        }

        fn periph_channel_run(&mut self, _msg_tx: &mut impl MessageSink, _index: [UNib32; 2]) {}
    }

    mod api_impl {
        use wire_weaver::ww_api;

        ww_api!(
            "traits.rs" as super::Traits for NoStdSyncServer,
            server = true, no_alloc = true, use_async = false,
            method_model = "_=immediate",
            property_model = "_=get_set",
            debug_to_file = "../target/tests_traits_server.rs"
        );
    }
}

mod std_async_client {
    use super::*;
    use wire_weaver_client_common::CommandSender;

    pub struct StdAsyncClient {
        pub args_scratch: [u8; 512],
        pub cmd_tx: CommandSender,
    }

    mod api_client {
        use super::*;
        ww_api!(
            "traits.rs" as crate::Traits for StdAsyncClient,
            client = "full_client",
            no_alloc = false,
            use_async = true,
            debug_to_file = "../target/tests_traits_client.rs"
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

struct DummyTx;
impl MessageSink for DummyTx {
    fn send(&mut self, _message: &[u8]) -> impl Future<Output = Result<(), ()>> {
        core::future::ready(Ok(()))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn std_async_client_driving_no_std_sync_server() {
    tracing_subscriber::fmt::init();
    let (transport_cmd_tx, mut transport_cmd_rx) = mpsc::unbounded_channel();
    let (dispatcher_msg_tx, dispatcher_msg_rx) = mpsc::unbounded_channel();
    dispatcher_msg_tx
        .send(DispatcherMessage::Connected)
        .unwrap();
    let data = Arc::new(RwLock::new(SharedTestData::default()));

    let mut dummy_msg_tx = DummyTx {};
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
                .process_request_bytes(&bytes, &mut s1, &mut s2, &mut se, &mut dummy_msg_tx)
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
    };
    tokio::time::sleep(Duration::from_millis(10)).await;

    client.g1().m1().call().await.unwrap();
    assert!(data.read().unwrap().subgroup_m1_called);

    client.gpio(0).set_high().call().await.unwrap();
    assert!(data.read().unwrap().gpio_used_indices.contains(&0));

    client.gpio(123).set_high().call().await.unwrap();
    assert!(data.read().unwrap().gpio_used_indices.contains(&123));

    client
        .periph(3)
        .channel(7)
        .write_gain(10.0)
        .write()
        .await
        .unwrap();
    assert_eq!(
        data.read().unwrap().set_gain.get(&[UNib32(3), UNib32(7)]),
        Some(&10.0)
    );
    let value = client
        .periph(3)
        .channel(7)
        .read_gain()
        .read()
        .await
        .unwrap();
    assert!(value == 10.0);
}
