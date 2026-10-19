//! Which bits of a whole request or event hold what (TOOL-9): the `ww_client_server` envelope (sequence number, path,
//! kind) read field by field with the derived types, the payload inside it (method arguments, a property value, a
//! return value, a stream item) explained by its type from the API, as [ww_self::explain] does for a value alone.
//!
//! The envelope's types are not in the API model (`TailBytes`, `UVlq32Backfill`), so its fields are read here the
//! way the derived code reads them (`BufReader::read`, one field after the other), and their places taken from the
//! reader between the reads. A payload is a `TailBytes` that borrows from the buffer: its place is where it points.

use crate::DynResource;
use crate::client::dynamic::DynResourceKind;
use anyhow::{Result, anyhow};
use std::sync::Arc;
use wire_weaver::prelude::UNib32;
use wire_weaver::shrink_wrap::buf_reader::Cursors;
use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
use wire_weaver::shrink_wrap::{BufReader, SerializeShrinkWrapOwned, UVlq32, UVlq32Backfill};
use ww_client_server::{
    EventKind, PathKind, PathKindOwned, Request, RequestKind, RequestKindOwned, RequestOwned,
};
use ww_numeric::NumericValue;
use ww_self::explain::{Explained, Part, PartKind, explain, explain_fields};
use ww_self::{ApiBundleOwned, TypeOwned, ValueOwned};

/// A request or event explained, with a title saying what it is.
pub struct Envelope {
    pub title: String,
    pub explained: Explained,
}

/// Explain `bytes` as a request (as the host sends it), its payload by the resource its path names in `root`'s API.
pub fn explain_request(bytes: &[u8], root: &DynResource) -> Envelope {
    let mut b = Builder::new(bytes, root);
    let mut title = "Request".to_string();
    let result = (|| {
        let mut rd = BufReader::new(bytes);
        let at = b.at(&rd);
        let seq: UVlq32Backfill = rd.read()?;
        b.leaf(1, "seq", "uvlq32", at, b.at(&rd), num(seq.0), None);

        let at = b.at(&rd);
        let path_kind: PathKind = rd.read()?;
        let resource = match &path_kind {
            PathKind::Absolute { path } => {
                let chain: Vec<u32> = path
                    .iter()
                    .map(|i| i.map(|i| i.0))
                    .collect::<Result<_, _>>()?;
                b.leaf(
                    1,
                    "path",
                    "PathKind::Absolute",
                    at,
                    b.at(&rd),
                    ids(&chain),
                    None,
                );
                let resource = root.descend(&chain);
                if let Some(part) = b.parts.last_mut() {
                    part.note = Some(match &resource {
                        Ok(r) => r.path().to_string(),
                        Err(e) => format!("{e}"),
                    });
                }
                resource.ok()
            }
            other => {
                // a path through a global trait id: shown, its payload not explained yet
                b.leaf(
                    1,
                    "path",
                    "PathKind",
                    at,
                    b.at(&rd),
                    None,
                    Some(format!("{other:?}")),
                );
                None
            }
        };

        let at = b.at(&rd);
        let kind: RequestKind = rd.read()?;
        let end = b.at(&rd);
        let variant = request_variant(&kind);
        title = match &resource {
            Some(r) => format!("Request {variant} {}", r.path()),
            None => format!("Request {variant}"),
        };
        b.node(1, "kind", "RequestKind", at, end, Some(variant.to_string()));
        b.discriminant(2, at);
        let payload = match &kind {
            RequestKind::Call { args } => Some(("args", args.as_slice(), Payload::Args)),
            RequestKind::Write { data } => Some(("data", data.as_slice(), Payload::Value)),
            RequestKind::StreamOpen { args, .. } => Some(("args", args.as_slice(), Payload::Args)),
            _ => None,
        };
        if let Some((label, slice, what)) = payload {
            b.payload(
                2,
                label,
                slice,
                resource.as_ref().map(|r| what.of(r, false)),
            )?;
        }
        Ok(())
    })();
    b.finish(title, result)
}

