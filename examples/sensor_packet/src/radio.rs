//! The radio link of the example, run on one PC: the node writes a reading into a frame, the gateway reads it back,
//! once borrowed and once owned. [transcript] is what the example `sensor_packet_radio` prints.

use crate::{FRAME_BYTES_MAX, ROOF, Reading, ReadingOwned, hex, node_sends};
use shrink_wrap::prelude::*;
use std::fmt::Write;

// --8<-- [start:receive]
/// Reads a reading from a received frame without copying: `station` points into `bytes`.
pub fn gateway_receives(bytes: &[u8]) -> Result<Reading<'_>, ShrinkWrapError> {
    Reading::from_ww_bytes(bytes)
}
// --8<-- [end:receive]

// --8<-- [start:keep]
/// Reads a reading from a received frame into a value of its own, so the frame's buffer can take the next one.
pub fn gateway_keeps(bytes: &[u8], log: &mut Vec<ReadingOwned>) -> Result<(), ShrinkWrapError> {
    let reading = ReadingOwned::from_ww_bytes_owned(bytes)?;
    log.push(reading);
    Ok(())
}
// --8<-- [end:keep]

/// Where the humidity of [ROOF] is in its bytes: after the station's 4 bytes, the temperature's 2 and the byte that
/// holds the humidity's flag.
const HUMIDITY_BYTE: usize = 7;

/// What the example prints: the frame the node sends, what the gateway reads from it, from a corrupted and from a cut
/// frame, and what it keeps.
///
/// # Panics
///
/// When the example's own reading does not fit a frame, or the gateway can't read a whole frame back.
pub fn transcript() -> String {
    let mut out = String::new();
    let mut frame = [0u8; FRAME_BYTES_MAX];
    let bytes = node_sends(&ROOF, &mut frame).expect("a reading fits a frame");
    writeln!(out, "sent {} bytes: {}", bytes.len(), hex(bytes)).expect("writes to a String");

    let reading = gateway_receives(bytes).expect("a whole frame reads back");
    writeln!(out, "received: {reading:?}").expect("writes to a String");

    let mut corrupt = [0u8; FRAME_BYTES_MAX];
    let corrupt = &mut corrupt[..bytes.len()];
    corrupt.copy_from_slice(bytes);
    corrupt[HUMIDITY_BYTE] = 120;
    writeln!(
        out,
        "received {}: {:?}",
        hex(corrupt),
        gateway_receives(corrupt)
    )
    .expect("writes to a String");

    let cut = &bytes[..bytes.len() - 1];
    writeln!(out, "received {}: {:?}", hex(cut), gateway_receives(cut))
        .expect("writes to a String");

    let mut log = Vec::new();
    gateway_keeps(bytes, &mut log).expect("a whole frame reads back");
    frame.fill(0);
    writeln!(out, "kept: {:?}", log[0]).expect("writes to a String");
    let again = log[0].to_ww_bytes_owned().expect("an owned reading writes");
    writeln!(out, "kept as bytes: {}", hex(&again)).expect("writes to a String");
    out
}
