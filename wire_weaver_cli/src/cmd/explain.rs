//! `ww explain <what> <hex>` (TOOL-9): which bits of a buffer hold which value, field by field, from the API's
//! types alone. The decoding is `ww_self::explain`; this module finds the type and prints the parts. Instead of bytes
//! a value can be given as a Rust expression (`--value`), or the whole call (`motor.set_speed(1.5)`), read by
//! `ww_self::expr` and encoded first.

use crate::cmd::tap::ViewArgs;
use crate::util::{display_path, resolve};
use crate::value_fmt::fmt_value;
use anyhow::{Context, Result, anyhow, bail};
use clap::Args;
use console::{Style, style};
use std::path::PathBuf;
use std::sync::Arc;
use wire_weaver_client::envelope;
use wire_weaver_client::expr::{self, Call};
use wire_weaver_client::ww_self::explain::{Explained, Part, PartKind, explain, explain_fields};
use wire_weaver_client::ww_self::{ApiBundleOwned, TypeOwned, ValueOwned};
use wire_weaver_client::{DynClient, DynResource, DynResourceKind};

#[derive(Args)]
pub(crate) struct ExplainArgs {
    /// A type (`Config`), or a resource path: a method's arguments (`motor.set_speed`, `--return` for its return
    /// value), a property's value, a stream's item. Without bytes: a call (`'motor.set_speed(1.5, Dir::Cw)'`) or a
    /// property write (`'motor.limit = 3'`) as a Rust expression. `request` or `event`: a whole message as sent. A
    /// capture file (`ww capture -o FILE`): everything in it, as `ww monitor` showed it
    #[arg(value_hint = clap::ValueHint::AnyPath)]
    what: String,

    /// The bytes, in hex: `0a1b2c`, `0a 1b 2c` or `0x0a,0x1b`
    hex: Vec<String>,

    /// Read the bytes from a binary file instead
    #[arg(long, conflicts_with_all = ["hex", "value"])]
    file: Option<PathBuf>,

    /// The value as a Rust expression instead of bytes, e.g. `'Config { mode: Mode::Fast, gains: [1.0, 2.5] }'`
    #[arg(long, short('e'), value_name = "EXPR", conflicts_with = "hex")]
    value: Option<String>,

    /// Explain a method's return value instead of its arguments
    #[arg(long("return"))]
    ret: bool,

    /// Explain the whole request a call, write or read (WHAT) is on the wire: envelope and payload
    #[arg(long, conflicts_with_all = ["hex", "file", "value"])]
    request: bool,

    /// With `ww explain event HEX`: the method or property the event answers (a stream's data names its stream)
    #[arg(long("for"), value_name = "PATH")]
    for_path: Option<String>,

    /// Take the API from the crate that defines the ww_trait instead of a device
    #[arg(long("crate"), value_name = "PATH", conflicts_with = "bundle")]
    crate_path: Option<PathBuf>,

    /// Trait name, when the crate has more than one
    #[arg(long("trait"), requires = "crate_path")]
    trait_name: Option<String>,

    /// Take the API from a saved bundle (`.ron`, see `ww api save`) instead of a device
    #[arg(long)]
    bundle: Option<PathBuf>,

    /// How a capture file is shown
    #[command(flatten)]
    view: ViewArgs,

    /// A capture file: read on past damaged blocks (to the next block start) instead of stopping at the first one
    #[arg(long)]
    resync: bool,
}

impl ExplainArgs {
    /// Without `--crate` or `--bundle` the API comes from the device's introspection.
    pub(crate) fn need_device(&self) -> bool {
        self.crate_path.is_none() && self.bundle.is_none() && !self.is_capture()
    }

    /// A WHAT that is written like a file (a path, a `.wwtape`) but is none: said before anything connects, a
    /// device would only be asked to explain a name that is no type or resource.
    pub(crate) fn check(&self) -> Result<()> {
        if !self.hex.is_empty() || self.file.is_some() || self.value.is_some() {
            return Ok(());
        }
        let what = self.what.as_str();
        let looks_like_a_file = what.contains('/')
            || what.starts_with('~')
            || what.ends_with(".wwtape")
            || what.ends_with(".wwtape.partial");
        if looks_like_a_file && !std::path::Path::new(what).exists() {
            bail!(
                "'{what}': no such capture file (a type or resource name is not written as a path)"
            );
        }
        Ok(())
    }

