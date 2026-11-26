//! The book's wire-size chart: one bar per format, its size relative to the smallest encoding of each message,
//! grouped by how the format evolves.

use crate::rows::{Evolution, Message};
use std::fmt::Write as _;

const WIDTH: u32 = 720;
/// Left edge of the bars; the format names sit before it.
const BAR_X: u32 = 220;
/// Longest bar, for the largest ratio.
const BAR_WIDTH_MAX: u32 = 410;
const ROW_HEIGHT: u32 = 22;
const BAR_HEIGHT: u32 = 14;
const GROUP_HEAD: u32 = 30;
const TOP: u32 = 56;
const BOTTOM: u32 = 44;

/// The four groups in chart order, each with its colour (the default categorical slots 1 to 4) and heading.
const GROUPS: [(Evolution, &str, &str); 4] = [
    (
        Evolution::SchemaRules,
        "#257de0",
        "Schema rules: a later revision adds fields",
    ),
    (
        Evolution::SelfDescribing,
        "#f47230",
        "Self-describing: names travel with the data",
    ),
    (
        Evolution::ExtensionMarkers,
        "#00b894",
        "Extension markers: room reserved for later fields",
    ),
    (
        Evolution::None,
        "#f2a600",
        "None: any change breaks old readers",
    ),
];

/// One format's mean size over the messages, as a multiple of each message's smallest encoding.
struct Bar {
    format: &'static str,
    evolution: Evolution,
    /// Thousandths of the smallest encoding, so the text rounds the same on every machine
    ratio_milli: u64,
}

fn bars(messages: &[Message]) -> Vec<Bar> {
    let first = &messages[0];
    (0..first.rows.len())
        .map(|i| {
            let sum_milli: u64 = messages
                .iter()
                .map(|message| {
                    let smallest = message
                        .rows
                        .iter()
                        .map(|row| row.bytes.len())
                        .min()
                        .expect("every message has rows") as u64;
                    message.rows[i].bytes.len() as u64 * 1000 / smallest
                })
                .sum();
            Bar {
                format: first.rows[i].format,
                evolution: first.rows[i].evolution,
                ratio_milli: sum_milli / messages.len() as u64,
            }
        })
        .collect()
}

/// A ratio in thousandths, rounded half up to hundredths.
fn hundredths(milli: u64) -> u64 {
    (milli + 5) / 10
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;")
}

/// The chart as a standalone SVG.
pub fn svg(messages: &[Message]) -> String {
    let bars = bars(messages);
    let max_milli = bars
        .iter()
        .map(|bar| bar.ratio_milli)
        .max()
        .expect("there are formats");
    let height = TOP + GROUPS.len() as u32 * GROUP_HEAD + bars.len() as u32 * ROW_HEIGHT + BOTTOM;
    let scale = |milli: u64| (milli * u64::from(BAR_WIDTH_MAX) / max_milli) as u32;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH}\" height=\"{height}\" \
         viewBox=\"0 0 {WIDTH} {height}\" font-family=\"Helvetica, Arial, sans-serif\" font-size=\"13\">\n\
         <title>Wire size of each format relative to the smallest encoding</title>\n\
         <desc>For each of {} formats, the mean over {} messages of its size divided by the smallest encoding \
         of that message; 1.00 means always the smallest. The bars are grouped by how the format lets a later \
         revision add a field.</desc>\n\
         <rect x=\"0\" y=\"0\" width=\"{WIDTH}\" height=\"{height}\" rx=\"10\" fill=\"#ffffff\"/>\n\
         <text x=\"20\" y=\"30\" font-weight=\"bold\" fill=\"#222\">Size relative to the smallest encoding, \
         mean over {} messages</text>\n",
        bars.len(),
        messages.len(),
        messages.len()
    );
    let one_x = BAR_X + scale(1000);
    let mut y = TOP;
    for (evolution, colour, heading) in GROUPS {
        let mut group: Vec<&Bar> = bars
            .iter()
            .filter(|bar| bar.evolution.as_str() == evolution.as_str())
            .collect();
        group.sort_by_key(|bar| bar.ratio_milli);
        writeln!(
            svg,
            "<rect x=\"20\" y=\"{}\" width=\"12\" height=\"12\" rx=\"2\" fill=\"{colour}\"/>\n\
             <text x=\"40\" y=\"{}\" font-weight=\"bold\" fill=\"#222\">{}</text>",
            y + 10,
            y + 21,
            escape(heading)
        )
        .expect("writing to a String");
        y += GROUP_HEAD;
        let rows_bottom = y + group.len() as u32 * ROW_HEIGHT - (ROW_HEIGHT - BAR_HEIGHT);
        writeln!(
            svg,
            "<line x1=\"{BAR_X}\" y1=\"{}\" x2=\"{BAR_X}\" y2=\"{}\" stroke=\"#999\"/>\n\
             <line x1=\"{one_x}\" y1=\"{}\" x2=\"{one_x}\" y2=\"{}\" stroke=\"#999\" stroke-dasharray=\"4 4\"/>",
            y - 4,
            rows_bottom + 4,
            y - 4,
            rows_bottom + 4
        )
        .expect("writing to a String");
        for bar in group {
            let width = scale(bar.ratio_milli).max(2);
            let text_y = y + BAR_HEIGHT - 2;
            writeln!(
                svg,
                "<text x=\"{}\" y=\"{text_y}\" text-anchor=\"end\" fill=\"#222\">{}</text>\n\
                 <rect x=\"{BAR_X}\" y=\"{y}\" width=\"{width}\" height=\"{BAR_HEIGHT}\" rx=\"3\" fill=\"{colour}\"/>\n\
                 <text x=\"{}\" y=\"{text_y}\" fill=\"#222\">{}.{:02}</text>",
                BAR_X - 10,
                escape(bar.format),
                BAR_X + width + 6,
                hundredths(bar.ratio_milli) / 100,
                hundredths(bar.ratio_milli) % 100,
            )
            .expect("writing to a String");
            y += ROW_HEIGHT;
        }
    }
    writeln!(
        svg,
        "<text x=\"{one_x}\" y=\"{}\" text-anchor=\"middle\" fill=\"#555\">1.00: the smallest</text>",
        y + 20
    )
    .expect("writing to a String");
    svg + "</svg>\n"
}
