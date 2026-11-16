//! Dispatch cost of a counted stream under a 2-level indexed resource (R10): `tests/counted_stream_api`'s
//! `group[].item[].event`, a framework-only fixture with the same shape (array under array, one counted event
//! stream at the leaf) `ww_gpio::Bank`/`Pin` had before this bench moved off the stdlib GPIO API (`qemu-ww-gpio`
//! before the rename), its stream counted as `ww_gpio::Pin::event` is.
#![no_main]
#![no_std]

extern crate panic_semihosting;

use core::fmt::Write;
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hio};
use wire_weaver::prelude::*;
use counted_stream_api::Edge;
use ww_client_server::stream::StreamHead;
use ww_stream::tx::Counted;

#[entry]
fn main() -> ! {
    let mut stdout = hio::hstdout().unwrap();

    let mut scratch = [0u8; 512];
    let mut event_scratch = [0u8; 512];
    let mut server = ServerState {
        item_events: Counted::new(),
        stream_sessions: 0,
    };

    let r = api_server::stream_data_ser()
        .group(0)
        .item(0)
        .event(&StreamHead::at(0), &Edge::Rising, &mut scratch);
    writeln!(stdout, "{r:02x?}").unwrap();

    let event = [1u8, 2, 3];
    let mut out = EventWriter::new(DummyTx, &mut event_scratch);
    let r = server.process_request_bytes(&event, &mut scratch, &mut out, ());
    writeln!(stdout, "{r:?}").unwrap();

    // exit QEMU
    debug::exit(debug::EXIT_SUCCESS);

    loop {}
}

struct DummyTx;
impl wire_weaver::BlockingMessageSink for DummyTx {
    fn send(&mut self, _message: &[u8]) -> Result<(), ()> {
        Ok(())
    }
}

pub struct ServerState {
    item_events: Counted,
    stream_sessions: u32,
}

impl ServerState {
    /// Valid indices of `group[]`.
    fn valid_indices_root_group(&mut self) -> ValidIndices<'_> {
        ValidIndices::range_u32(0..8)
    }

    /// Valid indices of `group[].item[]`.
    fn valid_indices_root_group_item(
        &mut self,
        _index_chain: [ww_client_server::ResourceIndex; 1],
    ) -> ValidIndices<'_> {
        ValidIndices::range_u32(0..16)
    }

    /// Opens the stream (a new session): `Ok` / `Ready` starts it.
    fn group_item_event(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index_chain: [ww_client_server::ResourceIndex; 2],
    ) -> RpcResult<()> {
        RpcResult::Ready(())
    }

    /// The host closed the stream.
    fn close_group_item_event(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index_chain: [ww_client_server::ResourceIndex; 2],
    ) {
    }

    /// One state for every item: the stream never opens on this fixture.
    fn stream_group_item_event(
        &mut self,
        _index_chain: [ww_client_server::ResourceIndex; 2],
    ) -> &mut Counted {
        &mut self.item_events
    }

    fn new_stream_session(&mut self) -> u32 {
        self.stream_sessions = self.stream_sessions.wrapping_add(1);
        self.stream_sessions
    }
}

pub mod api_server {
    wire_weaver::ww_codegen!(
        counted_stream_api :: CountedStreamBench for super::ServerState,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        property_model = "_=get_set",
        debug_to_file = "./target/generated_mcu_qemu_counted_stream_server.rs"
    );
}
