//! The trait without `Send` (SRV-73): one call site against the local service and the client, and the service
//! served on one thread with its `!Send` item streams pumped.

use crate::not_send::{LocalService, client, server};
use futures_lite::StreamExt;
use serve_trait_api::{Gate, How, LocalServed, LocalServedDyn, ServeError, Tick};
use std::rc::Rc;
use tokio::sync::oneshot;
use tokio::task::LocalSet;
use wire_weaver::CallContext;
use wire_weaver::call::{CallError, ErrorKind};
use wire_weaver_client::in_process::TokioClock;
use wire_weaver_server::in_process::InProcessListener;
use wire_weaver_server::{Local, LocalConnection, Server};

/// The same calls through any implementation of the trait, and what they gave.
async fn exercise(served: &impl LocalServed) -> Vec<String> {
    let cx = CallContext::local();
    let mut report = vec![];
    served.write_mode(&cx, Gate(7)).await.unwrap();
    report.push(format!("mode {:?}", served.read_mode(&cx).await.unwrap()));
    report.push(format!(
        "wait {:?}",
        served.wait(&cx, Gate(2)).await.unwrap()
    ));
    assert!(matches!(
        served.fail(&cx, How::Api).await,
        Err(CallError::Api(ServeError::Refused))
    ));
    let denied = served.fail(&cx, How::Denied).await.unwrap_err();
    report.push(format!("fail {:?}", denied.kind()));
    let (count, items) = served.ticks(&cx, Tick(3)).await.unwrap().unwrap();
    let items: Vec<_> = items.map(|item| item.unwrap().0).collect().await;
    report.push(format!("ticks {count:?} {items:?}"));
    let refused = served.ticks(&cx, Tick(0)).await.unwrap();
    assert!(matches!(refused, Err(ServeError::Refused)));
    report
}

#[test]
fn local_service_and_client_answer_alike_on_one_thread() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    LocalSet::new().block_on(&runtime, async {
        tests_common::api!(server::api_impl).seed();
        let path = "serve_trait/not_send";
        let service = Rc::new(LocalService::default());
        let listener = InProcessListener::new(path, 4096);
        let (stop, stopped) = oneshot::channel::<()>();
        let for_servers = service.clone();
        // the service is an `Rc`: the server runs on this thread
        let serving = tokio::task::spawn_local(async move {
            let make = Local(move |connection: &LocalConnection| {
                server::Server::new(for_servers.clone(), connection)
            });
            Server::new(TokioClock::new())
                .serve(crate::server::link_config(), listener, make, async {
                    let _ = stopped.await;
                })
                .await
                .expect("serve");
        });

        let here: Rc<dyn LocalServedDyn> = Rc::new(LocalService::default());
        let direct = exercise(&here).await;

        let remote: Rc<dyn LocalServedDyn> = Rc::new(
            client::Client::config(|c| {
                c.in_process_path(path.to_owned())
                    .resolver(tests_common::resolver())
            })
            .connect()
            .await
            .expect("connect"),
        );
        let served = exercise(&remote).await;

        assert_eq!(direct, served);
        assert_eq!(
            direct,
            [
                "mode Gate(7)",
                "wait Gate(2)",
                &format!("fail {:?}", Some(ErrorKind::Denied)),
                "ticks Tick(3) [0, 1, 2]",
            ]
        );
        // the served items came from the service's `!Send` stream, through the connection's local pump
        assert_eq!(service.ticks_made(), 3);
        assert_eq!(service.log(), ["write 7", "read 7", "wait 2 after 0 ticks"]);
        drop(stop);
        serving.await.expect("server task");
    });
}
