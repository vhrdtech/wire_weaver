//! Records what goes over the link on the device side, at two levels: `ww_link` messages (decoded into method calls
//! and replies with the API model of the firmware that runs) and the wire units of the medium (USB packets, UDP
//! datagrams, WebSocket frames). [Recorder::transcript] renders both as the markdown the docs include, and
//! [check_or_update] compares it with the golden file (or writes it with `DFU_TRANSCRIPTS=update`).
//!
//! Link setup (`Nop`, `GetDeviceInfo`, `LinkSetup`, `LinkReady`, the WebSocket upgrade) is the same on every
//! connection and carries crate versions that change with every release, so it is summarized in one line and its
//! bytes are left out; pings are left out too.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};

use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use wire_weaver_client::ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiLevelOwned, FieldsValueOwned, TypeOwned, ValueOwned,
};
use ww_client_server::{CreditWrite, Event, EventKind, PathKind, Request, RequestKind};
use ww_link::Kind;

use crate::flash::Firmware;

/// Which way something went.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Dir {
    ToDevice,
    ToHost,
}

impl Dir {
    fn arrow(self) -> &'static str {
        match self {
            Dir::ToDevice => "→",
            Dir::ToHost => "←",
        }
    }
}

#[derive(Clone, Debug)]
enum Rec {
    Note(String),
    Boot(Firmware),
    /// A unit of the medium; `setup` for the WebSocket upgrade
    Packet {
        dir: Dir,
        bytes: Vec<u8>,
        setup: bool,
    },
    Message {
        dir: Dir,
        kind: u8,
        bytes: Vec<u8>,
    },
}

/// Shared by the device's taps and the test.
#[derive(Clone, Default)]
pub struct Recorder(Arc<Mutex<Vec<Rec>>>);

impl Recorder {
    /// A line of commentary in both views: a reboot, a power cut, what the bootloader does, what the host does.
    pub fn note(&self, note: &str) {
        self.push(Rec::Note(note.to_string()));
    }

    pub fn boot(&self, fw: Firmware) {
        self.push(Rec::Boot(fw));
    }

    pub fn packet(&self, dir: Dir, bytes: &[u8]) {
        self.push(Rec::Packet {
            dir,
            bytes: bytes.to_vec(),
            setup: false,
        });
    }

    fn push(&self, rec: Rec) {
        self.0.lock().unwrap().push(rec);
    }

    /// Render the transcript as markdown: the readable exchange, then the wire bytes in a collapsed block.
    pub fn transcript(&self, medium: &str) -> String {
        let recs = self.0.lock().unwrap().clone();
        let hidden = hidden_packets(&recs);
        let mut readable = String::new();
        let mut wire = String::new();
        let mut decoder = Decoder::default();
        let mut setup = false;
        let mut packets = 0;
        for (i, rec) in recs.iter().enumerate() {
            match rec {
                Rec::Note(n) => {
                    writeln!(readable, "   ── {n}").unwrap();
                    writeln!(wire, "   ── {n}").unwrap();
                }
                Rec::Boot(fw) => {
                    decoder.firmware(*fw);
                    writeln!(readable, "   ── device runs the {}", fw.name()).unwrap();
                    writeln!(wire, "   ── device runs the {}", fw.name()).unwrap();
                }
                Rec::Packet { dir, bytes, .. } => {
                    if hidden.contains(&i) {
                        continue;
                    }
                    packets += 1;
                    write_hex_lines(&mut wire, dir.arrow(), bytes);
                }
                Rec::Message { dir, kind, bytes } => {
                    let Some(kind) = Kind::from_repr(*kind) else {
                        writeln!(readable, "{} ?? kind {kind}", dir.arrow()).unwrap();
                        continue;
                    };
                    match kind {
                        Kind::Nop | Kind::GetDeviceInfo if *dir == Dir::ToDevice => {
                            if !setup {
                                setup = true;
                                decoder.reset_seq();
                            }
                        }
                        Kind::LinkReady => {
                            setup = false;
                            writeln!(
                                readable,
                                "   ── link up: {} (link setup not shown, same on every link)",
                                decoder.api_name()
                            )
                            .unwrap();
                        }
                        Kind::Ping => {}
                        Kind::Disconnect => {
                            let reason = ww_link::Message::decode(Kind::Disconnect as u8, bytes)
                                .map(|m| format!("{m:?}"))
                                .unwrap_or_default();
                            line(&mut readable, *dir, &reason, bytes);
                        }
                        Kind::Data0 | Kind::Data1 | Kind::Data2 if !setup => {
                            let text = decoder.decode(*dir, bytes);
                            line(&mut readable, *dir, &text, bytes);
                        }
                        _ => {}
                    }
                }
            }
        }
        format!(
            "```text\n{readable}```\n\n<details markdown=\"1\">\n<summary>Wire bytes: {packets} {medium} (link setup and pings left out)</summary>\n\n```text\n{wire}```\n\n</details>\n"
        )
    }
}

