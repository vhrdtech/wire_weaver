//! One API used two ways (CLT-57): [exercise] is written once against the [Counter] trait and runs against
//! [LocalCounter], a plain struct called directly (no server, no serialization, no channel), and against
//! [client::CounterClient], the generated client talking to a device or service.
//!
//! [server::CounterServer] shows the other direction: a server whose handlers forward to the same local
//! implementation, so the logic is written once and served remotely when needed.

use futures_lite::StreamExt;
use local_trait_api::{Counter, CounterError, SnapshotOwned};
use std::sync::Mutex;
use tokio::sync::broadcast;
use wire_weaver_client::local::{ItemStream, item_stream};
use wire_weaver_client::{CallError, Error, StreamError};

/// The counter in this process: a plain struct implementing [Counter].
pub struct LocalCounter {
    name: String,
    state: Mutex<State>,
    totals: broadcast::Sender<u32>,
}

struct State {
    total: u32,
    step: u32,
}

impl LocalCounter {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            state: Mutex::new(State { total: 0, step: 1 }),
            totals: broadcast::channel(16).0,
        }
    }
}

impl Counter for LocalCounter {
    async fn add(&self, by: u32) -> Result<u32, Error> {
        let total = {
            let mut state = self.state.lock().unwrap();
            state.total = state.total.wrapping_add(by.wrapping_mul(state.step));
            state.total
        };
        // nobody listening is fine
        let _ = self.totals.send(total);
        Ok(total)
    }

    async fn split(&self, parts: u32) -> Result<u32, CallError<CounterError>> {
        if parts == 0 {
            return Err(CallError::Device(CounterError::DivByZero));
        }
        Ok(self.state.lock().unwrap().total / parts)
    }

    async fn snapshot(&self) -> Result<SnapshotOwned, Error> {
        let state = self.state.lock().unwrap();
        Ok(SnapshotOwned {
            name: self.name.clone(),
            total: state.total,
            step: state.step,
        })
    }

    async fn read_step(&self) -> Result<u32, Error> {
        Ok(self.state.lock().unwrap().step)
    }

    async fn write_step(&self, step: u32) -> Result<(), Error> {
        self.state.lock().unwrap().step = step;
        Ok(())
    }

    async fn totals(&self) -> Result<ItemStream<u32>, Error> {
        let rx = self.totals.subscribe();
        Ok(item_stream(futures_lite::stream::unfold(
            rx,
            |mut rx| async move {
                match rx.recv().await {
                    Ok(total) => Some((Ok(total), rx)),
                    Err(broadcast::error::RecvError::Closed) => None,
                    // fell behind: the stream's queue was full, like a device stream's on the host
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        Some((Err(StreamError::Closed), rx))
                    }
                }
            },
        )))
    }
}

/// What [exercise] saw.
#[derive(Debug, PartialEq, Eq)]
pub struct Report {
    pub totals: Vec<u32>,
    pub step: u32,
    pub half: u32,
    pub div_by_zero: bool,
    pub snapshot: SnapshotOwned,
}

/// The call site, written once: the same code drives a [LocalCounter] and a remote counter.
pub async fn exercise(counter: &impl Counter) -> Result<Report, Box<dyn std::error::Error>> {
    let mut totals = counter.totals().await?;
    counter.write_step(2).await?;
    let step = counter.read_step().await?;
    counter.add(3).await?;
    counter.add(1).await?;
    let half = counter.split(2).await?;
    let div_by_zero = matches!(
        counter.split(0).await,
        Err(CallError::Device(CounterError::DivByZero))
    );
    let snapshot = counter.snapshot().await?;
    let mut seen = vec![];
    for _ in 0..2 {
        match totals.next().await {
            Some(total) => seen.push(total?),
            None => break,
        }
    }
    Ok(Report {
        totals: seen,
        step,
        half,
        div_by_zero,
        snapshot,
    })
}

/// A server whose handlers forward to a [LocalCounter] through the [Counter] trait: the same implementation,
/// served remotely.
pub mod server {
    use super::LocalCounter;
    use local_trait_api::{Counter, CounterError, SnapshotOwned};
    use wire_weaver::prelude::*;
    use wire_weaver_client::CallError;

    pub struct CounterServer {
        pub core: LocalCounter,
    }

    impl CounterServer {
        async fn add(&mut self, cx: &mut Context<'_, impl EventOut>, by: u32) -> RpcResult<u32> {
            let Ok(total) = self.core.add(by).await else {
                return Unimplemented.into();
            };
            // the local stream reaches local listeners, a remote one gets the total here
            let _ = api_impl::stream_data_ser().totals_send(&total, cx).await;
            Ready(total)
        }

