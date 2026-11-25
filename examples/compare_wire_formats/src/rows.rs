//! One row per format for a message: the bytes and how the format lets a field be added later.

use serde::Serialize;
use wincode::config::Configuration;

/// Largest example message in Postcard or `shrink_wrap` bytes, with room to spare.
const SCRATCH_BYTES_MAX: usize = 8192;

/// How a format lets old and new ends read each other's messages, as this example encodes it.
#[derive(Clone, Copy)]
pub enum Evolution {
    /// Field numbers or layout rules of a schema: fields can be added or dropped
    SchemaRules,
    /// Field names travel with the data: a reader skips or defaults what it doesn't know
    SelfDescribing,
    /// An extension marker reserves room for fields added later
    ExtensionMarkers,
    /// Positional and nothing else: any schema change breaks old readers
    None,
}

impl Evolution {
    pub fn as_str(self) -> &'static str {
        match self {
            Evolution::SchemaRules => "schema_rules",
            Evolution::SelfDescribing => "self_describing",
            Evolution::ExtensionMarkers => "extension_markers",
            Evolution::None => "none",
        }
    }
}

pub struct Row {
    pub format: &'static str,
    pub bytes: Vec<u8>,
    pub evolution: Evolution,
}

/// A message and its encodings, in the order every table shows them.
pub struct Message {
    /// File stem of the generated table
    pub name: &'static str,
    /// One line for the page
    pub title: &'static str,
    pub rows: Vec<Row>,
}

fn row(format: &'static str, bytes: Vec<u8>, evolution: Evolution) -> Row {
    Row {
        format,
        bytes,
        evolution,
    }
}

/// The encodings that read the message's plain `serde::Serialize` type, each at the setting a project would pick
/// to be small: MessagePack with and without names, CBOR with names, wincode fixed-width and varint.
pub fn serde_rows<T: Serialize>(value: &T) -> Vec<Row> {
    let postcard = postcard_bytes(value);
    let msgpack_array = rmp_serde::to_vec(value).expect("MessagePack encodes the example");
    let msgpack_named = rmp_serde::to_vec_named(value).expect("MessagePack encodes the example");
    let json = serde_json::to_vec(value).expect("JSON encodes the example");
    let mut cbor = Vec::new();
    ciborium::into_writer(value, &mut cbor).expect("CBOR encodes the example");
    let wincode_fixed = <serde_wincode::SerdeCompat<T> as wincode::Serialize>::serialize(value)
        .expect("wincode encodes the example");
    let wincode_varint =
        <serde_wincode::SerdeCompat<T> as wincode::config::Serialize<_>>::serialize(
            value,
            Configuration::default().with_varint_encoding(),
        )
        .expect("wincode encodes the example");
    vec![
        row("Postcard", postcard, Evolution::None),
        row("MessagePack (array)", msgpack_array, Evolution::None),
        row(
            "MessagePack (names)",
            msgpack_named,
            Evolution::SelfDescribing,
        ),
        row("JSON", json, Evolution::SelfDescribing),
        row("CBOR (names)", cbor, Evolution::SelfDescribing),
        row("wincode (fixed)", wincode_fixed, Evolution::None),
        row("wincode (varint)", wincode_varint, Evolution::None),
    ]
}

/// The bytes of every encoding of one message, in table order.
pub struct Encodings<'a, T: Serialize> {
    /// The message as a plain Rust type: `shrink_wrap` without layout directives and every `serde` format
    pub plain: &'a T,
    /// `shrink_wrap` bytes of the same message with layout directives
    pub ww_tuned: Vec<u8>,
    pub ww_plain: Vec<u8>,
    /// Postcard bytes of the message with `fixint` on the fields that use all of their bits; `None` where no field
    /// gains from it
    pub postcard_tuned: Option<Vec<u8>>,
    pub protobuf: Vec<u8>,
    pub flatbuffers: Vec<u8>,
    pub asn1_closed: Vec<u8>,
    pub asn1_extensible: Vec<u8>,
}

impl<T: Serialize> Encodings<'_, T> {
    pub fn into_rows(self) -> Vec<Row> {
        let mut rows = vec![
            row("WireWeaver (plain)", self.ww_plain, Evolution::SchemaRules),
            row("WireWeaver (tuned)", self.ww_tuned, Evolution::SchemaRules),
        ];
        let mut serde = serde_rows(self.plain);
        let tuned = self
            .postcard_tuned
            .unwrap_or_else(|| serde[0].bytes.clone());
        serde.insert(1, row("Postcard (tuned)", tuned, Evolution::None));
        rows.extend(serde);
        rows.push(row("Protobuf", self.protobuf, Evolution::SchemaRules));
        rows.push(row("FlatBuffers", self.flatbuffers, Evolution::SchemaRules));
        rows.push(row(
            "ASN.1 UPER (closed)",
            self.asn1_closed,
            Evolution::None,
        ));
        rows.push(row(
            "ASN.1 UPER (extensible)",
            self.asn1_extensible,
            Evolution::ExtensionMarkers,
        ));
        rows
    }
}

/// `shrink_wrap` bytes of a type that owns no heap data.
pub fn ww_bytes<T: wire_weaver::shrink_wrap::SerializeShrinkWrap>(value: &T) -> Vec<u8> {
    let mut scratch = vec![0u8; SCRATCH_BYTES_MAX];
    value
        .to_ww_bytes(&mut scratch)
        .expect("shrink_wrap encodes the example")
        .to_vec()
}

/// `shrink_wrap` bytes of a type with a `Vec` or a `String`.
pub fn ww_bytes_owned<T: wire_weaver::shrink_wrap::SerializeShrinkWrapOwned>(value: &T) -> Vec<u8> {
    value
        .to_ww_bytes_owned()
        .expect("shrink_wrap encodes the example")
}

/// Postcard bytes of a value.
pub fn postcard_bytes<T: Serialize>(value: &T) -> Vec<u8> {
    let mut scratch = vec![0u8; SCRATCH_BYTES_MAX];
    postcard::to_slice(value, &mut scratch)
        .expect("scratch buffer holds every example message")
        .to_vec()
}