/// Packets of link setup (until the device's `LinkReady` went out) and lone pings.
fn hidden_packets(recs: &[Rec]) -> Vec<usize> {
    let mut hidden = vec![];
    let mut setup = false;
    let mut last_rx_packets: Vec<usize> = vec![];
    let mut hide_next_tx = false;
    for (i, rec) in recs.iter().enumerate() {
        match rec {
            Rec::Packet {
                dir,
                bytes,
                setup: s,
            } => {
                let lone_ping = is_lone_ping(bytes);
                if *s || setup || lone_ping || (*dir == Dir::ToHost && hide_next_tx) {
                    hidden.push(i);
                    if *dir == Dir::ToHost {
                        hide_next_tx = false;
                    }
                } else if *dir == Dir::ToDevice {
                    last_rx_packets.push(i);
                }
            }
            Rec::Message { dir, kind, .. } => {
                let kind = Kind::from_repr(*kind);
                match (dir, kind) {
                    (Dir::ToDevice, Some(Kind::Nop | Kind::GetDeviceInfo)) if !setup => {
                        // the packet this came in was setup already
                        if let Some(p) = last_rx_packets.pop() {
                            hidden.push(p);
                        }
                        setup = true;
                    }
                    (Dir::ToHost, Some(Kind::LinkReady)) => {
                        setup = false;
                        hide_next_tx = true;
                    }
                    _ => {}
                }
                if *dir == Dir::ToDevice {
                    last_rx_packets.clear();
                }
            }
            Rec::Boot(_) => setup = true,
            Rec::Note(_) => {}
        }
    }
    hidden
}

/// A frame (or WebSocket message) that carries nothing but a ping.
fn is_lone_ping(bytes: &[u8]) -> bool {
    static PING: LazyLock<Vec<u8>> = LazyLock::new(|| {
        let mut tx =
            ww_framer::TxOwned::<ww_link::UsbHead, ww_link::UsbChecksum, ww_link::UsbTail>::new(64);
        assert!(tx.write(Kind::Ping as u8, &[]).unwrap());
        tx.flush_to_vec().unwrap()
    });
    bytes == PING.as_slice() || ws_payload(bytes).is_some_and(|p| p == [Kind::Ping as u8])
}

/// One readable line: the decoded message, then its size and first bytes.
fn line(out: &mut String, dir: Dir, text: &str, bytes: &[u8]) {
    let short = short_hex(bytes, 10);
    // long values (NodeInfo) wrapped after a comma
    let mut row = format!("{} ", dir.arrow());
    for (i, part) in text.split_inclusive(", ").enumerate() {
        if i > 0 && row.chars().count() + part.len() > 112 {
            writeln!(out, "{}", row.trim_end()).unwrap();
            row = "      ".into();
        }
        row.push_str(part);
    }
    writeln!(out, "{row}").unwrap();
    writeln!(out, "      {:>4} B  {short}", bytes.len()).unwrap();
}

fn short_hex(bytes: &[u8], max: usize) -> String {
    let mut s: Vec<String> = bytes.iter().take(max).map(|b| format!("{b:02x}")).collect();
    if bytes.len() > max {
        s.push("…".into());
    }
    s.join(" ")
}

fn write_hex_lines(out: &mut String, arrow: &str, bytes: &[u8]) {
    for (i, row) in bytes.chunks(16).enumerate() {
        let hex: Vec<String> = row.iter().map(|b| format!("{b:02x}")).collect();
        if i == 0 {
            writeln!(out, "{arrow} {:>4} B  {}", bytes.len(), hex.join(" ")).unwrap();
        } else {
            writeln!(out, "          {}", hex.join(" ")).unwrap();
        }
    }
}

// ---- decoding with the API model

