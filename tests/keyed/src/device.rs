//! The Dissector device of the keyed tests: a flow table, and blocking `no_alloc` and async servers of it.

use keyed_api::{Endpoint, FlowKey, Port, Proto};
use std::sync::{Arc, Mutex};
use tests_common::TestDevice;
use wire_weaver::prelude::*;
use ww_client_server::ResourceIndex;
use ww_keyed::KeyEvent;
use ww_si::origins::DeviceMicros;

/// Flows by index, each with its byte count; `None` is a free index.
#[derive(Default)]
pub struct Table {
    pub flows: Vec<Option<(FlowKey, u64)>>,
}

impl Table {
    fn indices(&self) -> Vec<UNib32> {
        (0u32..)
            .zip(&self.flows)
            .filter(|(_, flow)| flow.is_some())
            .map(|(i, _)| UNib32(i))
            .collect()
    }
}

/// Same handlers for the blocking and async servers: `$($async)?`, the event out trait and how a stream event is
/// sent differ.
macro_rules! handlers {
    ($($async:ident)?; $out:path; $send:ident; $($await:tt)*) => {
        fn valid_indices_root_flow(&mut self) -> ValidIndices<'_> {
            self.valid = self.table.lock().unwrap().indices();
            ValidIndices::List(RefVec::Slice { slice: &self.valid })
        }

        $($async)? fn flow_index_of(
            &mut self,
            _cx: &mut Context<'_, impl $out>,
            key: FlowKey,
        ) -> RpcResult<Option<UNib32>> {
            let table = self.table.lock().unwrap();
            let found = (0u32..)
                .zip(&table.flows)
                .find(|(_, flow)| flow.as_ref().map(|(k, _)| k) == Some(&key));
            Ready(found.map(|(i, _)| UNib32(i)))
        }

        $($async)? fn get_flow_key(
            &mut self,
            _cx: &mut Context<'_, impl $out>,
            index_chain: [ResourceIndex; 1],
        ) -> GetResult<FlowKey, ()> {
            let table = self.table.lock().unwrap();
            let flow = table.flows[index_chain[0].get().0 as usize].clone();
            Value(flow.expect("valid indices are the occupied ones").0)
        }

        /// Drops every flow and announces each on the `keys` stream.
        $($async)? fn clear(&mut self, cx: &mut Context<'_, impl $out>) -> RpcResult<()> {
            let indices = self.table.lock().unwrap().indices();
            self.table.lock().unwrap().flows.clear();
            for index in indices {
                api_impl::stream_data_ser()
                    .$send(&KeyEvent::Removed { index }, cx)
                    $($await)*
                    .expect("event sent");
            }
            Ready(())
        }

        $($async)? fn get_flow_bytes(
            &mut self,
            _cx: &mut Context<'_, impl $out>,
            index_chain: [ResourceIndex; 1],
        ) -> GetResult<ww_si::Bytes<u64>, ()> {
            let table = self.table.lock().unwrap();
            let flow = &table.flows[index_chain[0].get().0 as usize];
            Value(ww_si::Bytes::new(flow.as_ref().expect("valid indices are the occupied ones").1))
        }

        $($async)? fn flow_packets(
            &mut self,
            _cx: &mut Context<'_, impl $out>,
            _index_chain: [ResourceIndex; 1],
        ) -> RpcResult<()> {
            Ready(())
        }

        $($async)? fn close_flow_packets(
            &mut self,
            _cx: &mut Context<'_, impl $out>,
            _index_chain: [ResourceIndex; 1],
        ) {
        }

        $($async)? fn sideband_flow_packets(
            &mut self,
            _cx: &mut Context<'_, impl $out>,
            _index_chain: [ResourceIndex; 1],
            _sideband: ww_client_server::StreamSideband,
        ) -> Option<ww_client_server::StreamSideband> {
            None
        }
    };
}

pub mod blocking_server {
    use super::*;
    use tests_common::TestProcessEvents;

    pub struct Server {
        pub table: Arc<Mutex<Table>>,
        pub valid: Vec<UNib32>,
    }

