//! The counter (`keep_trying_api::Counter`) served on this process's stdin and stdout (TR-19), as a client started
//! with `ClientConfig::command` sees it. Ends when its client leaves.

use host_server::counter::{self, Counter};
use wire_weaver_client::in_process::TokioClock;
use wire_weaver_server::Server;
use wire_weaver_server::byte_stream::{PreambleFraming, listen, stdio};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let listener = listen(
        stdio(),
        PreambleFraming::new("counter 0.1.0", counter::MESSAGE_LEN_MAX),
        TokioClock::new(),
    );
    let served = Server::new(TokioClock::new())
        .serve(
            counter::link(),
            listener,
            Counter::make,
            std::future::pending::<()>(),
        )
        .await;
    if let Err(e) = served {
        match e {}
    }
}