/// API model of each firmware, from the API crates' source.
fn bundle(fw: Firmware) -> Arc<ApiBundleOwned> {
    static BUNDLES: LazyLock<Mutex<HashMap<&'static str, Arc<ApiBundleOwned>>>> =
        LazyLock::new(Default::default);
    let (dir, name) = match fw {
        Firmware::App => ("dfu_app_api", "DfuApp"),
        Firmware::LeanApp => ("dfu_lean_app_api", "DfuLeanApp"),
        Firmware::Updater => ("dfu_updater_api", "DfuUpdater"),
    };
    BUNDLES
        .lock()
        .unwrap()
        .entry(dir)
        .or_insert_with(|| {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(dir);
            Arc::new(wire_weaver_core::load(&path, Some(name.into()), false).unwrap())
        })
        .clone()
}

/// A resource found by its index path: dotted name and kind.
struct Resource {
    name: String,
    kind: ApiItemKindOwned,
}

#[derive(Default)]
struct Decoder {
    fw: Option<Firmware>,
    /// Requests waiting for their reply, by seq
    pending: HashMap<u32, Resource>,
}

impl Decoder {
    fn firmware(&mut self, fw: Firmware) {
        self.fw = Some(fw);
        self.reset_seq();
    }

    fn reset_seq(&mut self) {
        self.pending.clear();
    }

    fn api_name(&self) -> String {
        match self.fw {
            Some(fw) => {
                let b = bundle(fw);
                b.root.crate_name(&b).unwrap_or("?").to_string()
            }
            None => "?".into(),
        }
    }

    fn resolve(&self, path: &[UNib32]) -> Option<Resource> {
        let b = bundle(self.fw?);
        let mut level: &ApiLevelOwned = &b.root;
        let mut names = vec![];
        for (i, idx) in path.iter().enumerate() {
            let item = level.items.iter().find(|it| it.id == *idx)?;
            names.push(item.ident.clone());
            match &item.kind {
                ApiItemKindOwned::Trait { trait_idx } => level = b.get_trait(trait_idx.0).ok()?,
                kind if i + 1 == path.len() => {
                    return Some(Resource {
                        name: names.join("."),
                        kind: kind.clone(),
                    });
                }
                _ => return None,
            }
        }
        None
    }

    fn decode(&mut self, dir: Dir, bytes: &[u8]) -> String {
        let r = match dir {
            Dir::ToDevice => self.request(bytes),
            Dir::ToHost => self.event(bytes),
        };
        r.unwrap_or_else(|e| format!("?? {e}"))
    }

    fn request(&mut self, bytes: &[u8]) -> Result<String, String> {
        let req = Request::from_ww_bytes(bytes).map_err(|e| format!("{e:?}"))?;
        let path: Vec<UNib32> = match &req.path_kind {
            PathKind::Absolute { path } => path.iter().map(|p| p.unwrap()).collect(),
            other => return Err(format!("path {other:?}")),
        };
        let res = self.resolve(&path).ok_or("unknown path")?;
        let b = bundle(self.fw.unwrap());
        let seq = req.seq.0;
        let text = match (&req.kind, &res.kind) {
            (RequestKind::Call { args }, ApiItemKindOwned::Method { args: defs, .. }) => {
                let values =
                    ValueOwned::des_shrink_wrap_vec_dyn(args.0, defs.iter().map(|a| &a.ty), &b)
                        .map_err(|e| format!("{e:#}"))?;
                let args: Vec<String> = defs
                    .iter()
                    .zip(&values)
                    .map(|(d, v)| format!("{}: {}", d.ident, fmt_value(v)))
                    .collect();
                format!("{}({})", res.name, args.join(", "))
            }
            (RequestKind::Write { data }, ApiItemKindOwned::FlowSink { ty, .. }) => {
                let w = CreditWrite::from_ww_bytes(data.0).map_err(|e| format!("{e:?}"))?;
                let v = ValueOwned::des_shrink_wrap_dyn(w.data.0, ty, &b)
                    .map_err(|e| format!("{e:#}"))?;
                format!(
                    "{} ⇐ {} (sink position {})",
                    res.name,
                    fmt_value(&v),
                    w.pos.0
                )
            }
            (RequestKind::Write { data }, ApiItemKindOwned::Stream { ty, .. }) => {
                let v = ValueOwned::des_shrink_wrap_dyn(data.0, ty, &b)
                    .map_err(|e| format!("{e:#}"))?;
                format!("{} ⇐ {}", res.name, fmt_value(&v))
            }
            (RequestKind::StreamSideband { sideband }, _) => {
                format!("{}: {}", res.name, fmt_sideband(sideband))
            }
            (kind, _) => format!("{}: {kind:?}", res.name),
        };
        if seq != 0 {
            self.pending.insert(seq, res);
        }
        Ok(text)
    }

