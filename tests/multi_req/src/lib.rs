//! Multi requests (`MultiCall`, `MultiRead`, `MultiWrite`) on a `multi_req = true` server, and a table of malformed
//! ones from the host: every one is answered with an error (for the whole request or per index), none panics the
//! device.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tests_common::TestDevice;
    use tokio::sync::oneshot;
    use wire_weaver::prelude::*;
    use wire_weaver::shrink_wrap::either_any_vec::{EitherAnyVec, EitherAnyVecIter};
    use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
    use wire_weaver::shrink_wrap::{BufReader, UVlq32Backfill};
    use wire_weaver_client::internal::Command;
    use wire_weaver_client::{Commander, Error};
    use ww_client_server::{
        ErrorKindOwned, ErrorOwned, MultiArgsOwned, MultiIndexOwned, PathKindOwned,
        RequestKindOwned, RequestOwned,
    };

    /// State shared by the device thread and the test
    #[derive(Default)]
    struct Shared {
        a: u8,
        b: u16,
        pin_level: [bool; PINS as usize],
        pin_mode: [u8; PINS as usize],
        /// `deferred` handler calls
        deferred_calls: u32,
    }

    const PINS: u32 = 4;

    /// Same handlers for the blocking and async servers: `$($async)?` and the event out trait differ.
    macro_rules! handlers {
        ($($async:ident)?; $out:path) => {
            $($async)? fn set_a(&mut self, _cx: &mut Context<'_, impl $out>, value: u8) -> SetResult<()> {
                self.shared.lock().unwrap().a = value;
                Set
            }

            $($async)? fn get_a(&mut self, _cx: &mut Context<'_, impl $out>) -> GetResult<u8, ()> {
                Value(self.shared.lock().unwrap().a)
            }

            $($async)? fn set_b(&mut self, _cx: &mut Context<'_, impl $out>, value: u16) -> SetResult<()> {
                self.shared.lock().unwrap().b = value;
                Set
            }

            $($async)? fn get_b(&mut self, _cx: &mut Context<'_, impl $out>) -> GetResult<u16, ()> {
                Value(self.shared.lock().unwrap().b)
            }

            $($async)? fn add(&mut self, _cx: &mut Context<'_, impl $out>, x: u8, y: u8) -> RpcResult<u8> {
                Ready(x.wrapping_add(y))
            }

            $($async)? fn deferred(&mut self, _cx: &mut Context<'_, impl $out>) -> RpcResult<u8> {
                self.shared.lock().unwrap().deferred_calls += 1;
                Deferred
            }

            $($async)? fn defers_anyway(&mut self, _cx: &mut Context<'_, impl $out>) -> RpcResult<u8> {
                Deferred
            }

            $($async)? fn absent(&mut self, _cx: &mut Context<'_, impl $out>) -> RpcResult<()> {
                Unimplemented.into()
            }

            fn valid_indices_root_pin(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..PINS)
            }

            $($async)? fn pin_set_level(
                &mut self,
                _cx: &mut Context<'_, impl $out>,
                index_chain: [UNib32; 1],
                high: bool,
            ) -> RpcResult<()> {
                self.shared.lock().unwrap().pin_level[index_chain[0].0 as usize] = high;
                Ready(())
            }

            $($async)? fn set_pin_mode(
                &mut self,
                _cx: &mut Context<'_, impl $out>,
                index_chain: [UNib32; 1],
                value: u8,
            ) -> SetResult<()> {
                self.shared.lock().unwrap().pin_mode[index_chain[0].0 as usize] = value;
                Set
            }

            $($async)? fn get_pin_mode(
                &mut self,
                _cx: &mut Context<'_, impl $out>,
                index_chain: [UNib32; 1],
            ) -> GetResult<u8, ()> {
                Value(self.shared.lock().unwrap().pin_mode[index_chain[0].0 as usize])
            }
        };
    }

    mod blocking_server {
        use super::*;
        use tests_common::TestProcessEvents;

        pub struct Server {
            pub shared: Arc<Mutex<Shared>>,
        }

        impl Server {
            handlers!(; BlockingEventOut);
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                multi_req_api :: MultiReq for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "deferred=deferred, _=immediate",
                property_model = "_=get_set",
                multi_req = true
                // debug_to_file = "../../target/tests_multi_req_server.rs"
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

    mod async_server {
        use super::*;
        use tests_common::TestProcessEventsAsync;

        pub struct Server {
            pub shared: Arc<Mutex<Shared>>,
        }

        impl Server {
            handlers!(async; EventOut);
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                multi_req_api :: MultiReq for super::Server,
                server = true, no_alloc = true, use_async = true,
                method_model = "deferred=deferred, _=immediate",
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

    mod std_client {
        use wire_weaver_client::{ClientConfig, Commander, WwClient};

        pub struct StdClient {
            pub cmd: Commander,
        }

        impl WwClient for StdClient {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                multi_req_api :: MultiReq for super::StdClient,
                client = "std_client",
            );
        }
    }

    use std_client::StdClient;

    /// Same wire format as the generated arguments of `add`
    #[derive_shrink_wrap(owned)]
    struct AddArgs {
        x: u8,
        y: u8,
    }

    /// Same wire format as the generated arguments of `Pin::set_level`
    #[derive_shrink_wrap(owned)]
    struct SetLevelArgs {
        high: bool,
    }

    // ids in the API
    const A: u32 = 0;
    const B: u32 = 1;
    const ADD: u32 = 2;
    const DEFERRED: u32 = 3;
    const DEFERS_ANYWAY: u32 = 4;
    const ABSENT: u32 = 5;
    const PIN: u32 = 6;
    const PIN_SET_LEVEL: u32 = 0;
    const PIN_MODE: u32 = 1;

    fn start_blocking(path: &str) -> (TestDevice, Arc<Mutex<Shared>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let server = blocking_server::Server {
            shared: shared.clone(),
        };
        let device = tests_common::start_device(
            path,
            server,
            multi_req_api::MULTI_REQ_FULL_GID,
            blocking_server::api_impl::api_hash(),
        );
        (device, shared)
    }

    fn start_async(path: &str) -> (TestDevice, Arc<Mutex<Shared>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let server = async_server::Server {
            shared: shared.clone(),
        };
        let device = tests_common::start_device_async(
            path,
            server,
            multi_req_api::MULTI_REQ_FULL_GID,
            async_server::api_impl::api_hash(),
        );
        (device, shared)
    }

    async fn connect(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    /// Send a request as is and wait for the answer: the value bytes, or the device's error.
    async fn raw(cmd: &Commander, path: &[u32], kind: RequestKindOwned) -> Result<Vec<u8>, Error> {
        let req = RequestOwned {
            seq: UVlq32Backfill(0),
            path_kind: PathKindOwned::Absolute {
                path: path.iter().copied().map(UNib32).collect(),
            },
            kind,
        };
        let bytes = req.to_ww_bytes_owned().unwrap();
        let (tx, rx) = oneshot::channel();
        cmd.send(Command::SendMessage {
            bytes,
            done_tx: Some((tx, Duration::from_secs(2))),
        })
        .await?;
        rx.await.expect("event loop answers")
    }

    fn read(multi_idx: MultiIndexOwned, in_each_array_id: Option<u32>) -> RequestKindOwned {
        RequestKindOwned::MultiRead {
            multi_idx,
            in_each_array_id: in_each_array_id.map(UNib32),
        }
    }

    fn call(
        multi_idx: MultiIndexOwned,
        in_each_array_id: Option<u32>,
        multi_args: MultiArgsOwned,
    ) -> RequestKindOwned {
        RequestKindOwned::MultiCall {
            multi_idx,
            in_each_array_id: in_each_array_id.map(UNib32),
            multi_args,
        }
    }

    fn write(
        multi_idx: MultiIndexOwned,
        in_each_array_id: Option<u32>,
        multi_data: MultiArgsOwned,
    ) -> RequestKindOwned {
        RequestKindOwned::MultiWrite {
            multi_idx,
            in_each_array_id: in_each_array_id.map(UNib32),
            multi_data,
        }
    }

    fn list(ids: &[u32]) -> MultiIndexOwned {
        MultiIndexOwned::List(ids.iter().copied().map(UNib32).collect())
    }

    fn range(r: std::ops::Range<u32>) -> MultiIndexOwned {
        MultiIndexOwned::Range(UNib32(r.start)..UNib32(r.end))
    }

    fn ser<T: SerializeShrinkWrapOwned>(value: &T) -> Vec<u8> {
        value.to_ww_bytes_owned().unwrap()
    }

    fn same<T: SerializeShrinkWrapOwned>(value: &T) -> MultiArgsOwned {
        MultiArgsOwned::Same(ser(value))
    }

    /// `MultiArgs::Different`: a serialized `Vec<Vec<u8>>`, one entry per index
    fn different(each: Vec<Vec<u8>>) -> MultiArgsOwned {
        MultiArgsOwned::Different(ser(&each))
    }

    /// Answer to one index
    #[derive(Debug)]
    enum Item {
        U8(u8),
        U16(u16),
        Unit,
        Err(ErrorKindOwned),
        /// Error of any kind
        AnyErr,
    }

    /// Answer to the whole request
    #[derive(Debug)]
    enum Expect {
        Items(Vec<Item>),
        /// The whole request failed with this error
        Fails(ErrorKindOwned),
    }

    fn same_kind(a: &ErrorKindOwned, b: &ErrorKindOwned) -> bool {
        std::mem::discriminant(a) == std::mem::discriminant(b)
    }

    /// Next answer item, None at the end
    fn next<T: DeserializeShrinkWrapOwned>(
        iter: &mut EitherAnyVecIter<'_>,
    ) -> Option<Result<T, ErrorOwned>> {
        iter.next_owned::<ErrorOwned, T>().ok().map(Into::into)
    }

    /// Checks the answer items one by one. The number of items isn't stored (`EitherAnyVec`), items past the
    /// expected ones aren't checked.
    fn check_items(name: &str, bytes: &[u8], expected: &[Item]) {
        let mut rd = BufReader::new(bytes);
        let vec = EitherAnyVec::des_shrink_wrap(&mut rd).unwrap();
        let mut iter = vec.iter();
        for (i, item) in expected.iter().enumerate() {
            let ok = match item {
                Item::U8(v) => matches!(next::<u8>(&mut iter), Some(Ok(x)) if x == *v),
                Item::U16(v) => matches!(next::<u16>(&mut iter), Some(Ok(x)) if x == *v),
                Item::Unit => matches!(next::<()>(&mut iter), Some(Ok(()))),
                Item::Err(kind) => {
                    matches!(next::<()>(&mut iter), Some(Err(e)) if same_kind(&e.kind, kind))
                }
                Item::AnyErr => matches!(next::<()>(&mut iter), Some(Err(_))),
            };
            assert!(ok, "{name}: item {i} is not {item:?}, answer {bytes:02x?}");
        }
        if expected.is_empty() {
            assert!(
                bytes.is_empty(),
                "{name}: no items expected, answer {bytes:02x?}"
            );
        }
    }

    /// Requests with what they must be answered with, in order (writes are read back by later cases).
    fn cases() -> Vec<(&'static str, Vec<u32>, RequestKindOwned, Expect)> {
        use ErrorKindOwned as K;
        use Expect::*;
        use Item::*;
        let deep_16 = vec![A; 16];
        let deep_15 = vec![A; 15];
        // no such resource at the first level
        let deep_15_bad = vec![99; 15];
        vec![
            (
                "read list",
                vec![],
                read(list(&[A, B]), None),
                Items(vec![U8(0), U16(0)]),
            ),
            (
                "write different",
                vec![],
                write(
                    list(&[A, B]),
                    None,
                    different(vec![ser(&7u8), ser(&0x1234u16)]),
                ),
                Items(vec![Unit, Unit]),
            ),
            (
                "read range",
                vec![],
                read(range(A..B + 1), None),
                Items(vec![U8(7), U16(0x1234)]),
            ),
            (
                "read mask32",
                vec![],
                read(MultiIndexOwned::Mask32(0b11), None),
                Items(vec![U8(7), U16(0x1234)]),
            ),
            (
                "read empty range",
                vec![],
                read(MultiIndexOwned::Range(UNib32(5)..UNib32(2)), None),
                Items(vec![]),
            ),
            (
                "read mask32 all bits",
                vec![],
                read(MultiIndexOwned::Mask32(u32::MAX), None),
                Items(vec![U8(7), U16(0x1234), Err(K::OperationNotSupported)]),
            ),
            (
                // answers to every index don't fit, the whole request fails instead of going on for 4G indices
                "read huge range",
                vec![],
                read(range(0..u32::MAX), None),
                Fails(K::ResponseSerFailed),
            ),
            (
                "read huge ids",
                vec![],
                read(list(&[u32::MAX, A]), None),
                Items(vec![Err(K::BadPath), U8(7)]),
            ),
            (
                "path at max depth",
                deep_16.clone(),
                read(list(&[A]), None),
                Fails(K::BadPath),
            ),
            (
                "path + in_each_array_id over max depth",
                deep_15.clone(),
                read(list(&[A]), Some(A)),
                Fails(K::BadPath),
            ),
            (
                "path one below max depth",
                deep_15_bad,
                read(list(&[A]), None),
                Items(vec![Err(K::BadPath)]),
            ),
            (
                "call same",
                vec![],
                call(list(&[ADD, ADD]), None, same(&AddArgs { x: 1, y: 2 })),
                Items(vec![U8(3), U8(3)]),
            ),
            (
                "call different",
                vec![],
                call(
                    list(&[ADD, ADD]),
                    None,
                    different(vec![
                        ser(&AddArgs { x: 1, y: 2 }),
                        ser(&AddArgs { x: 10, y: 20 }),
                    ]),
                ),
                Items(vec![U8(3), U8(30)]),
            ),
            (
                "call different, fewer args than indices",
                vec![],
                call(
                    list(&[ADD, ADD]),
                    None,
                    different(vec![ser(&AddArgs { x: 1, y: 2 })]),
                ),
                Items(vec![U8(3), Err(K::ArgsDesFailed)]),
            ),
            (
                "call different, not a list",
                vec![],
                call(list(&[ADD]), None, MultiArgsOwned::Different(vec![0xff; 3])),
                Fails(K::ArgsDesFailed),
            ),
            (
                "call same, args too short",
                vec![],
                call(list(&[ADD]), None, MultiArgsOwned::Same(vec![])),
                Items(vec![Err(K::ArgsDesFailed)]),
            ),
            (
                "call deferred",
                vec![],
                call(list(&[DEFERRED, ADD]), None, same(&AddArgs { x: 2, y: 2 })),
                Items(vec![Err(K::OperationNotSupported), U8(4)]),
            ),
            (
                "call defers anyway",
                vec![],
                call(list(&[DEFERS_ANYWAY]), None, MultiArgsOwned::Same(vec![])),
                Items(vec![Err(K::OperationNotSupported)]),
            ),
            (
                "call absent and a property",
                vec![],
                call(list(&[ABSENT, A]), None, MultiArgsOwned::Same(vec![])),
                Items(vec![Err(K::Unimplemented), Err(K::OperationNotSupported)]),
            ),
            (
                "call in each pin",
                vec![PIN],
                call(
                    range(0..PINS),
                    Some(PIN_SET_LEVEL),
                    same(&SetLevelArgs { high: true }),
                ),
                Items(vec![Unit, Unit, Unit, Unit]),
            ),
            (
                "call in each pin, past the last one",
                vec![PIN],
                call(
                    range(2..PINS + 2),
                    Some(PIN_SET_LEVEL),
                    same(&SetLevelArgs { high: true }),
                ),
                Items(vec![Unit, Unit, Err(K::BadIndex), Err(K::BadIndex)]),
            ),
            (
                "write in each pin",
                vec![PIN],
                write(MultiIndexOwned::Mask32(0b1001), Some(PIN_MODE), same(&5u8)),
                Items(vec![Unit, Unit]),
            ),
            (
                "read in each pin",
                vec![PIN],
                read(range(0..PINS), Some(PIN_MODE)),
                Items(vec![U8(5), U8(0), U8(0), U8(5)]),
            ),
            (
                "write a method",
                vec![],
                write(list(&[ADD, A]), None, same(&9u8)),
                Items(vec![Err(K::OperationNotSupported), Unit]),
            ),
            (
                "write, data too short",
                vec![],
                write(list(&[B]), None, same(&1u8)),
                Items(vec![Err(K::PropertyDesFailed)]),
            ),
            (
                "write different, garbage entry",
                vec![],
                write(list(&[A]), None, different(vec![vec![]])),
                Items(vec![Err(K::PropertyDesFailed)]),
            ),
            (
                "read through a method",
                vec![ADD],
                read(list(&[0, 1]), None),
                Items(vec![AnyErr, AnyErr]),
            ),
            (
                // pin 1 itself, a trait
                "read a trait",
                vec![],
                read(list(&[PIN]), Some(1)),
                Items(vec![Err(K::OperationNotSupported)]),
            ),
        ]
    }

    async fn run_cases(client: &mut StdClient, shared: &Mutex<Shared>) {
        for (name, path, kind, expect) in cases() {
            let r = raw(&client.cmd, &path, kind).await;
            match (&expect, r) {
                (Expect::Items(items), Ok(bytes)) => check_items(name, &bytes, items),
                (Expect::Fails(kind), Err(Error::RemoteError(e))) if same_kind(&e.kind, kind) => {}
                (expect, r) => panic!("{name}: expected {expect:?}, got {r:?}"),
            }
        }
        {
            let s = shared.lock().unwrap();
            assert_eq!(
                s.deferred_calls, 0,
                "deferred handler is not called in a multi request"
            );
            assert_eq!(s.pin_level, [true; PINS as usize]);
            assert_eq!(s.a, 9);
        }
        // device still answers normal requests
        assert_eq!(client.add(40, 2).call().await.unwrap(), 42);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn malformed_multi_requests_blocking() {
        let (device, shared) = start_blocking("multi_req/malformed_blocking");
        let mut client = connect(&device).await;
        run_cases(&mut client, &shared).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn malformed_multi_requests_async() {
        let (device, shared) = start_async("multi_req/malformed_async");
        let mut client = connect(&device).await;
        run_cases(&mut client, &shared).await;
    }

    /// Valid requests with every byte changed and cut short, straight into `process_request_bytes`: answered or
    /// refused, never a panic.
    #[test]
    fn mutated_requests() {
        let mut server = blocking_server::Server {
            shared: Arc::new(Mutex::new(Shared::default())),
        };
        let mut scratch = [0u8; tests_common::MAX_MESSAGE_LEN];
        let mut events = [0u8; 4096];
        let mut n = 0usize;
        for (_, path, kind, _) in cases() {
            let req = RequestOwned {
                seq: UVlq32Backfill(1),
                path_kind: PathKindOwned::Absolute {
                    path: path.iter().copied().map(UNib32).collect(),
                },
                kind,
            };
            let bytes = req.to_ww_bytes_owned().unwrap();
            let mut variants = vec![];
            for len in 0..bytes.len() {
                variants.push(bytes[..len].to_vec());
            }
            for i in 0..bytes.len() {
                for v in [0x00, 0xff, 0x0f, 0xf0, 0x55, 0xaa, 0x80, 0x01] {
                    let mut b = bytes.clone();
                    b[i] = v;
                    variants.push(b);
                }
            }
            for b in variants {
                let mut out = wire_weaver::EventQueue::new(&mut events);
                let _ = server.process_request_bytes(&b, &mut scratch, &mut out, ());
                n += 1;
            }
        }
        assert!(n > 1000, "{n} requests");
        // also plain calls, reads and writes in single requests
        let single = [
            || RequestKindOwned::Read,
            || RequestKindOwned::Write {
                data: TailBytesOwned(vec![1, 2]),
            },
            || RequestKindOwned::Call {
                args: TailBytesOwned(vec![1, 2]),
            },
            || RequestKindOwned::Introspect,
        ];
        for kind in single {
            for path in [vec![], vec![ADD], vec![PIN, 1, PIN_MODE], vec![PIN, 9, 0]] {
                let req = RequestOwned {
                    seq: UVlq32Backfill(1),
                    path_kind: PathKindOwned::Absolute {
                        path: path.iter().copied().map(UNib32).collect(),
                    },
                    kind: kind(),
                };
                let bytes = req.to_ww_bytes_owned().unwrap();
                for i in 0..bytes.len() {
                    for v in [0x00, 0xff, 0x55, 0xaa] {
                        let mut b = bytes.clone();
                        b[i] = v;
                        let mut out = wire_weaver::EventQueue::new(&mut events);
                        let _ = server.process_request_bytes(&b, &mut scratch, &mut out, ());
                    }
                }
            }
        }
    }
}
