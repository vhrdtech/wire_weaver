use super::*;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use wire_weaver::call::ErrorKind;
use wire_weaver_client::in_process::TokioClock;
use wire_weaver_server::in_process::InProcessListener;
use wire_weaver_server::{Connection, Server};

/// A [Service] served on `path` until `_stop` is dropped.
struct Bench {
    path: String,
    service: Arc<Service>,
    _stop: oneshot::Sender<()>,
}

impl Bench {
    fn start(path: &str) -> Self {
        tests_common::api!(server::api_impl).seed();
        let service = Arc::new(Service::default());
        let listener = InProcessListener::new(path, 4096);
        let (stop, stopped) = oneshot::channel::<()>();
        let for_servers = service.clone();
        tokio::spawn(async move {
            let make =
                move |connection: &Connection| server::Server::new(for_servers.clone(), connection);
            Server::new(TokioClock::new())
                .serve(server::link_config(), listener, make, async {
                    let _ = stopped.await;
                })
                .await
                .expect("serve");
        });
        Bench {
            path: path.to_owned(),
            service,
            _stop: stop,
        }
    }

    async fn connect(&self) -> Arc<client::Client> {
        let path = self.path.clone();
        Arc::new(
            client::Client::config(|c| {
                c.in_process_path(path.clone())
                    .resolver(tests_common::resolver())
            })
            .connect()
            .await
            .expect("connect"),
        )
    }

    /// Until `count` calls wait for a gate.
    async fn waiting(&self, count: u32) {
        while self.service.waiting() != count {
            tokio::task::yield_now().await;
        }
    }

    /// Give `pending` every chance to finish while it should not: requests of another connection go through.
    async fn still_pending<T>(&self, pending: &[&JoinHandle<T>]) {
        let other = self.connect().await;
        for _ in 0..20 {
            assert_eq!(
                Served::fail(&*other, &cx(), How::Not).await.unwrap(),
                Gate(0)
            );
        }
        for task in pending {
            assert!(!task.is_finished(), "answered too early");
        }
    }
}

fn cx() -> CallContext {
    CallContext::local()
}

fn wait(client: &Arc<client::Client>, gate: u8) -> JoinHandle<Gate> {
    let client = client.clone();
    tokio::spawn(async move { Served::wait(&*client, &cx(), Gate(gate)).await.unwrap() })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn calls_of_one_connection_overlap() {
    let bench = Bench::start("serve_trait/overlap");
    let client = bench.connect().await;
    let first = wait(&client, 1);
    let second = wait(&client, 2);
    // both are inside the service at once
    bench.waiting(2).await;
    // the later one answers while the earlier one still waits
    bench.service.open(Gate(2));
    assert_eq!(second.await.unwrap(), Gate(2));
    bench.still_pending(&[&first]).await;
    bench.service.open(Gate(1));
    assert_eq!(first.await.unwrap(), Gate(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_write_waits_for_the_calls_before_it_and_holds_the_read_after_it() {
    let bench = Bench::start("serve_trait/barrier");
    let client = bench.connect().await;
    let before = wait(&client, 1);
    bench.waiting(1).await;
    let write = tokio::spawn({
        let client = client.clone();
        async move {
            Served::write_mode(&*client, &cx(), Gate(5)).await.unwrap();
            Gate(5)
        }
    });
    bench.still_pending(&[&before, &write]).await;
    let read = tokio::spawn({
        let client = client.clone();
        async move { Served::read_mode(&*client, &cx()).await.unwrap() }
    });
    bench.still_pending(&[&before, &write, &read]).await;
    bench.service.open(Gate(1));
    assert_eq!(before.await.unwrap(), Gate(1));
    assert_eq!(write.await.unwrap(), Gate(5));
    assert_eq!(read.await.unwrap(), Gate(5));
    assert_eq!(bench.service.log(), ["wait 1 done", "write 5", "read 5"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_traits_errors_reach_the_client() {
    let bench = Bench::start("serve_trait/errors");
    let client = bench.connect().await;
    let fail = |how| {
        let client = client.clone();
        async move { Served::fail(&*client, &cx(), how).await.unwrap_err() }
    };
    assert!(matches!(
        fail(How::Api).await,
        CallError::Api(ServeError::Refused)
    ));
    assert_eq!(fail(How::Denied).await.kind(), Some(ErrorKind::Denied));
    assert_eq!(fail(How::Internal).await.kind(), Some(ErrorKind::Internal));
    assert_eq!(
        fail(How::Unimplemented).await.kind(),
        Some(ErrorKind::Unimplemented)
    );
    assert!(matches!(
        fail(How::Busy).await,
        CallError::Common(CommonErrorOwned::Busy)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_functions_items_flow_through_a_pump() {
    let bench = Bench::start("serve_trait/stream");
    let client = bench.connect().await;
    let (count, mut ticks) = Served::ticks(&*client, &cx(), Tick(3))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(count, Tick(3));
    for n in 0..3 {
        assert_eq!(
            futures_lite::StreamExt::next(&mut ticks)
                .await
                .unwrap()
                .unwrap(),
            Tick(n)
        );
    }
    // the trait's stream ended: the server closes the client's
    assert!(futures_lite::StreamExt::next(&mut ticks).await.is_none());
    let refused = Served::ticks(&*client, &cx(), Tick(0)).await.unwrap();
    assert!(matches!(refused, Err(ServeError::Refused)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancelled_call_drops_the_traits_future() {
    let bench = Bench::start("serve_trait/cancel");
    let client = bench.connect().await;
    let call = wait(&client, 9);
    bench.waiting(1).await;
    // the client sends a cancel for a call whose future it dropped (SRV-47)
    call.abort();
    bench.waiting(0).await;
    // the connection goes on
    assert_eq!(
        Served::fail(&*client, &cx(), How::Not).await.unwrap(),
        Gate(0)
    );
}