    fn event(&mut self, bytes: &[u8]) -> Result<String, String> {
        let ev = Event::from_ww_bytes(bytes).map_err(|e| format!("{e:?}"))?;
        let b = bundle(self.fw.ok_or("no firmware")?);
        let kind = match ev.result {
            Ok(kind) => kind,
            Err(e) => return Ok(format!("error {e:?}")),
        };
        Ok(match kind {
            EventKind::Value { data } => {
                let res = self.pending.remove(&ev.seq.0).ok_or("reply to nothing")?;
                match &res.kind {
                    ApiItemKindOwned::Method {
                        return_ty: Some(ty),
                        ..
                    } => fmt_value(
                        &ValueOwned::des_shrink_wrap_dyn(data.0, ty, &b)
                            .map_err(|e| format!("{e:#}"))?,
                    ),
                    _ => "()".into(),
                }
            }
            EventKind::Written => {
                self.pending.remove(&ev.seq.0);
                "written".into()
            }
            EventKind::StreamData { path, data } => {
                let path: Vec<UNib32> = path.iter().map(|p| p.unwrap()).collect();
                let res = self.resolve(&path).ok_or("unknown stream")?;
                let ApiItemKindOwned::Stream { ty, .. } = &res.kind else {
                    return Err("not a stream".into());
                };
                let v = ValueOwned::des_shrink_wrap_dyn(TailBytes(data.0).0, ty, &b)
                    .map_err(|e| format!("{e:#}"))?;
                format!("{} ⇒ {}", res.name, fmt_value(&v))
            }
            EventKind::StreamSideband { path, sideband } => {
                self.pending.remove(&ev.seq.0);
                let path: Vec<UNib32> = path.iter().map(|p| p.unwrap()).collect();
                let res = self.resolve(&path).ok_or("unknown stream")?;
                format!("{}: {}", res.name, fmt_sideband(&sideband))
            }
            EventKind::StreamItem { path, head, .. } => {
                let path: Vec<UNib32> = path.iter().map(|p| p.unwrap()).collect();
                let res = self.resolve(&path).ok_or("unknown stream")?;
                format!("{} at {} ⇒ (not decoded)", res.name, head.pos)
            }
        })
    }
}

fn fmt_sideband(s: &ww_client_server::StreamSideband) -> String {
    use ww_client_server::StreamSideband::*;
    match s {
        Open => "open".into(),
        Close => "close".into(),
        Credit { have, grant } => {
            format!("credit: have {} bytes, may send {} more", have.0, grant.0)
        }
        CreditQuery { sent } => format!("credit query: sent {} bytes", sent.0),
        Resend { have, grant } => format!("resend from byte {}, may send {} more", have.0, grant.0),
        other => format!("{other:?}"),
    }
}

/// Compact form of a value: numbers plain, byte arrays as hex, structs with their field names.
pub fn fmt_value(v: &ValueOwned) -> String {
    match v {
        ValueOwned::Bool(b) => b.to_string(),
        ValueOwned::Numeric(n) => {
            // `U32(5)` -> `5`
            let d = format!("{n:?}");
            match (d.find('('), d.rfind(')')) {
                (Some(a), Some(b)) => d[a + 1..b].to_string(),
                _ => d,
            }
        }
        ValueOwned::String(s) => format!("{s:?}"),
        ValueOwned::Vec(items) | ValueOwned::Array(items) => {
            if let Some(bytes) = as_bytes(items) {
                return fmt_bytes(&bytes, matches!(v, ValueOwned::Array(_)));
            }
            let items: Vec<String> = items.iter().map(fmt_value).collect();
            format!("[{}]", items.join(", "))
        }
        ValueOwned::Tuple(items) => {
            let items: Vec<String> = items.iter().map(fmt_value).collect();
            format!("({})", items.join(", "))
        }
        ValueOwned::Struct { fields } => fmt_fields(fields, true),
        ValueOwned::Enum { variant, fields } => match fields {
            FieldsValueOwned::Unit => variant.clone(),
            _ => format!("{variant}{}", fmt_fields(fields, false)),
        },
        ValueOwned::Option(o) => match o {
            Some(v) => format!("Some({})", fmt_value(v)),
            None => "None".into(),
        },
        ValueOwned::Result(r) => match r {
            Ok(v) => format!("Ok({})", fmt_value(v)),
            Err(v) => format!("Err({})", fmt_value(v)),
        },
        other => format!("{other:?}"),
    }
}

