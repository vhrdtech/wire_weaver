//! `ww explain <what> <hex>` (TOOL-9): which bits of a buffer hold which value, field by field, from the API's
//! types alone. The decoding is `ww_self::explain`; this module finds the type and prints the parts.

use crate::util::{display_path, resolve};
use crate::value_fmt::fmt_value;
use anyhow::{Context, Result, anyhow, bail};
use clap::Args;
use console::{Style, style};
use std::path::PathBuf;
use std::sync::Arc;
use wire_weaver_client::ww_self::explain::{Explained, Part, PartKind, explain, explain_fields};
use wire_weaver_client::ww_self::{ApiBundleOwned, TypeOwned, ValueOwned};
use wire_weaver_client::{DynClient, DynResource, DynResourceKind};

#[derive(Args)]
pub(crate) struct ExplainArgs {
    /// A type (`Config`), or a resource path: a method's arguments (`motor.set_speed`, `--return` for its return
    /// value), a property's value, a stream's item
    what: String,

    /// The bytes, in hex: `0a1b2c`, `0a 1b 2c` or `0x0a,0x1b`
    #[arg(required_unless_present = "file", num_args = 1..)]
    hex: Vec<String>,

    /// Read the bytes from a binary file instead
    #[arg(long, conflicts_with = "hex")]
    file: Option<PathBuf>,

    /// Explain a method's return value instead of its arguments
    #[arg(long("return"))]
    ret: bool,

    /// Take the API from the crate that defines the ww_trait instead of a device
    #[arg(long("crate"), value_name = "PATH", conflicts_with = "bundle")]
    crate_path: Option<PathBuf>,

    /// Trait name, when the crate has more than one
    #[arg(long("trait"), requires = "crate_path")]
    trait_name: Option<String>,

    /// Take the API from a saved bundle (`.ron`, see `ww api save`) instead of a device
    #[arg(long)]
    bundle: Option<PathBuf>,
}

impl ExplainArgs {
    /// Without `--crate` or `--bundle` the API comes from the device's introspection.
    pub(crate) fn need_device(&self) -> bool {
        self.crate_path.is_none() && self.bundle.is_none()
    }
}

pub(crate) fn explain_cmd(args: ExplainArgs, device: Option<&DynClient>) -> Result<()> {
    let bundle = Arc::new(load_bundle(&args, device)?);
    let bytes = match &args.file {
        Some(path) => std::fs::read(path).with_context(|| format!("reading {}", path.display()))?,
        None => parse_hex(&args.hex.join(" "))?,
    };
    let (title, explained) = run(&args, &bundle, &bytes)?;
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

/// What `args.what` names, decoded from `bytes`, with a title for it.
fn run(
    args: &ExplainArgs,
    bundle: &Arc<ApiBundleOwned>,
    bytes: &[u8],
) -> Result<(String, Explained)> {
    if let Some(ty) = find_type(bundle, &args.what) {
        return Ok((args.what.clone(), explain(bytes, &ty, bundle)));
    }
    let resource = resolve(DynResource::root(bundle.clone()), &args.what).with_context(|| {
        format!(
            "'{}' is neither a type nor a resource",
            display_path(&args.what)
        )
    })?;
    let path = display_path(&args.what);
    Ok(match resource.kind()? {
        DynResourceKind::Method { return_ty, .. } if args.ret => {
            let ty = return_ty.ok_or_else(|| anyhow!("'{path}' returns nothing"))?;
            (format!("{path} return value"), explain(bytes, ty, bundle))
        }
        DynResourceKind::Method { args: defs, .. } => (
            format!("{path} arguments"),
            explain_fields(
                bytes,
                defs.iter().map(|a| (a.ident.as_str(), &a.ty)),
                bundle,
            ),
        ),
        DynResourceKind::Property { ty, .. } => {
            (format!("{path} value"), explain(bytes, ty, bundle))
        }
        DynResourceKind::Stream { ty, .. } => (format!("{path} item"), explain(bytes, ty, bundle)),
        _ => bail!("'{path}' is not a method, property or stream"),
    })
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

fn render(title: &str, bytes: &[u8], explained: &Explained, bundle: &ApiBundleOwned) -> String {
    let parts = &explained.parts;
    // the field each bit belongs to (the part at the lowest depth below the top), for the colours
    let top = parts.iter().map(|p| p.depth).min().unwrap_or(0);
    let field_depth = if parts.iter().filter(|p| p.depth == top).count() == 1 {
        top + 1
    } else {
        top
    };
    let mut colour = vec![None; bytes.len() * 8];
    let mut field = 0usize;
    for part in parts {
        if part.depth == field_depth {
            field += 1;
        }
        if part.value.is_some() {
            for bit in bits_of(part) {
                colour[bit] = Some(PALETTE[field.saturating_sub(1) % PALETTE.len()]);
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
    let mut field = 0usize;
    for part in parts {
        if part.depth == field_depth {
            field += 1;
        }
        let paint = Style::new().color256(PALETTE[field.saturating_sub(1) % PALETTE.len()]);
        out += &row(part, bytes, top, &paint, bundle);
    }
    let padding = colour.iter().filter(|c| c.is_none()).count();
    out += &format!(
        "\n{} bits in values, {padding} padding or unread\n",
        colour.len() - padding
    );
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

fn row(part: &Part, bytes: &[u8], top: usize, paint: &Style, bundle: &ApiBundleOwned) -> String {
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
            if part.kind == PartKind::Discriminant
                && let Some(note) = &part.note
            {
                text += &format!(" ({note})");
            }
            text
        }
        (None, Some(note)) => style(note).italic().to_string(),
        (None, None) => String::new(),
    };
    let field = format!("{indent}{label}");
    let leaf = part.value.is_some();
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
