//! Bare firmware images of a method's reply as generated server code encodes it, built by the `size_bench_reply`
//! runner one level up to see what a server's reply costs on a small CPU. Entry point, buffers and panic handler
//! come from `size_bench_rt` (shrink_wrap's size bench).
//!
//! Every case is a `run(rx, tx) -> usize`: decode what is in `rx`, encode into `tx`, return the bytes written.
#![no_std]
#![no_main]

use shrink_wrap::prelude::*;
#[cfg(feature = "reply")]
use size_bench_rt::{sink, src};

// `$sized` is the size assumption directive: nothing for the evolvable default, `, sized` with `final`.
macro_rules! wave {
    ($($sized:tt)*) => {
        /// One line's waveform: a plain struct of integers and flags, the return value
        #[derive_shrink_wrap(borrowed, derive(Clone, Copy) $($sized)*)]
        pub struct Wave {
            pub line: u8,
            pub idle: bool,
            pub hold: bool,
            pub xor: bool,
            pub delay: u32,
            pub high: u32,
            pub low: u32,
            pub n: u32,
        }
    };
}

#[cfg(not(feature = "final"))]
wave!();
#[cfg(feature = "final")]
wave!(, sized);

#[cfg(feature = "reply")]
const WAVE: Wave = Wave {
    line: 1,
    idle: true,
    hold: false,
    xor: true,
    delay: 2,
    high: 3,
    low: 4,
    n: 5,
};

/// No shrink_wrap code at all: the image the other case is compared with
#[cfg(feature = "empty")]
pub fn run(rx: &[u8], tx: &mut [u8]) -> usize {
    tx[0] = rx.len() as u8;
    1
}

/// A method's reply as `ww_codegen!` server code encodes it (`ser_method_output`): the return value alone into
/// one scratch buffer, then the event around its bytes, and the error event every generated server sends when
/// that fails.
#[cfg(feature = "reply")]
pub fn run(_rx: &[u8], tx: &mut [u8]) -> usize {
    use ww_client_server::{Error, Event, EventKind, util::ser_err_event};

    fn reply<'a>(
        output: &Wave,
        seq: u32,
        scratch_args: &mut [u8],
        scratch_event: &'a mut [u8],
    ) -> Result<&'a [u8], Error<'static>> {
        let mut wr = BufWriter::new(scratch_args);
        output
            .ser_shrink_wrap(&mut wr)
            .map_err(|_| Error::response_ser_failed(1))?;
        #[cfg(not(feature = "finish_for"))]
        let output_bytes = wr.finish_and_take();
        #[cfg(feature = "finish_for")]
        let output_bytes = wr.finish_for_and_take::<Wave>();
        let output_bytes = output_bytes.map_err(|_| Error::response_ser_failed(2))?;
        let mut event_wr = BufWriter::new(scratch_event);
        let event = Event {
            seq: UVlq32(seq),
            result: Ok(EventKind::Value {
                data: shrink_wrap::tail_bytes::TailBytes(output_bytes),
            }),
        };
        event
            .ser_shrink_wrap(&mut event_wr)
            .map_err(|_| Error::response_ser_failed(3))?;
        event_wr
            .finish_and_take()
            .map_err(|_| Error::response_ser_failed(4))
    }

    let mut scratch = [0u8; 64];
    let (head, tail) = tx.split_at_mut(150);
    let seq = src(5);
    match reply(&src(WAVE), seq, &mut scratch, head) {
        Ok(bytes) => bytes.len(),
        Err(e) => match ser_err_event(tail, seq, e) {
            Ok(bytes) => bytes.len(),
            Err(e) => {
                sink(e);
                0
            }
        },
    }
}

size_bench_rt::entry!(run);