    /// WHAT is a capture file, and nothing else is given: it carries its APIs.
    fn is_capture(&self) -> bool {
        self.hex.is_empty()
            && self.file.is_none()
            && self.value.is_none()
            && crate::cmd::tap::is_capture(&self.what)
    }
}

pub(crate) fn explain_cmd(args: ExplainArgs, device: Option<&DynClient>) -> Result<()> {
    if args.is_capture() {
        return crate::cmd::tap::explain_file(
            std::path::Path::new(&args.what),
            args.view,
            args.resync,
        );
    }
    let mut bundle = load_bundle(&args, device)?;
    wire_weaver_client::ww_self::generics::prepare(&mut bundle)?;
    let bundle = Arc::new(bundle);
    let given = match &args.file {
        Some(path) => {
            Some(std::fs::read(path).with_context(|| format!("reading {}", path.display()))?)
        }
        None if !args.hex.is_empty() => Some(parse_hex(&args.hex.join(" "))?),
        None => None,
    };
    let root = DynResource::root(bundle.clone());
    let (title, bytes, explained) = match (args.what.as_str(), given) {
        ("request", Some(bytes)) => {
            let e = envelope::explain_request(&bytes, &root);
            (e.title, bytes, e.explained)
        }
        ("event", Some(bytes)) => {
            let for_resource = match &args.for_path {
                Some(path) => {
                    Some(resolve(root.clone(), path).with_context(|| format!("'{path}'"))?)
                }
                None => None,
            };
            let e = envelope::explain_event(&bytes, &root, for_resource.as_ref());
            (e.title, bytes, e.explained)
        }
        (_, Some(bytes)) => {
            let (title, target) = target(&args, &bundle)?;
            let explained = explain_target(&bytes, &target, &bundle);
            (title, bytes, explained)
        }
        // one line naming the field (`argument a: 200 is out of range for ub7`), not a chain
        (_, None) => {
            let (title, target, bytes, resource) =
                encode(&args, &bundle).map_err(|e| anyhow!("{e:#}"))?;
            if args.request {
                let resource = resource.ok_or_else(|| {
                    anyhow!("--request: WHAT must be a call, a property write or a property")
                })?;
                let payload = (!matches!(target, Target::None)).then_some(bytes);
                let bytes = envelope::request_bytes(&resource, payload)?;
                let e = envelope::explain_request(&bytes, &root);
                (e.title, bytes, e.explained)
            } else {
                let explained = explain_target(&bytes, &target, &bundle);
                (title, bytes, explained)
            }
        }
    };
    print!("{}", render(&title, &bytes, &explained, &bundle));
    if let Err(e) = explained.result {
        bail!("decoding stopped: {e:#}");
    }
    Ok(())
}

fn load_bundle(args: &ExplainArgs, device: Option<&DynClient>) -> Result<ApiBundleOwned> {
    if let Some(path) = &args.crate_path {
        return wire_weaver_core::load(path, args.trait_name.clone(), false);
    }
    if let Some(path) = &args.bundle {
        return crate::cmd::api::check::parse(path);
    }
    let device = device.ok_or_else(|| anyhow!("no device, and neither --crate nor --bundle"))?;
    Ok(device
        .device_introspect()
        .ok_or_else(|| anyhow!("device did not provide introspection data"))?
        .api_bundle
        .as_ref()
        .clone())
}

/// What the bytes are: one value, or the arguments of a method (the fields of an evolvable struct); `None`: no bytes
/// (a property read, with `--request`).
enum Target {
    Value(TypeOwned),
    Args(Vec<(String, TypeOwned)>),
    None,
}

fn explain_target(bytes: &[u8], target: &Target, bundle: &ApiBundleOwned) -> Explained {
    match target {
        Target::Value(ty) => explain(bytes, ty, bundle),
        Target::Args(defs) => {
            explain_fields(bytes, defs.iter().map(|(n, t)| (n.as_str(), t)), bundle)
        }
        Target::None => explain_fields(bytes, std::iter::empty(), bundle),
    }
}

