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
use std::ops::Range;
use std::sync::Arc;
use wire_weaver::prelude::UNib32;
use wire_weaver::shrink_wrap::buf_reader::Cursors;
use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
use wire_weaver::shrink_wrap::{BufReader, DeserializeShrinkWrap, SerializeShrinkWrapOwned, UVlq32, UVlq32Backfill};
use ww_client_server::{
    EventKind, PathKind, PathKindOwned, Request, RequestKind, RequestKindOwned, RequestOwned,
};
use ww_client_server::observe::{ObserveEntry, ObserveGranted, ObserveMode};
use ww_numeric::NumericValue;
use ww_self::explain::{Explained, Part, PartKind, explain, explain_fields};
use ww_self::{ApiBundleOwned, ApiLevelLocationOwned, TypeOwned, ValueOwned};
use ww_version::FullVersionOwned;
use std::collections::HashMap;
use std::sync::Mutex;

/// A request or event explained, with a title saying what it is.
pub struct Envelope {
    pub title: String,
    pub explained: Explained,
    /// The type of each value in `explained.result` when it is known (a property's, the arguments'), for showing
    /// them with their units; empty otherwise.
    pub types: Vec<TypeOwned>,
    /// The resource the request is for (or the event is about), when the API knows it.
    pub resource: Option<DynResource>,
}

/// Explain `bytes` as a request (as the host sends it), its payload by the resource its path names in `root`'s API,
/// or in the snapshot of a global trait ([global_trait]). Without a `root` (the API of the device is not known) only
/// the requests through global traits are explained.
pub fn explain_request<'r>(bytes: &[u8], root: impl Into<Option<&'r DynResource>>) -> Envelope {
    let root = root.into();
    let mut b = Builder::new(bytes);
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
                let resource = root
                    .ok_or_else(|| anyhow!("the device API is not known"))
                    .and_then(|root| root.descend(&chain).map_err(|e| anyhow!("{e}")));
                if let Some(part) = b.parts.last_mut() {
                    part.note = Some(match &resource {
                        Ok(r) => r.path().to_string(),
                        Err(e) => format!("{e}"),
                    });
                }
                resource.ok()
            }
            PathKind::GlobalFull {
                gid,
                trait_name,
                path_from_trait,
            } => {
                // a trait of a crate that is known from a snapshot, wherever the device has it
                let chain: Vec<u32> = path_from_trait
                    .iter()
                    .map(|i| i.map(|i| i.0))
                    .collect::<Result<_, _>>()?;
                let resource = global_trait(&gid.make_owned(), trait_name)
                    .ok_or_else(|| {
                        anyhow!("{}@{} is not known here", gid.crate_id, trait_name)
                    })
                    .and_then(|t| t.descend(&chain).map_err(|e| anyhow!("{e}")));
                b.leaf(
                    1,
                    "path",
                    "PathKind::GlobalFull",
                    at,
                    b.at(&rd),
                    ids(&chain),
                    Some(match &resource {
                        Ok(r) => r.path().to_string(),
                        Err(e) => format!("{}::{trait_name}: {e}", gid.crate_id),
                    }),
                );
                resource.ok()
            }
            other => {
                // a path through a compact global id: shown, its payload not explained
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

        b.set_resource(resource.clone());

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
        if let (RequestKind::Observe { cfg }, Some(level)) = (&kind, &resource) {
            let names = level_names(level);
            let entries = cfg.entries.iter().collect::<Result<Vec<_>, _>>()?;
            let mut text = observe_text(&entries, &names);
            if !cfg.patch {
                text += if text.is_empty() {
                    "stop observing"
                } else {
                    "; others off"
                };
            }
            if let Some(budget) = cfg.budget {
                text += &format!("; budget {} B/s", budget.0);
            }
            b.text(2, "cfg", "ObserveRequest", at.front_bit..end.front_bit, text);
        }
        Ok(())
    })();
    b.finish(title, result)
}

/// Explain `bytes` as an event (as the device sends it). A stream's data names its stream; a method's return value
/// or a property read doesn't (only `seq` ties it to its request): `for_resource` gives it.
pub fn explain_event<'r>(
    bytes: &[u8],
    root: impl Into<Option<&'r DynResource>>,
    for_resource: Option<&DynResource>,
) -> Envelope {
    explain_reply(bytes, root.into(), for_resource, false)
}