        async fn split(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            parts: u32,
        ) -> RpcResult<Result<u32, CounterError>> {
            match self.core.split(parts).await {
                Ok(v) => Ready(Ok(v)),
                Err(CallError::Device(e)) => Ready(Err(e)),
                Err(_) => Unimplemented.into(),
            }
        }

        async fn snapshot(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> RpcResult<SnapshotOwned> {
            match self.core.snapshot().await {
                Ok(s) => Ready(s),
                Err(_) => Unimplemented.into(),
            }
        }

        async fn set_step(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            value: u32,
        ) -> SetResult<()> {
            match self.core.write_step(value).await {
                Ok(()) => Set,
                Err(_) => Unimplemented.into(),
            }
        }

        async fn get_step(&mut self, _cx: &mut Context<'_, impl EventOut>) -> GetResult<u32, ()> {
            match self.core.read_step().await {
                Ok(v) => Value(v),
                Err(_) => Unimplemented.into(),
            }
        }

        async fn totals(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            Ready(())
        }

        async fn close_totals(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

        async fn sideband_totals(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            _sideband: ww_client_server::StreamSideband,
        ) -> Option<ww_client_server::StreamSideband> {
            None
        }
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            local_trait_api :: CounterApi for super::CounterServer,
            server = true, no_alloc = false, use_async = true,
            method_model = "_=immediate",
            property_model = "_=get_set",
        );
    }
}

/// The generated client, which implements [Counter] as well.
pub mod client {
    use wire_weaver_client::{ClientConfig, Commander, WwClient};

    pub struct CounterClient {
        pub cmd: Commander,
    }

    impl WwClient for CounterClient {
        fn default_config() -> ClientConfig {
            ClientConfig::default()
        }

        fn from_cmd(cmd: Commander) -> Self {
            Self { cmd }
        }
    }

    mod api_client {
        wire_weaver::ww_codegen!(
            local_trait_api :: CounterApi for super::CounterClient,
            client = "std_client",
            impl_local_trait = "local_trait_api::Counter",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use local_trait_api::CounterDyn;
    use std::sync::Arc;
    use tests_common::{TestDevice, TestProcessEventsAsync};
    use wire_weaver::prelude::*;

    impl TestProcessEventsAsync for server::CounterServer {
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

    fn expected(name: &str) -> Report {
        Report {
            totals: vec![6, 8],
            step: 2,
            half: 4,
            div_by_zero: true,
            snapshot: SnapshotOwned {
                name: name.to_string(),
                total: 8,
                step: 2,
            },
        }
    }

    fn start_device(path: &str) -> TestDevice {
        let server = server::CounterServer {
            core: LocalCounter::new("served"),
        };
        tests_common::start_device_async(
            path,
            server,
            local_trait_api::COUNTER_API_FULL_GID,
            server::api_impl::api_hash(),
        )
    }

    async fn connect(device: &TestDevice) -> client::CounterClient {
        client::CounterClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    #[tokio::test]
    async fn generic_call_site_on_a_local_struct() {
        let local = LocalCounter::new("local");
        assert_eq!(exercise(&local).await.unwrap(), expected("local"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn generic_call_site_on_the_client() {
        let device = start_device("local_trait/client");
        let client = connect(&device).await;
        assert_eq!(exercise(&client).await.unwrap(), expected("served"));
    }

    /// Picked at run time: a trait object of either kind goes through the same call site.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn generic_call_site_on_trait_objects() {
        let local: Box<dyn CounterDyn> = Box::new(LocalCounter::new("local"));
        assert_eq!(exercise(&local).await.unwrap(), expected("local"));

        let device = start_device("local_trait/dyn");
        let remote: Arc<dyn CounterDyn> = Arc::new(connect(&device).await);
        assert_eq!(exercise(&remote).await.unwrap(), expected("served"));
    }

    /// A generic caller can run on another task: the trait's futures are `Send`.
    async fn add_on_a_task<C: Counter + 'static>(counter: Arc<C>, by: u32) -> u32 {
        tokio::spawn(async move { counter.add(by).await.unwrap() })
            .await
            .unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn futures_are_send() {
        assert_eq!(
            add_on_a_task(Arc::new(LocalCounter::new("local")), 5).await,
            5
        );
        let device = start_device("local_trait/send");
        assert_eq!(add_on_a_task(Arc::new(connect(&device).await), 5).await, 5);
    }
}