/// What `args.what` names, with a title for it.
fn target(args: &ExplainArgs, bundle: &Arc<ApiBundleOwned>) -> Result<(String, Target)> {
    if let Some(ty) = find_type(bundle, &args.what) {
        return Ok((args.what.clone(), Target::Value(ty)));
    }
    let resource = resolve(DynResource::root(bundle.clone()), &args.what).with_context(|| {
        format!(
            "'{}' is neither a type nor a resource",
            display_path(&args.what)
        )
    })?;
    resource_target(&resource, &args.what, args.ret)
}

fn resource_target(resource: &DynResource, path: &str, ret: bool) -> Result<(String, Target)> {
    let path = display_path(path);
    Ok(match resource.kind()? {
        DynResourceKind::Method { return_ty, .. } if ret => {
            let ty = return_ty.ok_or_else(|| anyhow!("'{path}' returns nothing"))?;
            (format!("{path} return value"), Target::Value(ty.clone()))
        }
        DynResourceKind::Method { args: defs, .. } => (
            format!("{path} arguments"),
            Target::Args(
                defs.iter()
                    .map(|a| (a.ident.clone(), a.ty.clone()))
                    .collect(),
            ),
        ),
        DynResourceKind::Property { ty, .. } => {
            (format!("{path} value"), Target::Value(ty.clone()))
        }
        DynResourceKind::Stream { ty, .. } => (format!("{path} item"), Target::Value(ty.clone())),
        _ => bail!("'{path}' is not a method, property or stream"),
    })
}

/// No bytes given: the value from `--value`, or the call written as WHAT, encoded; with the resource it is for.
fn encode(
    args: &ExplainArgs,
    bundle: &Arc<ApiBundleOwned>,
) -> Result<(String, Target, Vec<u8>, Option<DynResource>)> {
    if let Some(text) = &args.value {
        let (title, target) = target(args, bundle)?;
        let Target::Value(ty) = &target else {
            bail!(
                "for a method's arguments write the call: ww explain '{}(..)'",
                args.what
            );
        };
        let value = expr::parse_value(text, ty, bundle)?;
        let bytes = value.ser_shrink_wrap_dyn(ty, bundle)?;
        return Ok((title, target, bytes, None));
    }
    let root = DynResource::root(bundle.clone());
    match expr::parse_call(&args.what)? {
        Call::Method { path, args: exprs } => {
            let resource = resolve(root, &path).with_context(|| format!("'{path}'"))?;
            let (title, target) = resource_target(&resource, &path, false)?;
            let Target::Args(defs) = &target else {
                bail!("'{path}' is not a method");
            };
            if exprs.len() != defs.len() {
                bail!(
                    "'{path}' takes {} arguments, got {}",
                    defs.len(),
                    exprs.len()
                );
            }
            let values = exprs
                .iter()
                .zip(defs)
                .map(|(e, (name, ty))| {
                    expr::value_from_expr(e, ty, bundle).with_context(|| format!("argument {name}"))
                })
                .collect::<Result<Vec<_>>>()?;
            let bytes =
                ValueOwned::ser_shrink_wrap_vec_dyn(&values, defs.iter().map(|(_, t)| t), bundle)?;
            Ok((title, target, bytes, Some(resource)))
        }
        Call::Write { path, value } => {
            let resource = resolve(root, &path).with_context(|| format!("'{path}'"))?;
            let (title, target) = resource_target(&resource, &path, false)?;
            let Target::Value(ty) = &target else {
                bail!("'{path}' is not a property");
            };
            let value = expr::value_from_expr(&value, ty, bundle)?;
            let bytes = value.ser_shrink_wrap_dyn(ty, bundle)?;
            Ok((title, target, bytes, Some(resource)))
        }
        // a property read: no payload, only the request
        Call::Path { path } if args.request => {
            let resource = resolve(root, &path).with_context(|| format!("'{path}'"))?;
            Ok((path, Target::None, vec![], Some(resource)))
        }
        Call::Path { path } => bail!(
            "no bytes for '{}': give them in hex, with --file, or as --value 'EXPR'",
            display_path(&path)
        ),
    }
}

