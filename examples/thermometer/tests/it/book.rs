//! What the book shows of the thermometer, checked against `docs/generated/thermometer/`: the `ww` transcripts are
//! the text `ww` prints, from the same device the chapters run, served in process.

use thermometer::SimBoard;
use thermometer_api::{Gain, Rate};
use tokio::sync::oneshot;
use wire_weaver::shrink_wrap::SerializeShrinkWrap;
use wire_weaver_cli::cmd::introspect::{IntrospectView, render};
use wire_weaver_client::{DynClient, Error, in_process};
use ww_client_server::{ErrorKindOwned, ErrorOwned, PathKind};

/// The URL a reader's simulated device serves at: `thermometer_sim`'s default.
const SIM_URL: &str = "ws://127.0.0.1:8080/ww";

/// Runs `test` against a simulated thermometer at `path`, through the dynamic client `ww` and Python use.
async fn with_dyn_device<F: Future<Output = ()>>(path: &str, test: impl FnOnce(DynClient) -> F) {
    let local = tokio::task::LocalSet::new();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let incoming = in_process::ws_device(path);
    let device = local.spawn_local(thermometer::simulated(
        SimBoard::default(),
        incoming,
        async {
            _ = stop_rx.await;
        },
    ));
    local
        .run_until(async {
            let config = wire_weaver_client::ClientConfig::new()
                .in_process_path(path.to_owned())
                .no_ww_usb();
            let host = DynClient::from_config(config)
                .connect()
                .await
                .expect("connects");
            test(host).await;
            _ = stop_tx.send(());
            device.await.expect("the device runs to its stop");
        })
        .await;
}

#[tokio::test(start_paused = true)]
async fn book_output_introspect() {
    console::set_colors_enabled(false);
    with_dyn_device("thermometer/book_introspect", |dev| async move {
        let view = IntrospectView {
            skip_docs: true,
            ..IntrospectView::default()
        };
        let tree = render(&dev, view).await.expect("renders");
        book_outputs::check(
            "thermometer/introspect.txt",
            &format!("$ ww --websocket {SIM_URL} introspect -d\n{tree}"),
        );
    })
    .await;
}

// --8<-- [start:gain_bounds]
/// A value outside a property's bounds never reaches its handler: the device refuses the bytes when it decodes them.
/// The typed client can't even build one (`Gain::try_new(20)` fails), so the test writes the bytes itself.
#[tokio::test(start_paused = true)]
async fn the_device_refuses_a_gain_out_of_bounds() {
    assert!(Gain::try_new(20).is_err());
    with_dyn_device("thermometer/gain_bounds", |dev| async move {
        let root = dev
            .device_introspect()
            .expect("introspection data")
            .api_bundle;
        let gain = wire_weaver_client::DynResource::root(root)
            .child("gain")
            .expect("gain");
        let path = gain.index_chain().to_vec();
        let write = |byte: u8| {
            dev.cmd()
                .prepare_write::<Error>(
                    PathKind::Absolute {
                        path: wire_weaver::prelude::RefVec::Slice { slice: &path },
                    },
                    Ok(vec![byte]),
                    "gain",
                )
                .write()
        };
        let refused = write(20).await;
        assert!(
            matches!(
                &refused,
                Err(Error::RemoteError(ErrorOwned {
                    kind: ErrorKindOwned::PropertyDesFailed,
                    ..
                }))
            ),
            "{refused:?}"
        );
        write(8).await.expect("8 is in bounds");
    })
    .await;
}
// --8<-- [end:gain_bounds]

/// What `thermometer_units` prints, split by chapter: numbers with units, then temperatures and timestamps.
#[test]
fn book_output_units() {
    let mut out = Vec::new();
    crate::units_example::numbers(&mut out).expect("writes to a Vec");
    book_outputs::check(
        "thermometer/units.txt",
        &format!(
            "$ cargo run -p thermometer --example thermometer_units\n{}",
            text(out)
        ),
    );
    let mut out = Vec::new();
    crate::units_example::points(&mut out).expect("writes to a Vec");
    book_outputs::check("thermometer/points.txt", &text(out));
}

/// A rate of 250 Hz and a bare `u16` of 250 as `bits` fences, from their real bytes: the same two bytes.
#[test]
fn book_output_rate_bits() {
    let mut buf = [0u8; 8];
    let typed = Rate::bounded(250)
        .to_ww_bytes(&mut buf)
        .expect("fits")
        .to_vec();
    book_outputs::check("thermometer/rate_bits.md", &bits_fence(&typed, "250 Hz"));
    let bare = 250u16.to_ww_bytes(&mut buf).expect("fits").to_vec();
    book_outputs::check("thermometer/u16_bits.md", &bits_fence(&bare, "250"));
    assert_eq!(typed, bare);
}