/// Explain `bytes` as an event (as the device sends it). A stream's data names its stream; a method's return value
/// or a property read doesn't (only `seq` ties it to its request): `for_resource` gives it.
pub fn explain_event(
    bytes: &[u8],
    root: &DynResource,
    for_resource: Option<&DynResource>,
) -> Envelope {
    let mut b = Builder::new(bytes, root);
    let mut title = "Event".to_string();
    let result = (|| {
        let mut rd = BufReader::new(bytes);
        let at = b.at(&rd);
        let seq: UVlq32 = rd.read()?;
        b.leaf(1, "seq", "uvlq32", at, b.at(&rd), num(seq.0), None);

        let at = b.at(&rd);
        let event: Result<EventKind, ww_client_server::Error> = rd.read()?;
        let end = b.at(&rd);
        let flag_end = Cursors {
            front_bit: at.front_bit + 1,
            ..at
        };
        let kind = match event {
            Err(e) => {
                title = "Event error".to_string();
                b.node(
                    1,
                    "result",
                    "Result<EventKind, Error>",
                    at,
                    end,
                    Some("Err".into()),
                );
                b.leaf(
                    2,
                    "flag",
                    "bool",
                    at,
                    flag_end,
                    Some(ValueOwned::Bool(false)),
                    None,
                );
                b.leaf(
                    2,
                    "err",
                    "Error",
                    flag_end,
                    end,
                    None,
                    Some(format!("{e:?}")),
                );
                return Ok(());
            }
            Ok(kind) => kind,
        };
        b.node(
            1,
            "result",
            "Result<EventKind, Error>",
            at,
            end,
            Some("Ok".into()),
        );
        b.leaf(
            2,
            "flag",
            "bool",
            at,
            flag_end,
            Some(ValueOwned::Bool(true)),
            None,
        );
        let variant = event_variant(&kind);
        b.node(
            2,
            "kind",
            "EventKind",
            flag_end,
            end,
            Some(variant.to_string()),
        );
        b.discriminant(3, flag_end);
        let path = match &kind {
            EventKind::StreamData { path, .. }
            | EventKind::StreamSideband { path, .. }
            | EventKind::StreamItem { path, .. }
            | EventKind::PropertyUpdate { path, .. } => {
                let chain: Vec<u32> = path
                    .iter()
                    .map(|i| i.map(|i| i.0))
                    .collect::<Result<_, _>>()?;
                Some(root.descend(&chain).map_err(|e| anyhow!("{e}")))
            }
            _ => None,
        };
        let resource = match path {
            Some(Ok(r)) => Some(r),
            Some(Err(e)) => {
                title = format!("Event {variant} ({e})");
                None
            }
            None => for_resource.cloned(),
        };
        if let Some(r) = &resource {
            title = format!("Event {variant} {}", r.path());
        } else if title == "Event" {
            title = format!("Event {variant}");
        }
        let payload = match &kind {
            EventKind::Value { data } => Some(data.as_slice()),
            EventKind::StreamData { data, .. } | EventKind::StreamItem { data, .. } => {
                Some(data.as_slice())
            }
            // a level's update is a sparse struct made for the observe request, not a type of the API
            EventKind::PropertyUpdate { data, .. } => {
                b.payload(3, "data", data.as_slice(), None)?;
                None
            }
            _ => None,
        };
        if let Some(slice) = payload {
            b.payload(
                3,
                "data",
                slice,
                resource.as_ref().map(|r| Payload::Value.of(r, true)),
            )?;
        }
        Ok(())
    })();
    b.finish(title, result)
}

/// The request a call, write or read of `resource` is on the wire (sequence number 1): `payload` the arguments
/// (method), the value (property write), `None` a read.
pub fn request_bytes(resource: &DynResource, payload: Option<Vec<u8>>) -> Result<Vec<u8>> {
    let path = PathKindOwned::Absolute {
        path: resource.index_chain().to_vec(),
    };
    let kind = match (resource.kind().map_err(|e| anyhow!("{e}"))?, payload) {
        (DynResourceKind::Method { .. }, Some(args)) => RequestKindOwned::Call {
            args: TailBytesOwned(args),
        },
        (DynResourceKind::Property { .. }, Some(data)) => RequestKindOwned::Write {
            data: TailBytesOwned(data),
        },
        (DynResourceKind::Property { .. }, None) => RequestKindOwned::Read,
        _ => anyhow::bail!(
            "'{}': only method calls and property reads and writes",
            resource.path()
        ),
    };
    let mut bytes = RequestOwned {
        seq: UVlq32Backfill(0),
        path_kind: path,
        kind,
    }
    .to_ww_bytes_owned()?;
    // as the client sends it: `seq` filled in, in its shortest form
    let sent = Request::set_seq(&mut bytes, 1)?.to_vec();
    Ok(sent)
}