/// A struct or enum of the bundle by its name.
fn find_type(bundle: &ApiBundleOwned, name: &str) -> Option<TypeOwned> {
    (0..bundle.types.len() as u32).find_map(|idx| {
        let ident = match bundle.get_ty(idx).ok()?.0 {
            TypeOwned::Struct(s) => &s.ident,
            TypeOwned::Enum(e) => &e.ident,
            _ => return None,
        };
        (ident == name).then_some(TypeOwned::OutOfLine {
            type_idx: shrink_wrap::UNib32(idx),
        })
    })
}

fn parse_hex(text: &str) -> Result<Vec<u8>> {
    let digits: String = text
        .split([' ', ',', '\n', '\t'])
        .map(|w| w.trim_start_matches("0x"))
        .collect();
    if !digits.len().is_multiple_of(2) {
        bail!("odd number of hex digits");
    }
    (0..digits.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&digits[i..i + 2], 16)
                .with_context(|| format!("'{}' is not hex", &digits[i..i + 2]))
        })
        .collect()
}

/// Colours the top-level parts take in turn, in the byte map and the table.
const PALETTE: [u8; 6] = [33, 172, 35, 170, 38, 167];

pub(crate) fn render(
    title: &str,
    bytes: &[u8],
    explained: &Explained,
    bundle: &ApiBundleOwned,
) -> String {
    let parts = &explained.parts;
    // the field each bit belongs to (the part at the lowest depth below the top), for the colours
    let top = parts.iter().map(|p| p.depth).min().unwrap_or(0);
    let field_depth = if parts.iter().filter(|p| p.depth == top).count() == 1 {
        top + 1
    } else {
        top
    };
    // a part with no parts inside it holds its bits itself
    let leaf: Vec<bool> = (0..parts.len())
        .map(|i| {
            parts
                .get(i + 1)
                .is_none_or(|next| next.depth <= parts[i].depth)
        })
        .collect();
    let mut colour = vec![None; bytes.len() * 8];
    let mut fields = Vec::with_capacity(parts.len());
    let mut field = 0usize;
    for (part, leaf) in parts.iter().zip(&leaf) {
        if part.depth == field_depth {
            field += 1;
        }
        let c = PALETTE[field.saturating_sub(1) % PALETTE.len()];
        fields.push(c);
        if *leaf && part.kind != PartKind::Padding {
            let bits = colour.len();
            for bit in bits_of(part).filter(|b| *b < bits) {
                colour[bit] = Some(c);
            }
        }
    }
    let mut out = format!("{} · {} bytes\n\n", style(title).bold(), bytes.len());
    out += &byte_map(bytes, &colour);
    out += "\n";
    out += &format!(
        "{}\n",
        style(format!(
            "{:<16} {:<20} {:<26} {:<12} {}",
            "bits", "raw", "field", "type", "value"
        ))
        .dim()
    );
    for ((part, leaf), c) in parts.iter().zip(&leaf).zip(&fields) {
        let paint = Style::new().color256(*c);
        out += &row(part, *leaf, bytes, top, &paint, bundle);
    }
    let padding = colour.iter().filter(|c| c.is_none()).count();
    let skipped: usize = parts
        .iter()
        .filter(|p| p.kind == PartKind::Skipped)
        .map(|p| p.front.len())
        .sum();
    out += &format!(
        "\n{} bits in values, {padding} padding or unread",
        colour.len() - padding - skipped
    );
    if skipped > 0 {
        out += &format!(", {skipped} skipped (a newer version's)");
    }
    out += "\n";
    out
}

/// Bits of a part: its front range, and 4 per nibble at the back.
fn bits_of(part: &Part) -> impl Iterator<Item = usize> + '_ {
    part.front
        .clone()
        .chain(part.back.start * 4..part.back.end * 4)
}

