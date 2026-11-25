//! The bench supply simulated on this PC, served over WebSocket: two channels, each loading its output with a
//! fixed resistance. Ctrl-C stops it.
//!
//! `cargo run -p bench_supply --example bench_supply_sim`, then in a second terminal
//! `cargo run -p bench_supply --example bench_supply_bench`.

use std::num::NonZeroU16;

use anyhow::Result;
use clap::Parser;
use futures_util::Stream;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};

#[derive(Parser)]
struct Args {
    /// TCP port of the WebSocket server
    #[arg(long, default_value_t = NonZeroU16::new(8081).expect("not zero"))]
    port: NonZeroU16,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let listener = TcpListener::bind(("127.0.0.1", args.port.get())).await?;
        println!(
            "bench supply at ws://127.0.0.1:{}{}",
            args.port,
            bench_supply::WS_PATH
        );
        let stop = async {
            _ = tokio::signal::ctrl_c().await;
        };
        bench_supply::simulated(incoming(listener), stop).await;
        println!("stopped");
        Ok(())
    })
}

/// The clients of `listener`, one at a time as `IoSocket` takes them, with Nagle off.
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