/// A `bits` fence of a little-endian number's bytes: one `value` field per byte, low byte first.
fn bits_fence(bytes: &[u8], what: &str) -> String {
    let mut fence = String::from("```bits\n");
    for (i, b) in bytes.iter().enumerate() {
        let label = match i {
            0 => format!("{what}: low byte, 0x{b:02x}"),
            _ => format!("high byte, 0x{b:02x}"),
        };
        fence += &format!("value {b:08b}  {label}\n");
    }
    let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
    fence += &format!("bytes {}\n```\n", hex.join(" "));
    fence
}

fn text(out: Vec<u8>) -> String {
    String::from_utf8(out).expect("the examples write UTF-8")
}

/// What `thermometer_observe` prints, and the book's timeline figure drawn from the same lines.
#[tokio::test(start_paused = true)]
async fn book_output_observe() {
    let mut out = Vec::new();
    crate::with_device("thermometer/book_observe", |t| async move {
        crate::observe_example::run(&t, &mut out)
            .await
            .expect("runs");
        let lines = text(out);
        book_outputs::check(
            "thermometer/observe.txt",
            &format!("$ cargo run -p thermometer --example thermometer_observe\n{lines}"),
        );
        book_outputs::check(
            "thermometer/observe_timeline.svg",
            &observe_timeline(&lines),
        );
    })
    .await;
}

/// What `thermometer_stream` prints: continuous samples, then triggered ones.
#[tokio::test(start_paused = true)]
async fn book_output_stream() {
    let mut out = Vec::new();
    crate::with_device("thermometer/book_stream", |t| async move {
        crate::stream_example::run(&t, &mut out)
            .await
            .expect("runs");
        book_outputs::check(
            "thermometer/stream.txt",
            &format!(
                "$ cargo run -p thermometer --example thermometer_stream\n{}",
                text(out)
            ),
        );
    })
    .await;
}

/// What `thermometer_channels` prints.
#[tokio::test(start_paused = true)]
async fn book_output_channels() {
    let mut out = Vec::new();
    crate::with_device("thermometer/book_channels", |t| async move {
        crate::channels_example::run(&t, &mut out)
            .await
            .expect("runs");
        book_outputs::check(
            "thermometer/channels.txt",
            &format!(
                "$ cargo run -p thermometer --example thermometer_channels\n{}",
                text(out)
            ),
        );
    })
    .await;
}

/// One line of `thermometer_observe`: its time in seconds, and what happened.
enum Step {
    Host(f32, String),
    Update(f32, String),
}

fn steps(lines: &str) -> Vec<Step> {
    lines
        .lines()
        .filter_map(|line| {
            let (t, what) = line.split_once(" s  ")?;
            let t: f32 = t.parse().ok()?;
            Some(match what.strip_prefix("update: gain ") {
                Some(value) => Step::Update(t, value.to_owned()),
                None => Step::Host(t, what.trim_start_matches("host ").to_owned()),
            })
        })
        .collect()
}