fn fmt_fields(fields: &FieldsValueOwned, braces_always: bool) -> String {
    match fields {
        FieldsValueOwned::Named(f) => {
            let f: Vec<String> = f
                .iter()
                .map(|(n, v)| format!("{n}: {}", fmt_value(v)))
                .collect();
            format!(" {{ {} }}", f.join(", "))
                .trim_start_matches(if braces_always { " " } else { "" })
                .to_string()
        }
        FieldsValueOwned::Unnamed(f) => {
            let f: Vec<String> = f.iter().map(fmt_value).collect();
            format!("({})", f.join(", "))
        }
        FieldsValueOwned::Unit => String::new(),
    }
}

fn as_bytes(items: &[ValueOwned]) -> Option<Vec<u8>> {
    items
        .iter()
        .map(|v| match v {
            ValueOwned::Numeric(n) => {
                let d = format!("{n:?}");
                d.strip_prefix("U8(")?.strip_suffix(')')?.parse().ok()
            }
            _ => None,
        })
        .collect::<Option<Vec<u8>>>()
        .filter(|b| !b.is_empty())
}

/// Hashes (8-byte arrays) in full, longer byte strings shortened.
fn fmt_bytes(bytes: &[u8], array: bool) -> String {
    if array && bytes.len() <= 8 {
        return bytes.iter().map(|b| format!("{b:02x}")).collect();
    }
    format!("[{} B: {}]", bytes.len(), short_hex(bytes, 6))
}

/// Unused: kept for types that might show up in replies later.
#[allow(dead_code)]
fn type_name(ty: &TypeOwned) -> String {
    format!("{ty:?}")
}

// ---- taps

/// [ww_device::MessageTx] that records every message.
pub struct TapTx<T> {
    pub inner: T,
    pub rec: Recorder,
}

impl<T: ww_device::MessageTx> ww_device::MessageTx for TapTx<T> {
    type Error = T::Error;

    async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), T::Error> {
        self.rec.push(Rec::Message {
            dir: Dir::ToHost,
            kind,
            bytes: message.to_vec(),
        });
        self.inner.write_message(kind, message).await
    }

    async fn flush(&mut self) -> Result<(), T::Error> {
        self.inner.flush().await
    }

    fn reset(&mut self) {
        self.inner.reset()
    }
}

/// [ww_device::MessageRx] that records every message once.
pub struct TapRx<R> {
    pub inner: R,
    pub rec: Recorder,
    pub recorded: bool,
}

impl<R: ww_device::MessageRx> ww_device::MessageRx for TapRx<R> {
    type Error = R::Error;

    async fn wait_message(&mut self) -> Result<(), R::Error> {
        self.inner.wait_message().await?;
        if !self.recorded
            && let Some((kind, bytes)) = self.inner.message()
        {
            self.recorded = true;
            self.rec.push(Rec::Message {
                dir: Dir::ToDevice,
                kind,
                bytes: bytes.to_vec(),
            });
        }
        Ok(())
    }

    fn message(&self) -> Option<(u8, &[u8])> {
        self.inner.message()
    }

    fn consume(&mut self) {
        self.recorded = false;
        self.inner.consume()
    }

    async fn wait_connected(&mut self) {
        self.inner.wait_connected().await
    }

    fn reset(&mut self) {
        self.recorded = false;
        self.inner.reset()
    }

    fn max_message_len(&self) -> usize {
        self.inner.max_message_len()
    }
}

/// [ww_device::PacketSink] and [ww_device::PacketSource] that record every packet (USB).
pub struct TapPackets<P> {
    pub inner: P,
    pub rec: Recorder,
}

impl<P: ww_device::PacketSink> ww_device::PacketSink for TapPackets<P> {
    type Error = P::Error;

    async fn write_packet(&mut self, packet: &[u8]) -> Result<(), P::Error> {
        self.rec.packet(Dir::ToHost, packet);
        self.inner.write_packet(packet).await
    }
}

