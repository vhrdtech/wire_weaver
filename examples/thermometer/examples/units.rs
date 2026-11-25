//! The thermometer's numbers without a device: a rate with its unit and bounds, its bytes on the wire, temperatures
//! as points, sample numbers that wrap, and the time of a sample.
//!
//! `cargo run -p thermometer --example thermometer_units`

use std::io::{self, Write};

use thermometer_api::{Rate, Sample, Temperature};
use wire_weaver::shrink_wrap::SerializeShrinkWrap;
use ww_si::origins::{AbsoluteZero, UnixMicros};
use ww_si::prefix::{Centi, One};
use ww_si::units::{Kelvin, Sample as SampleUnit};
use ww_si::{Fixed, IndexFrom1, Micros, Range, Seq, Timed};

fn main() -> io::Result<()> {
    let mut out = io::stdout();
    numbers(&mut out)?;
    points(&mut out)
}

/// A rate: made, refused, and on the wire.
pub fn numbers(out: &mut impl Write) -> io::Result<()> {
    rate(out)?;
    bytes(out)
}

/// Temperatures, sample numbers and the time of a sample.
pub fn points(out: &mut impl Write) -> io::Result<()> {
    temperature(out)?;
    seq(out)?;
    timed(out)
}

fn rate(out: &mut impl Write) -> io::Result<()> {
    // --8<-- [start:rate]
    // a value written in the source: checked when the program is built
    let rate = Rate::bounded(250);
    writeln!(out, "rate: {rate}")?;
    // the number stored, in steps of 1 Hz
    writeln!(out, "stored: {}", rate.get())?;
    // a value that arrives at run time: checked, with an error to handle
    match Rate::try_new(2000) {
        Ok(rate) => writeln!(out, "rate: {rate}")?,
        Err(e) => writeln!(out, "refused: {e}")?,
    }
    // --8<-- [end:rate]
    Ok(())
}

fn bytes(out: &mut impl Write) -> io::Result<()> {
    // --8<-- [start:bytes]
    let mut buf = [0u8; 8];
    let typed = Rate::bounded(250)
        .to_ww_bytes(&mut buf)
        .map_err(io::Error::other)?;
    writeln!(out, "Rate 250 Hz: {}", hex(typed))?;
    let bare = 250u16.to_ww_bytes(&mut buf).map_err(io::Error::other)?;
    writeln!(out, "u16 250:     {}", hex(bare))?;
    // --8<-- [end:bytes]
    Ok(())
}

fn temperature(out: &mut impl Write) -> io::Result<()> {
    // --8<-- [start:temperature]
    // 21.50 °C is 2150 steps of 0.01 K above the Celsius zero
    let morning = Temperature::from_raw(2150);
    let noon = Temperature::from_raw(2375);
    writeln!(out, "morning: {morning}, noon: {noon}")?;
    // two points give an amount: how much warmer, in kelvin
    let warmer: Fixed<Kelvin, Centi, i16> = noon - morning;
    writeln!(out, "warmer by: {warmer}")?;
    // a point and an amount give a point
    writeln!(out, "evening: {}", noon - Fixed::new(300))?;
    // the same point on other scales, if it fits their steps and storage
    if let Some(k) = noon.to::<AbsoluteZero, Centi, u16>() {
        writeln!(out, "noon in K: {k}")?;
    }
    // 296.9 K is no whole number of kelvin
    if noon.to::<AbsoluteZero, One, u16>().is_none() {
        writeln!(out, "noon in whole K: not exact")?;
    }
    // --8<-- [end:temperature]
    Ok(())
}

fn seq(out: &mut impl Write) -> io::Result<()> {
    // --8<-- [start:seq]
    let last: Seq<SampleUnit, u16> = Seq::new(65535);
    let next = last.next();
    writeln!(out, "after {} comes {}", last.get(), next.get())?;
    // compared across the wrap: 0 comes one sample after 65535
    writeln!(
        out,
        "after: {}, by {}",
        next.is_after(last),
        last.distance(next)
    )?;
    // --8<-- [end:seq]
    Ok(())
}

// --8<-- [start:stamp]
/// When `sample` was taken: `start` is when sample 0 was taken, and the device samples at `rate`.
fn stamp(
    start: UnixMicros<u64>,
    rate: Rate,
    sample: Sample,
) -> Timed<Temperature, UnixMicros<u64>> {
    let period_us = 1_000_000 / u64::from(rate.get());
    let since_start: Micros<u64> = Fixed::new(u64::from(sample.seq.get()) * period_us);
    Timed::new(start + since_start, sample.temp)
}
// --8<-- [end:stamp]

fn timed(out: &mut impl Write) -> io::Result<()> {
    // --8<-- [start:timed]
    // 25 Nov 2026 12:00:00 UTC, in µs since 1 Jan 1970
    let start = UnixMicros::from_raw(1_795_608_000_000_000);
    let sample = Sample {
        seq: Seq::new(12),
        input: IndexFrom1::<_, u8, Range<1, 4>>::bounded(1),
        temp: Temperature::from_raw(2150),
    };
    let reading = stamp(start, Rate::bounded(5), sample);
    writeln!(
        out,
        "sample {} at {}: {}",
        sample.seq.get(),
        reading.t,
        reading.value
    )?;
    // --8<-- [end:timed]
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
    hex.join(" ")
}