    impl Server {
        handlers!(; BlockingEventOut; flow_keys_send_blocking;);
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            keyed_api :: Dissector for super::Server,
            server = true, no_alloc = true, use_async = false,
            method_model = "_=immediate",
            property_model = "_=get_set",
            multi_req = true,
            debug_to_file = "../../target/tests_keyed_server.rs"
        );
    }

    impl TestProcessEvents for Server {
        type Medium = ();

        fn process_request_bytes<'a>(
            &mut self,
            bytes: &[u8],
            scratch: &'a mut [u8],
            out: &mut impl BlockingEventOut,
            medium: (),
        ) -> Result<&'a [u8], ShrinkWrapError> {
            self.process_request_bytes(bytes, scratch, out, medium)
        }
    }
}

pub mod async_server {
    use super::*;
    use tests_common::TestProcessEventsAsync;

    pub struct Server {
        pub table: Arc<Mutex<Table>>,
        pub valid: Vec<UNib32>,
    }

    impl Server {
        handlers!(async; EventOut; flow_keys_send; .await);
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            keyed_api :: Dissector for super::Server,
            server = true, no_alloc = false, use_async = true,
            method_model = "_=immediate",
            property_model = "_=get_set",
            multi_req = true
        );
    }

    impl TestProcessEventsAsync for Server {
        type Medium = ();

        async fn process_request_bytes<'a>(
            &mut self,
            bytes: &[u8],
            scratch: &'a mut [u8],
            out: &mut impl EventOut,
            medium: (),
        ) -> Result<&'a [u8], ShrinkWrapError> {
            self.process_request_bytes(bytes, scratch, out, medium)
                .await
        }
    }
}

pub fn key(port: u16, first_seen: u64) -> FlowKey {
    let endpoint = |addr: [u8; 4], port: u16| Endpoint {
        addr,
        port: Port(port),
    };
    FlowKey {
        proto: if port.is_multiple_of(2) {
            Proto::Tcp
        } else {
            Proto::Udp
        },
        src: endpoint([10, 0, 0, 1], port),
        dst: endpoint([10, 0, 0, 2], 443),
        first_seen: DeviceMicros::from_raw(first_seen),
    }
}

/// Flows met in an order that left index 1 free: 0, 2 and 3 are in use.
pub fn table() -> Arc<Mutex<Table>> {
    Arc::new(Mutex::new(Table {
        flows: vec![
            Some((key(5000, 10), 0)),
            None,
            Some((key(5001, 20), 0)),
            Some((key(5002, 30), 0)),
        ],
    }))
}

pub fn start_blocking(path: &str, table: Arc<Mutex<Table>>) -> TestDevice {
    let _ = tracing_subscriber::fmt::try_init();
    let server = blocking_server::Server {
        table,
        valid: vec![],
    };
    tests_common::start_device(
        path,
        server,
        keyed_api::DISSECTOR_FULL_GID,
        keyed_api::DISSECTOR_WIRE_VERSION,
        blocking_server::api_impl::api_hash(),
    )
}

pub fn start_async(path: &str, table: Arc<Mutex<Table>>) -> TestDevice {
    let _ = tracing_subscriber::fmt::try_init();
    let server = async_server::Server {
        table,
        valid: vec![],
    };
    tests_common::start_device_async(
        path,
        server,
        keyed_api::DISSECTOR_FULL_GID,
        keyed_api::DISSECTOR_WIRE_VERSION,
        async_server::api_impl::api_hash(),
    )
}

/// Flows of the shuffled table: key, byte count; the first `REMOVED` were removed again.
pub const FLOWS: u16 = 12;
pub const REMOVED: u16 = 3;

pub fn flow(i: u16) -> (FlowKey, u64) {
    (key(6000 + i, 100 + u64::from(i)), 1000 * u64::from(i) + 7)
}

/// A device that met `FLOWS` flows in an order of its own (a fixed xorshift shuffle) and removed `REMOVED` of
/// them: no flow's index is its number.
pub fn shuffled_table() -> Arc<Mutex<Table>> {
    let mut order: Vec<u16> = (0..FLOWS).collect();
    let mut state = 0x2545_f491_u32;
    for i in (1..order.len()).rev() {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        order.swap(i, state as usize % (i + 1));
    }
    let mut flows: Vec<_> = order.iter().map(|&i| Some(flow(i))).collect();
    for slot in flows.iter_mut() {
        if slot
            .as_ref()
            .is_some_and(|(k, _)| (0..REMOVED).any(|i| flow(i).0 == *k))
        {
            *slot = None;
        }
    }
    assert!(
        order
            .iter()
            .enumerate()
            .any(|(at, &i)| at != usize::from(i)),
        "the shuffle moved flows"
    );
    Arc::new(Mutex::new(Table { flows }))
}
