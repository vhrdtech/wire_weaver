//! What the book shows of the sensor packet, checked against `docs/generated/sensor_packet/`: every byte comes from a
//! real `to_ww_bytes`, every reading from a real `from_ww_bytes`.

use sensor_packet::{Message, NodeId, ROOF, Reading, hex, radio, v2};
use shrink_wrap::prelude::*;
use std::fmt::Write;

/// The bytes `value` writes on its own.
fn bytes_of<T: SerializeShrinkWrap>(value: &T) -> Vec<u8> {
    let mut buf = [0u8; 64];
    value.to_ww_bytes(&mut buf).expect("fits 64 bytes").to_vec()
}

/// The body of a `bits` fence: `fields` (kind, bit count, label) cut from the bits of `bytes`, then the bytes.
fn bits_fence(bytes: &[u8], fields: &[(&str, usize, &str)]) -> String {
    let bits: String = bytes.iter().map(|b| format!("{b:08b}")).collect();
    assert_eq!(
        fields.iter().map(|f| f.1).sum::<usize>(),
        bits.len(),
        "the fields cover every bit of the bytes"
    );
    let mut lines = Vec::new();
    let mut at = 0;
    for (kind, len, label) in fields {
        // a field of whole bytes on a byte boundary shows a group per byte
        let field = &bits[at..at + len];
        let shown = if at % 8 == 0 && len % 8 == 0 {
            field
                .as_bytes()
                .chunks(8)
                .map(|c| String::from_utf8_lossy(c))
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            field.to_owned()
        };
        lines.push((kind, shown, label));
        at += len;
    }
    let width = lines.iter().map(|l| l.1.len()).max().unwrap_or(0) + 2;
    let mut out = String::new();
    for (kind, shown, label) in lines {
        writeln!(out, "{kind:<6}{shown:<width$}{label}").expect("writes to a String");
    }
    writeln!(out, "bytes {}", hex(bytes)).expect("writes to a String");
    out
}

#[test]
fn book_output_radio() {
    book_outputs::check(
        "sensor_packet/radio.txt",
        &format!(
            "$ cargo run -p sensor_packet --example sensor_packet_radio\n{}",
            radio::transcript()
        ),
    );
}

#[test]
fn book_output_reading_bits() {
    let fence = bits_fence(
        &bytes_of(&ROOF),
        &[
            ("value", 32, "`station`: \"roof\""),
            ("value", 16, "`temperature`: 2150, low byte first"),
            ("head", 1, "`humidity` is `Some`"),
            ("pad", 7, "padding"),
            ("value", 8, "`humidity`: 48"),
            ("value", 1, "`battery_low`: false"),
            ("pad", 3, "padding"),
            ("head", 4, "length of `station`: 4"),
        ],
    );
    book_outputs::check("sensor_packet/reading_bits.txt", &fence);
}

/// A table row: the type, the value as written in Rust, and its bytes.
macro_rules! row {
    ($out:ident, $ty:literal, $value:expr) => {
        row!($out, $ty, stringify!($value), $value)
    };
    ($out:ident, $ty:literal, $shown:expr, $value:expr) => {
        let bytes = bytes_of(&$value);
        let hex = if bytes.is_empty() {
            "no bytes".to_owned()
        } else {
            format!("`{}`", hex(&bytes))
        };
        writeln!($out, "| `{}` | `{}` | {} |", $ty, $shown, hex).expect("writes to a String");
    };
}

#[test]
fn book_output_built_in_types() {
    let mut out = String::from("| Type | Value | Bytes |\n|---|---|---|\n");
    row!(out, "bool", true);
    row!(out, "U3", "5", U3::new(5).expect("5 fits 3 bits"));
    row!(
        out,
        "(U3, U7)",
        "(5, 100)",
        (U3::new(5).expect("fits"), U7::new(100).expect("fits"))
    );
    row!(out, "(bool, u8)", "(true, 0xaa)", (true, 0xaa_u8));
    row!(out, "u8", "0xaa", 0xaa_u8);
    row!(out, "u16", "0x1234", 0x1234_u16);
    row!(out, "i16", "-2", -2_i16);
    row!(out, "f32", "1.5", 1.5_f32);
    row!(out, "UNib32", "5", UNib32(5));
    row!(out, "UNib32", "300", UNib32(300));
    row!(out, "&str", "abc");
    row!(
        out,
        "RefVec<'i, u8>",
        "[1, 2, 3]",
        RefVec::new_bytes(&[1, 2, 3])
    );
    row!(out, "[u8; 3]", "[1, 2, 3]", [1_u8, 2, 3]);
    row!(
        out,
        "MaxLen<&str, 8>",
        "\"abc\"",
        MaxLen::<&str, 8>::new("abc").expect("3 is at most 8")
    );
    row!(out, "Option<u8>", "Some(5)", Some(5_u8));
    row!(out, "Option<u8>", "None", None::<u8>);
    row!(out, "Result<u8, u8>", "Ok(1)", Ok::<u8, u8>(1));
    row!(out, "Result<u8, u8>", "Err(2)", Err::<u8, u8>(2));
    row!(out, "()", ());
    book_outputs::check("sensor_packet/built_in_types.md", &out);
}

#[test]
fn book_output_messages() {
    let mut out = String::from("| Value | Bytes |\n|---|---|\n");
    let mut row = |value: &str, bytes: Vec<u8>| {
        writeln!(out, "| `{value}` | `{}` |", hex(&bytes)).expect("writes to a String");
    };
    row("NodeId(7)", bytes_of(&NodeId(7)));
    row(
        "Message::Hello { node: NodeId(7), firmware: \"1.0\" }",
        bytes_of(&Message::Hello {
            node: NodeId(7),
            firmware: "1.0",
        }),
    );
    row("Message::Reading(ROOF)", bytes_of(&Message::Reading(ROOF)));
    row("Message::Goodbye", bytes_of(&Message::Goodbye));
    book_outputs::check("sensor_packet/messages.md", &out);
}

#[test]
fn book_output_versions() {
    let first = bytes_of(&ROOF);
    let next = bytes_of(&v2::ROOF);
    let rain = bytes_of(&v2::Message::Rain);
    let mut out = String::new();
    writeln!(out, "first firmware sends: {}", hex(&first)).expect("writes");
    writeln!(out, "next firmware sends:  {}", hex(&next)).expect("writes");
    writeln!(out, "next gateway reads the first firmware's bytes:").expect("writes");
    writeln!(out, "  {:?}", v2::Reading::from_ww_bytes(&first)).expect("writes");
    writeln!(out, "first gateway reads the next firmware's bytes:").expect("writes");
    writeln!(out, "  {:?}", Reading::from_ww_bytes(&next)).expect("writes");
    writeln!(out, "next firmware sends Message::Rain: {}", hex(&rain)).expect("writes");
    writeln!(out, "first gateway reads it:").expect("writes");
    writeln!(out, "  {:?}", Message::from_ww_bytes(&rain)).expect("writes");
    book_outputs::check("sensor_packet/versions.txt", &out);
}