/// What a payload is, of the resource it belongs to.
#[derive(Copy, Clone)]
enum Payload {
    /// A method's or stream function's arguments.
    Args,
    /// A property's value, a stream's item, a method's return value (`ret`).
    Value,
}

/// How a payload's bytes are read.
enum Target {
    Value(TypeOwned),
    Args(Vec<(String, TypeOwned)>),
}

impl Payload {
    fn of(self, resource: &DynResource, ret: bool) -> Option<Target> {
        Some(match (self, resource.kind().ok()?) {
            (Payload::Args, DynResourceKind::Method { args, .. }) => Target::Args(
                args.iter()
                    .map(|a| (a.ident.clone(), a.ty.clone()))
                    .collect(),
            ),
            (Payload::Value, DynResourceKind::Method { return_ty, .. }) if ret => {
                Target::Value(return_ty?.clone())
            }
            (Payload::Value, DynResourceKind::Property { ty, .. })
            | (Payload::Value, DynResourceKind::Stream { ty, .. }) => Target::Value(ty.clone()),
            _ => return None,
        })
    }
}

/// The parts of an envelope, built as its fields are read.
struct Builder<'b> {
    base: &'b [u8],
    bundle: Arc<ApiBundleOwned>,
    parts: Vec<Part>,
    /// Result of the payload's decode, when there is one.
    payload: Option<Result<Vec<ValueOwned>>>,
}

impl<'b> Builder<'b> {
    fn new(base: &'b [u8], root: &DynResource) -> Self {
        let whole = Part {
            depth: 0,
            label: String::new(),
            kind: PartKind::Value,
            ty: String::new(),
            front: 0..base.len() * 8,
            back: 0..0,
            value: None,
            note: None,
        };
        Builder {
            base,
            bundle: root.bundle().clone(),
            parts: vec![whole],
            payload: None,
        }
    }

    fn at(&self, rd: &BufReader) -> Cursors {
        rd.cursors_in(self.base)
            .expect("readers of the envelope are made from its bytes")
    }

    #[allow(clippy::too_many_arguments)]
    fn leaf(
        &mut self,
        depth: usize,
        label: &str,
        ty: &str,
        from: Cursors,
        to: Cursors,
        value: Option<ValueOwned>,
        note: Option<String>,
    ) {
        self.parts.push(Part {
            depth,
            label: label.into(),
            kind: PartKind::Value,
            ty: ty.into(),
            front: from.front_bit..to.front_bit,
            back: to.back_nibble.min(from.back_nibble)..from.back_nibble,
            value,
            note,
        });
    }

    fn node(
        &mut self,
        depth: usize,
        label: &str,
        ty: &str,
        from: Cursors,
        to: Cursors,
        note: Option<String>,
    ) {
        self.parts.push(Part {
            depth,
            label: label.into(),
            kind: PartKind::Value,
            ty: ty.into(),
            front: from.front_bit..to.front_bit,
            back: to.back_nibble.min(from.back_nibble)..from.back_nibble,
            value: None,
            note,
        });
    }

    /// The nibble discriminant of a `ww_repr = nib` enum read from `at`, padding before it included.
    fn discriminant(&mut self, depth: usize, at: Cursors) {
        let start = at.front_bit.next_multiple_of(4);
        if start + 4 > self.base.len() * 8 {
            return;
        }
        if start > at.front_bit {
            self.parts.push(Part {
                depth,
                label: "padding".into(),
                kind: PartKind::Padding,
                ty: String::new(),
                front: at.front_bit..start,
                back: 0..0,
                value: None,
                note: None,
            });
        }
        let byte = self.base[start / 8];
        let nib = if start.is_multiple_of(8) {
            byte >> 4
        } else {
            byte & 0xf
        };
        self.parts.push(Part {
            depth,
            label: "discriminant".into(),
            kind: PartKind::Discriminant,
            ty: "nib".into(),
            front: start..start + 4,
            back: 0..0,
            value: Some(ValueOwned::Numeric(NumericValue::U32(nib as u32))),
            note: None,
        });
    }

