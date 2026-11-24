//! A firmware's mount prefix rewrite (SRV-77, SRV-80): `MountPrefix<PathArray<8>>` puts a slot's path in front of a
//! backend's event, without a heap. `rewrite` is kept out of line so its flash and stack read off the ELF
//! (`-Z emit-stack-sizes`, `llvm-readobj --stack-sizes`): most of both is decoding the `Event`.
#![no_main]
#![no_std]

extern crate panic_semihosting;

use core::fmt::Write;
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hio};
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use wire_weaver::shrink_wrap::{RefVec, SerializeShrinkWrap, UVlq32};
use ww_client_server::mount::{MountPrefix, PathArray, RewriteError};
use ww_client_server::{Event, EventKind, ResourceIndex, resource_path};

/// Size of the event buffer.
const MESSAGE_LEN_MAX: usize = 64;
/// The longest joined path the firmware forwards.
const PATH_LEN_MAX: usize = 8;

#[entry]
fn main() -> ! {
    let mut stdout = hio::hstdout().expect("semihosting stdout");
    let slot: [ResourceIndex; 2] = resource_path([3, 1]);
    let mut prefix =
        MountPrefix::new(PathArray::<PATH_LEN_MAX>::new(), &slot).expect("the slot path fits");
    let below: [ResourceIndex; 2] = resource_path([0, 4]);
    let mut event = [0u8; MESSAGE_LEN_MAX];
    let event = Event {
        seq: UVlq32(0),
        result: Ok(EventKind::PropertyUpdate {
            path: RefVec::Slice { slice: &below },
            t: None,
            missed: None,
            sample_times: None,
            data: TailBytes(&[1, 0]),
        }),
    }
    .to_ww_bytes(&mut event)
    .expect("the event fits");
    let mut buf = [0u8; MESSAGE_LEN_MAX];
    let r = rewrite(&mut prefix, core::hint::black_box(event), &mut buf).map(<[u8]>::len);
    writeln!(stdout, "{r:?}").expect("semihosting write");

    debug::exit(debug::EXIT_SUCCESS);

    loop {}
}

#[inline(never)]
fn rewrite<'b>(
    prefix: &mut MountPrefix<PathArray<PATH_LEN_MAX>>,
    event: &[u8],
    buf: &'b mut [u8],
) -> Result<&'b [u8], RewriteError> {
    prefix.event_out(event, buf)
}