/// Hex dump, 16 bytes a line, each byte in the colour of the field that holds its first bit.
fn byte_map(bytes: &[u8], colour: &[Option<u8>]) -> String {
    let mut out = String::new();
    for (line, chunk) in bytes.chunks(16).enumerate() {
        out += &format!("{}  ", style(format!("{:04x}", line * 16)).dim());
        for (i, byte) in chunk.iter().enumerate() {
            let at = (line * 16 + i) * 8;
            let c = colour[at..at + 8].iter().flatten().next();
            let text = format!("{byte:02x}");
            out += &match c {
                Some(c) => style(text).color256(*c).to_string(),
                None => style(text).dim().to_string(),
            };
            out += if i == 7 { "  " } else { " " };
        }
        out += "\n";
    }
    out
}

fn row(
    part: &Part,
    leaf: bool,
    bytes: &[u8],
    top: usize,
    paint: &Style,
    bundle: &ApiBundleOwned,
) -> String {
    let indent = "  ".repeat(part.depth - top);
    let label = match (part.label.as_str(), part.kind) {
        ("", _) => "·".to_string(),
        (label, _) => label.to_string(),
    };
    if part.kind == PartKind::Padding {
        let text = format!(
            "{:<16} {:<20} {indent}{label}",
            location(part),
            raw(part, bytes)
        );
        return format!("{}\n", style(text).dim());
    }
    let value = match (&part.value, &part.note) {
        (Some(v), _) => {
            let mut text = fmt_value(v, None, bundle);
            if let ValueOwned::Numeric(n) = v
                && let Some(i) = n.to_i128()
                && part.front.len() >= 8
                && !(0..10).contains(&i)
            {
                let bits = part.front.len() + part.back.len() * 4;
                let mask = if bits >= 128 {
                    u128::MAX
                } else {
                    (1u128 << bits) - 1
                };
                text += &style(format!("  0x{:x}", (i as u128) & mask))
                    .dim()
                    .to_string();
            }
            if let Some(note) = &part.note {
                text += &style(format!("  ({note})")).italic().to_string();
            }
            text
        }
        (None, Some(note)) => style(note).italic().to_string(),
        (None, None) => String::new(),
    };
    let field = format!("{indent}{label}");
    format!(
        "{:<16} {:<20} {} {:<12} {}\n",
        location(part),
        if leaf {
            raw(part, bytes)
        } else {
            String::new()
        },
        if leaf {
            paint.apply_to(format!("{field:<26}")).to_string()
        } else {
            style(format!("{field:<26}")).bold().to_string()
        },
        style(&part.ty).dim(),
        value
    )
}

/// `5` (bit 5), `0‥7` (bits 0 to 7), `↩30l` (bits 3:0 of byte 30, read from the back), `↩29h‥30l`.
fn location(part: &Part) -> String {
    let nibble = |n: usize| format!("{}{}", n / 2, if n.is_multiple_of(2) { "h" } else { "l" });
    let front = match part.front.len() {
        0 => String::new(),
        1 => part.front.start.to_string(),
        _ => format!("{}‥{}", part.front.start, part.front.end - 1),
    };
    let back = match part.back.len() {
        0 => String::new(),
        1 => format!("↩{}", nibble(part.back.start)),
        _ => format!("↩{}‥{}", nibble(part.back.start), nibble(part.back.end - 1)),
    };
    match (front.is_empty(), back.is_empty()) {
        (false, false) => format!("{front} {back}"),
        (true, _) => back,
        (_, true) => front,
    }
}

/// The part's own bits: in binary up to 16, else its bytes in hex (the first 8).
fn raw(part: &Part, bytes: &[u8]) -> String {
    let bit = |i: usize| (bytes[i / 8] >> (7 - i % 8)) & 1;
    let binary = |range: std::ops::Range<usize>| {
        range
            .map(|i| char::from(b'0' + bit(i)))
            .collect::<Vec<_>>()
            .chunks(4)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut out = if part.front.len() <= 16 {
        binary(part.front.clone())
    } else {
        let (from, to) = (part.front.start / 8, part.front.end.div_ceil(8));
        let shown: Vec<String> = bytes[from..to.min(from + 8)]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        shown.join("") + if to - from > 8 { "…" } else { "" }
    };
    if !part.back.is_empty() {
        let back = binary(part.back.start * 4..part.back.end * 4);
        out = if out.is_empty() {
            back
        } else {
            format!("{out} ↩{back}")
        };
    }
    out
}