impl<P: ww_device::PacketSource> ww_device::PacketSource for TapPackets<P> {
    type Error = P::Error;

    fn max_packet_len(&self) -> usize {
        self.inner.max_packet_len()
    }

    async fn read_packet(&mut self, buf: &mut [u8]) -> Result<usize, P::Error> {
        let len = self.inner.read_packet(buf).await?;
        self.rec.packet(Dir::ToDevice, &buf[..len]);
        Ok(len)
    }

    async fn wait_connected(&mut self) {
        self.inner.wait_connected().await
    }
}

/// Splits a WebSocket byte stream (one direction) into the HTTP upgrade and frames, records each.
pub struct WsSplitter {
    dir: Dir,
    buf: Vec<u8>,
    upgraded: bool,
}

impl WsSplitter {
    pub fn new(dir: Dir) -> Self {
        WsSplitter {
            dir,
            buf: vec![],
            upgraded: false,
        }
    }

    pub fn feed(&mut self, rec: &Recorder, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
        loop {
            if !self.upgraded {
                let Some(end) = self.buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                    return;
                };
                let head: Vec<u8> = self.buf.drain(..end + 4).collect();
                rec.push(Rec::Packet {
                    dir: self.dir,
                    bytes: head,
                    setup: true,
                });
                self.upgraded = true;
                continue;
            }
            let Some(len) = ws_frame_len(&self.buf) else {
                return;
            };
            let frame: Vec<u8> = self.buf.drain(..len).collect();
            rec.push(Rec::Packet {
                dir: self.dir,
                bytes: normalize_mask(&frame),
                setup: false,
            });
        }
    }
}

/// Length of the WebSocket frame at the start of `b`, if it is all there.
fn ws_frame_len(b: &[u8]) -> Option<usize> {
    let (header, payload) = ws_header(b)?;
    (b.len() >= header + payload).then_some(header + payload)
}

fn ws_header(b: &[u8]) -> Option<(usize, usize)> {
    if b.len() < 2 {
        return None;
    }
    let masked = b[1] & 0x80 != 0;
    let (mut header, payload) = match b[1] & 0x7F {
        126 if b.len() >= 4 => (4, u16::from_be_bytes([b[2], b[3]]) as usize),
        127 if b.len() >= 10 => (
            10,
            u64::from_be_bytes(b[2..10].try_into().unwrap()) as usize,
        ),
        126 | 127 => return None,
        n => (2, n as usize),
    };
    if masked {
        header += 4;
    }
    (b.len() >= header).then_some((header, payload))
}

/// Host frames are masked with a random key: show them unmasked, with the key zeroed, so a transcript is the same on
/// every run. The docs say so.
fn normalize_mask(frame: &[u8]) -> Vec<u8> {
    let mut f = frame.to_vec();
    if f[1] & 0x80 == 0 {
        return f;
    }
    let (header, _) = ws_header(&f).unwrap();
    let key: [u8; 4] = f[header - 4..header].try_into().unwrap();
    for (i, b) in f[header..].iter_mut().enumerate() {
        *b ^= key[i % 4];
    }
    f[header - 4..header].fill(0);
    f
}

/// Payload of a WebSocket frame (unmasked by [normalize_mask]).
fn ws_payload(frame: &[u8]) -> Option<&[u8]> {
    if frame.first()? & 0x0F != 2 {
        return None;
    }
    let (header, len) = ws_header(frame)?;
    frame.get(header..header + len)
}

// ---- golden files

/// Compare `transcript` with `tests/dfu_scenarios/transcripts/<code>.md`, or write it when `DFU_TRANSCRIPTS=update`
/// (`just dfu-transcripts`). The docs include these files, so a change of the exchange fails here until the
/// transcripts are regenerated and the change is looked at.
pub fn check_or_update(code: &str, transcript: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("transcripts")
        .join(format!("{code}.md"));
    if std::env::var("DFU_TRANSCRIPTS").as_deref() == Ok("update") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, transcript).unwrap();
        return;
    }
    let golden = std::fs::read_to_string(&path).unwrap_or_default();
    if golden != transcript {
        let actual = path.with_extension("actual.md");
        std::fs::write(&actual, transcript).unwrap();
        panic!(
            "transcript of {code} changed: compare {} with {}, then run `just dfu-transcripts` to accept it",
            path.display(),
            actual.display()
        );
    }
}