    /// A payload at `slice` (borrowed from the envelope's bytes), explained by `target` when known.
    fn payload(
        &mut self,
        depth: usize,
        label: &str,
        slice: &[u8],
        target: Option<Option<Target>>,
    ) -> Result<()> {
        let offset = (slice.as_ptr() as usize)
            .checked_sub(self.base.as_ptr() as usize)
            .filter(|o| o + slice.len() <= self.base.len())
            .ok_or_else(|| anyhow!("payload is not inside the envelope"))?;
        let front = offset * 8..(offset + slice.len()) * 8;
        let Some(Some(target)) = target else {
            self.parts.push(Part {
                depth,
                label: label.into(),
                kind: PartKind::Value,
                ty: "TailBytes".into(),
                front,
                back: 0..0,
                value: Some(ValueOwned::Vec(
                    slice
                        .iter()
                        .map(|b| ValueOwned::Numeric(NumericValue::U8(*b)))
                        .collect(),
                )),
                note: None,
            });
            return Ok(());
        };
        self.parts.push(Part {
            depth,
            label: label.into(),
            kind: PartKind::Value,
            ty: "TailBytes".into(),
            front,
            back: 0..0,
            value: None,
            note: None,
        });
        let explained = match &target {
            Target::Value(ty) => explain(slice, ty, &self.bundle),
            Target::Args(defs) => explain_fields(
                slice,
                defs.iter().map(|(n, t)| (n.as_str(), t)),
                &self.bundle,
            ),
        };
        for mut part in explained.parts {
            part.depth += depth + 1;
            part.front = part.front.start + offset * 8..part.front.end + offset * 8;
            part.back = part.back.start + offset * 2..part.back.end + offset * 2;
            self.parts.push(part);
        }
        self.payload = Some(explained.result);
        Ok(())
    }

    fn finish(mut self, title: String, result: Result<()>) -> Envelope {
        self.parts[0].ty = title.split(' ').next().unwrap_or_default().to_string();
        let result = match (result, self.payload) {
            (Err(e), _) => Err(e),
            (Ok(()), Some(payload)) => payload,
            (Ok(()), None) => Ok(vec![]),
        };
        Envelope {
            title,
            explained: Explained {
                parts: self.parts,
                result,
                len: self.base.len(),
            },
        }
    }
}

fn num(n: u32) -> Option<ValueOwned> {
    Some(ValueOwned::Numeric(NumericValue::U32(n)))
}

fn ids(chain: &[u32]) -> Option<ValueOwned> {
    Some(ValueOwned::Vec(
        chain
            .iter()
            .map(|i| ValueOwned::Numeric(NumericValue::UNib32(UNib32(*i))))
            .collect(),
    ))
}

pub(crate) fn request_variant(kind: &RequestKind) -> &'static str {
    match kind {
        RequestKind::Call { .. } => "Call",
        RequestKind::MultiCall { .. } => "MultiCall",
        RequestKind::Read => "Read",
        RequestKind::MultiRead { .. } => "MultiRead",
        RequestKind::Write { .. } => "Write",
        RequestKind::MultiWrite { .. } => "MultiWrite",
        RequestKind::StreamSideband { .. } => "StreamSideband",
        RequestKind::Introspect => "Introspect",
        RequestKind::StreamOpen { .. } => "StreamOpen",
        RequestKind::Observe { .. } => "Observe",
        RequestKind::SinkWrite { .. } => "SinkWrite",
    }
}

pub(crate) fn event_variant(kind: &EventKind) -> &'static str {
    match kind {
        EventKind::Value { .. } => "Value",
        EventKind::Written => "Written",
        EventKind::StreamData { .. } => "StreamData",
        EventKind::StreamSideband { .. } => "StreamSideband",
        EventKind::StreamItem { .. } => "StreamItem",
        EventKind::PropertyUpdate { .. } => "PropertyUpdate",
    }
}
