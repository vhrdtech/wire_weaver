//! A server on stdio (TR-19): the client starts the `stdio_server` binary with `ClientConfig::command` and talks over
//! its stdin and stdout, as over `ssh host svc --stdio`.

use std::time::Duration;

use host_server::counter::{self, CounterClient};

/// How long a test waits for what it expects before calling it a hang.
const SOON: Duration = Duration::from_secs(20);

async fn started() -> CounterClient {
    CounterClient::config(|c| {
        c.command(env!("CARGO_BIN_EXE_stdio_server"), [] as [&str; 0])
            .resolver(tests_common::resolver())
            .link_setup_timeout(SOON)
    })
    .connect()
    .await
    .expect("the command's server answers")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_on_stdio_is_reached_through_its_command() {
    tests_common::api!(counter::api_impl).seed();
    // every client starts a server of its own
    let first = started().await;
    let second = started().await;
    for client in [&first, &second] {
        let ticks = tokio::time::timeout(SOON, client.read_ticks().read())
            .await
            .expect("an answer")
            .expect("ticks");
        assert_eq!(ticks.value, counter::TICKS);
    }
}