/// [explain_event] knowing what the event answers: with `observe` the value is what an `Observe` request of the
/// trait level `for_resource` was granted (`ObserveGranted`), not a property.
pub fn explain_reply<'r>(
    bytes: &[u8],
    root: impl Into<Option<&'r DynResource>>,
    for_resource: Option<&DynResource>,
    observe: bool,
) -> Envelope {
    let root = root.into();
    let mut b = Builder::new(bytes);
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
                title = match for_resource {
                    Some(r) => format!("Event error {}", r.path()),
                    None => "Event error".to_string(),
                };
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
                root.map(|root| root.descend(&chain).map_err(|e| anyhow!("{e}")))
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
        b.set_resource(resource.clone());
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
            // a level's update is a sparse struct made for the observe request, not a type of the API: its slots
            // are the level's `+observe` properties
            EventKind::PropertyUpdate { data, .. } => {
                b.update(3, data.as_slice(), resource.as_ref())?;
                None
            }
            _ => None,
        };
        if let (Some(slice), true, Some(level)) = (payload, observe, &resource) {
            let granted = ObserveGranted::from_ww_bytes(slice)?;
            let names = level_names(level);
            let entries = granted.entries.iter().collect::<Result<Vec<_>, _>>()?;
            let mut text = observe_text(&entries, &names);
            for r in granted.refused.iter() {
                let r = r?;
                text += &format!("; {} refused: {:?}", name_of(&names, r.index.0), r.reason);
            }
            text += &format!("; budget {} B/s", granted.budget.0);
            let text = text.trim_start_matches("; ").to_string();
            let front = b.range_of(slice)?;
            b.text(3, "granted", "ObserveGranted", front, text);
        } else if let Some(slice) = payload {
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
    /// The API the payload is read by: that of the resource, once known.
    bundle: Option<Arc<ApiBundleOwned>>,
    parts: Vec<Part>,
    /// Result of the payload's decode, when there is one.
    payload: Option<Result<Vec<ValueOwned>>>,
    /// Types of the payload's values when known.
    types: Vec<TypeOwned>,
    resource: Option<DynResource>,
}

