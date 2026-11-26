//! The blinky device simulated on this PC, served over WebSocket. Its LED is a line printed each time it changes.
//! Ctrl-C stops it.
//!
//! `cargo run -p blinky --example blinky_sim`, then in a second terminal `cargo run -p blinky --example blinky_host`.

use anyhow::Result;
use blinky::Led;
use clap::Parser;
use futures_util::Stream;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};

#[derive(Parser)]
struct Args {
    /// TCP port of the WebSocket server; 0 takes a free one, which the first line names
    #[arg(long, default_value_t = 8080)]
    port: u16,
}

// --8<-- [start:led]
/// The simulated LED: it prints its state.
struct PrintedLed;

impl Led for PrintedLed {
    fn set(&mut self, on: bool) {
        println!("LED {}", if on { "on" } else { "off" });
    }
}
// --8<-- [end:led]

fn main() -> Result<()> {
    let args = Args::parse();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let listener = TcpListener::bind(("127.0.0.1", args.port)).await?;
        println!(
            "blinky at ws://{}{}",
            listener.local_addr()?,
            blinky::WS_PATH
        );
        let stop = async {
            _ = tokio::signal::ctrl_c().await;
        };
        blinky::simulated(incoming(listener), Box::leak(Box::new(PrintedLed)), stop).await;
        Ok(())
    })
}

/// The clients of `listener`, one at a time, with Nagle off.
fn incoming(listener: TcpListener) -> impl Stream<Item = Compat<TcpStream>> + Unpin {
    Box::pin(futures_util::stream::unfold(
        listener,
        |listener| async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer)) => {
                        if let Err(e) = stream.set_nodelay(true) {
                            println!("{peer}: TCP_NODELAY: {e}");
                        }
                        println!("client {peer}");
                        return Some((stream.compat(), listener));
                    }
                    Err(e) => println!("accept: {e}"),
                }
            }
        },
    ))
}