/// A timeline of the observed gain: the host's actions, the gain on the device, and the updates the host receives.
fn observe_timeline(lines: &str) -> String {
    const LEFT: f32 = 150.0;
    const RIGHT: f32 = 680.0;
    const SECONDS: f32 = 2.5;
    const HOST_Y: f32 = 70.0;
    const GAIN_Y: f32 = 170.0;
    const UPDATE_Y: f32 = 260.0;
    let x = |t: f32| LEFT + t / SECONDS * (RIGHT - LEFT);
    let steps = steps(lines);
    let mut svg = String::from(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"720\" height=\"320\" viewBox=\"0 0 720 320\" \
         font-family=\"Helvetica, Arial, sans-serif\" font-size=\"13\">\n\
         <title>The thermometer's gain observed by a host</title>\n\
         <desc>Three lanes over time: the host program, the gain on the device, and the updates the host receives. \
         Each change of the gain, by the host's write or by the device itself, reaches the host as one update.</desc>\n\
         <defs><marker id=\"obs-arrow\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"7\" \
         markerHeight=\"7\" orient=\"auto-start-reverse\"><path d=\"M0,0 L10,5 L0,10 z\" fill=\"#1f5f99\"/></marker>\
         </defs>\n\
         <rect x=\"0\" y=\"0\" width=\"720\" height=\"320\" rx=\"10\" fill=\"#ffffff\"/>\n",
    );
    for (y, name) in [
        (HOST_Y, "Host program"),
        (GAIN_Y, "Gain on the device"),
        (UPDATE_Y, "Updates received"),
    ] {
        svg += &format!(
            "<text x=\"20\" y=\"{}\" font-weight=\"bold\" fill=\"#222\">{name}</text>\n\
             <line x1=\"{LEFT}\" y1=\"{y}\" x2=\"{RIGHT}\" y2=\"{y}\" stroke=\"#999\" stroke-dasharray=\"4 4\"/>\n",
            y + 4.0
        );
    }
    for s in 0..=2 {
        let t = s as f32;
        svg += &format!(
            "<text x=\"{}\" y=\"305\" text-anchor=\"middle\" fill=\"#555\">{s} s</text>\n",
            x(t)
        );
    }
    // the gain as a step line, changing at each update
    let mut previous: Option<(f32, &str)> = None;
    for step in &steps {
        if let Step::Update(t, value) = step {
            if let Some((t0, v0)) = previous {
                svg += &format!(
                    "<line x1=\"{}\" y1=\"{GAIN_Y}\" x2=\"{}\" y2=\"{GAIN_Y}\" stroke=\"#b03a2e\" stroke-width=\"3\"/>\n\
                     <text x=\"{}\" y=\"{}\" text-anchor=\"middle\" fill=\"#b03a2e\">{v0}</text>\n",
                    x(t0),
                    x(*t),
                    (x(t0) + x(*t)) / 2.0,
                    GAIN_Y - 10.0
                );
            }
            previous = Some((*t, value));
        }
    }
    if let Some((t0, v0)) = previous {
        svg += &format!(
            "<line x1=\"{}\" y1=\"{GAIN_Y}\" x2=\"{RIGHT}\" y2=\"{GAIN_Y}\" stroke=\"#b03a2e\" stroke-width=\"3\"/>\n\
             <text x=\"{}\" y=\"{}\" text-anchor=\"middle\" fill=\"#b03a2e\">{v0}</text>\n",
            x(t0),
            (x(t0) + RIGHT) / 2.0,
            GAIN_Y - 10.0
        );
    }
    for step in &steps {
        match step {
            Step::Host(t, what) => {
                svg += &format!(
                    "<circle cx=\"{0}\" cy=\"{HOST_Y}\" r=\"5\" fill=\"#222\"/>\n\
                     <text x=\"{0}\" y=\"{1}\" text-anchor=\"middle\" fill=\"#222\">{2}</text>\n\
                     <line x1=\"{0}\" y1=\"{3}\" x2=\"{0}\" y2=\"{4}\" stroke=\"#222\" stroke-dasharray=\"2 3\"/>\n",
                    x(*t),
                    HOST_Y - 14.0,
                    escape_xml(what),
                    HOST_Y + 6.0,
                    GAIN_Y - 6.0
                );
            }
            Step::Update(t, value) => {
                svg += &format!(
                    "<line x1=\"{0}\" y1=\"{1}\" x2=\"{0}\" y2=\"{2}\" stroke=\"#1f5f99\" stroke-width=\"2\" \
                     marker-end=\"url(#obs-arrow)\"/>\n\
                     <circle cx=\"{0}\" cy=\"{UPDATE_Y}\" r=\"6\" fill=\"#1f5f99\"/>\n\
                     <text x=\"{3}\" y=\"{4}\" fill=\"#1f5f99\">gain {value}</text>\n",
                    x(*t),
                    GAIN_Y + 4.0,
                    UPDATE_Y - 8.0,
                    x(*t) + 10.0,
                    UPDATE_Y + 20.0
                );
            }
        }
    }
    svg + "</svg>\n"
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The thermometer's API as a tree, from the introspection data the simulated device sends: every item with the
/// index chain a request to it carries; of an array of traits, the first element in full.
#[tokio::test(start_paused = true)]
async fn book_output_channel_tree() {
    with_dyn_device("thermometer/book_tree", |dev| async move {
        let bundle = dev
            .device_introspect()
            .expect("introspection data")
            .api_bundle
            .clone();
        let root = wire_weaver_client::DynResource::root(bundle);
        let mut rows = vec![Row {
            depth: 0,
            label: format!("{} (API root)", trait_name(&root)),
            chain: String::new(),
        }];
        tree_rows(&dev, &root, 1, &[], &mut rows).await;
        book_outputs::check("thermometer/channel_tree.svg", &tree_svg(&rows));
    })
    .await;
}

/// One line of the tree: how deep, what, and the index chain a request carries to it.
struct Row {
    depth: usize,
    label: String,
    chain: String,
}

fn trait_name(level: &wire_weaver_client::DynResource) -> String {
    match level.kind() {
        Ok(wire_weaver_client::DynResourceKind::Level(l)) => l.trait_name.clone(),
        _ => String::from("?"),
    }
}

fn chain_text(chain: &[String]) -> String {
    format!("[{}]", chain.join(", "))
}

fn tree_rows<'a>(
    dev: &'a DynClient,
    level: &'a wire_weaver_client::DynResource,
    depth: usize,
    prefix: &'a [String],
    rows: &'a mut Vec<Row>,
) -> std::pin::Pin<Box<dyn Future<Output = ()> + 'a>> {
    use wire_weaver_client::ww_self::{ApiItemKindOwned, Multiplicity};
    Box::pin(async move {
        for item in level.children() {
            let mut chain = prefix.to_vec();
            chain.push(item.id.0.to_string());
            let name = &item.ident;
            let kind = match &item.kind {
                ApiItemKindOwned::Method { .. } => "method",
                ApiItemKindOwned::Property { .. } => "property",
                ApiItemKindOwned::Stream { is_up: true, .. }
                | ApiItemKindOwned::StreamFn { .. } => "stream",
                ApiItemKindOwned::Stream { .. } | ApiItemKindOwned::FlowSink { .. } => "sink",
                _ => "trait",
            };
            if kind != "trait" {
                rows.push(Row {
                    depth,
                    label: format!("{name}: {kind}"),
                    chain: chain_text(&chain),
                });
                continue;
            }
            let child = level.child(name).expect("a child the level lists");
            if matches!(item.multiplicity, Multiplicity::Array { .. }) {
                let valid: Vec<u32> = child
                    .valid_indices(dev.cmd())
                    .await
                    .expect("the device lists its indices")
                    .iter()
                    .collect();
                let first = child.index(valid[0]).expect("a valid index");
                rows.push(Row {
                    depth,
                    label: format!("{name}[]: array of {}", trait_name(&first)),
                    chain: chain_text(&chain),
                });
                let mut element = chain.clone();
                element.push(valid[0].to_string());
                rows.push(Row {
                    depth: depth + 1,
                    label: format!("{name}[{}]", valid[0]),
                    chain: chain_text(&element),
                });
                tree_rows(dev, &first, depth + 2, &element, rows).await;
                if let (Some(second), Some(last)) = (valid.get(1), valid.last()) {
                    let mut rest = chain.clone();
                    rest.push(format!("{second}..={last}"));
                    rows.push(Row {
                        depth: depth + 1,
                        label: format!("{name}[{second}] to {name}[{last}]: the same items"),
                        chain: chain_text(&rest),
                    });
                }
            } else {
                rows.push(Row {
                    depth,
                    label: format!("{name}: {}", trait_name(&child)),
                    chain: chain_text(&chain),
                });
                tree_rows(dev, &child, depth + 1, &chain, rows).await;
            }
        }
    })
}