impl<'b> Builder<'b> {
    fn new(base: &'b [u8]) -> Self {
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
            bundle: None,
            parts: vec![whole],
            payload: None,
            types: vec![],
            resource: None,
        }
    }

    fn set_resource(&mut self, resource: Option<DynResource>) {
        self.bundle = resource.as_ref().map(|r| r.bundle().clone());
        self.resource = resource;
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
        let bundle = self
            .bundle
            .clone()
            .ok_or_else(|| anyhow!("no API to explain the payload by"))?;
        let explained = match &target {
            Target::Value(ty) => explain(slice, ty, &bundle),
            Target::Args(defs) => explain_fields(
                slice,
                defs.iter().map(|(n, t)| (n.as_str(), t)),
                &bundle,
            ),
        };
        for mut part in explained.parts {
            part.depth += depth + 1;
            part.front = part.front.start + offset * 8..part.front.end + offset * 8;
            part.back = part.back.start + offset * 2..part.back.end + offset * 2;
            self.parts.push(part);
        }
        self.payload = Some(explained.result);
        self.types = match target {
            Target::Value(ty) => vec![ty],
            Target::Args(defs) => defs.into_iter().map(|(_, ty)| ty).collect(),
        };
        Ok(())
    }

    /// Bits of the envelope that `slice`, borrowed from it, covers.
    fn range_of(&self, slice: &[u8]) -> Result<Range<usize>> {
        let offset = (slice.as_ptr() as usize)
            .checked_sub(self.base.as_ptr() as usize)
            .filter(|o| o + slice.len() <= self.base.len())
            .ok_or_else(|| anyhow!("payload is not inside the envelope"))?;
        Ok(offset * 8..(offset + slice.len()) * 8)
    }

    /// Text where there are no values to show, e.g. what an `Observe` asked for.
    fn text(&mut self, depth: usize, label: &str, ty: &str, front: Range<usize>, text: String) {
        self.parts.push(Part {
            depth,
            label: label.into(),
            kind: PartKind::Value,
            ty: ty.into(),
            front,
            back: 0..0,
            value: None,
            note: Some(text),
        });
    }

    /// A level's `PropertyUpdate` data at `slice`: the properties it carries, each with its place in it, by the
    /// level's `+observe` properties.
    fn update(
        &mut self,
        depth: usize,
        slice: &[u8],
        level: Option<&DynResource>,
    ) -> Result<()> {
        let Some(level) = level else {
            return self.payload(depth, "data", slice, None);
        };
        let offset = (slice.as_ptr() as usize)
            .checked_sub(self.base.as_ptr() as usize)
            .filter(|o| o + slice.len() <= self.base.len())
            .ok_or_else(|| anyhow!("payload is not inside the envelope"))?;
        self.parts.push(Part {
            depth,
            label: "data".into(),
            kind: PartKind::Value,
            ty: "TailBytes".into(),
            front: offset * 8..(offset + slice.len()) * 8,
            back: 0..0,
            value: None,
            note: None,
        });
        let slots = level
            .observe_schema()
            .map_err(|e| anyhow!("{e}"))?
            .view(slice);
        let slots = match slots {
            Ok(slots) => slots,
            Err(e) => {
                self.payload = Some(Err(e));
                return Ok(());
            }
        };
        let mut values = vec![];
        for s in slots {
            self.parts.push(Part {
                depth: depth + 1,
                label: s.name,
                kind: PartKind::Value,
                ty: s
                    .ty
                    .human_name(false, level.bundle())
                    .unwrap_or_else(|_| format!("{:?}", s.ty)),
                front: s.front.start + offset * 8..s.front.end + offset * 8,
                back: s.back.start + offset * 2..s.back.end + offset * 2,
                value: None,
                note: None,
            });
            values.push(s.value);
            self.types.push(s.ty);
        }
        self.payload = Some(Ok(values));
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
            types: self.types,
            resource: self.resource,
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
        RequestKind::SinkWriteUnits { .. } => "SinkWriteUnits",
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

/// Property names of a trait level by resource index, as far as they can be observed.
fn level_names(level: &DynResource) -> Vec<(u32, String)> {
    level
        .observe_schema()
        .map(|s| s.props().iter().map(|(i, n, _)| (*i, n.clone())).collect())
        .unwrap_or_default()
}

fn name_of(names: &[(u32, String)], index: u32) -> String {
    match names.iter().find(|(i, _)| *i == index) {
        Some((_, name)) => name.clone(),
        None => format!("[{index}]"),
    }
}

/// `reading on change, temperature every 10 ms`: how each property of an `Observe` is observed.
fn observe_text(entries: &[ObserveEntry], names: &[(u32, String)]) -> String {
    entries
        .iter()
        .map(|e| format!("{}: {}", name_of(names, e.index.0), mode_text(&e.mode)))
        .collect::<Vec<_>>()
        .join("; ")
}

fn mode_text(mode: &ObserveMode) -> String {
    match mode {
        ObserveMode::Off => "off".into(),
        ObserveMode::OnChange { min_us } if min_us.0 == 0 => "on change".into(),
        ObserveMode::OnChange { min_us } => format!("on change (min {})", us(min_us.0)),
        ObserveMode::Periodic { every_us } => format!("every {}", us(every_us.0)),
        ObserveMode::OnChangeOrBeat { min_us, max_us } => format!("on change (min {}), beat every {}", us(min_us.0), us(max_us.0)),
        ObserveMode::Samples {
            every_us,
            max_age_us,
        } if every_us.0 == 0 => format!("every sample (batches up to {})", us(max_age_us.0)),
        ObserveMode::Samples {
            every_us,
            max_age_us,
        } => format!(
            "a sample every {} (batches up to {})",
            us(every_us.0),
            us(max_age_us.0)
        ),
        other => format!("{other:?}"),
    }
}

fn us(us: u32) -> String {
    match us {
        0 => "0 µs".into(),
        us if us % 1_000_000 == 0 => format!("{} s", us / 1_000_000),
        us if us % 1_000 == 0 => format!("{} ms", us / 1_000),
        us => format!("{us} µs"),
    }
}

/// The trait `trait_name` of crate `version` as a level, from the API snapshots this host carries
/// ([crate::snapshots]): the traits a device leaves out of its introspection data, and what a request through a
/// global trait id (`PathKind::GlobalFull`) names. `None` when the version is not embedded or has no such trait.
pub fn global_trait(version: &FullVersionOwned, trait_name: &str) -> Option<DynResource> {
    type Key = (FullVersionOwned, String);
    static CACHE: Mutex<Option<HashMap<Key, Option<DynResource>>>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap();
    cache
        .get_or_insert_with(HashMap::new)
        .entry((version.clone(), trait_name.to_string()))
        .or_insert_with(|| {
            let mut bundle = crate::snapshots::get(version)?.clone();
            crate::client::introspect::inline_known(&mut bundle).ok()?;
            let idx = bundle.traits.iter().position(|t| {
                matches!(
                    t,
                    ApiLevelLocationOwned::InLine { level, crate_idx }
                        if level.trait_name == trait_name
                            && bundle.ext_crates.get(crate_idx.0 as usize) == Some(version)
                )
            })?;
            let path = format!("{}::{trait_name}", version.crate_id);
            Some(DynResource::trait_level(Arc::new(bundle), idx as u32, path))
        })
        .clone()
}