/// The rows as an SVG tree: names indented by depth, with elbows to the parent, and the index chains in a column.
fn tree_svg(rows: &[Row]) -> String {
    const ROW: usize = 26;
    const INDENT: usize = 26;
    const TOP: usize = 40;
    const CHAIN_X: usize = 470;
    let height = TOP + rows.len() * ROW + 10;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"640\" height=\"{height}\" viewBox=\"0 0 640 {height}\" \
         font-family=\"Helvetica, Arial, sans-serif\" font-size=\"13\">\n\
         <title>The thermometer's API as a tree</title>\n\
         <desc>Every item of the thermometer's API with the index chain a request to it carries. The array \
         channel holds one Channel per input; the first is shown in full.</desc>\n\
         <rect x=\"0\" y=\"0\" width=\"640\" height=\"{height}\" rx=\"10\" fill=\"#ffffff\"/>\n\
         <text x=\"20\" y=\"24\" font-weight=\"bold\" fill=\"#222\">Item</text>\n\
         <text x=\"{CHAIN_X}\" y=\"24\" font-weight=\"bold\" fill=\"#222\">Index chain</text>\n"
    );
    for (i, row) in rows.iter().enumerate() {
        let y = TOP + i * ROW + 14;
        let x = 20 + row.depth * INDENT;
        if row.depth > 0 {
            // the elbow from the parent row's column down to this row
            let parent = rows[..i]
                .iter()
                .rposition(|r| r.depth < row.depth)
                .unwrap_or(0);
            let px = 20 + rows[parent].depth * INDENT + 6;
            let py = TOP + parent * ROW + 18;
            svg += &format!(
                "<path d=\"M{px},{py} V{} H{}\" fill=\"none\" stroke=\"#999\"/>\n",
                y - 4,
                x - 4
            );
        }
        let (color, weight) = match row.depth {
            0 => ("#222", "bold"),
            _ if row.label.contains("[]") || row.label.contains('[') => ("#b03a2e", "normal"),
            _ => ("#222", "normal"),
        };
        svg += &format!(
            "<text x=\"{x}\" y=\"{y}\" fill=\"{color}\" font-weight=\"{weight}\">{}</text>\n\
             <text x=\"{CHAIN_X}\" y=\"{y}\" fill=\"#1f5f99\" font-family=\"monospace\">{}</text>\n",
            escape_xml(&row.label),
            escape_xml(&row.chain)
        );
    }
    svg + "</svg>\n"
}
