//! What the "Getting connected" chapters show, checked against `docs/generated/thermometer/`: one call over two
//! media, and a host that keeps trying while the device is unplugged, reset and unplugged again, on tokio's paused
//! clock.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::rc::Rc;
use std::time::Duration;

use thermometer::Thermometer;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep};
use wire_weaver_client::{ConnectionEvent, Error, KeepTrying, in_process};

/// A simulated thermometer that can be plugged in and out: its stop signal and its task.
struct Plug {
    stop: oneshot::Sender<()>,
    device: JoinHandle<thermometer::Counters>,
}

impl Plug {
    /// Registers a device under `path`, as plugging a board in does.
    fn insert(path: &str) -> Plug {
        let (stop, stop_rx) = oneshot::channel::<()>();
        let incoming = in_process::ws_device(path);
        let device = tokio::task::spawn_local(thermometer::simulated(incoming, async {
            _ = stop_rx.await;
        }));
        Plug { stop, device }
    }

    /// Stops the device and removes it, as unplugging does.
    async fn remove(self) {
        _ = self.stop.send(());
        self.device.await.expect("the device runs to its stop");
    }
}

/// The text a host program prints for one sample.
fn line(sample: &thermometer_api::Sample) -> String {
    format!("input {}: {}", sample.input.get(), sample.temp)
}

/// The same call over the in-process WebSocket and over the in-process UDP socket: only the config differs.
#[tokio::test(start_paused = true)]
async fn book_output_connect() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            for (transport, path) in [
                ("websocket", "thermometer/connected_ws"),
                ("udp", "thermometer/connected_udp"),
            ] {
                let (stop, stop_rx) = oneshot::channel::<()>();
                let device = if transport == "udp" {
                    let socket = in_process::udp_device(path);
                    tokio::task::spawn_local(thermometer::simulated_udp(socket, async {
                        _ = stop_rx.await;
                    }))
                } else {
                    let incoming = in_process::ws_device(path);
                    tokio::task::spawn_local(thermometer::simulated(incoming, async {
                        _ = stop_rx.await;
                    }))
                };
                let host = Thermometer::config(|c| c.in_process_path(path.to_owned()).no_ww_usb())
                    .connect()
                    .await
                    .expect("connects");
                let sample = host.read_once().call().await.expect("reads");
                let out = format!(
                    "$ cargo run -p thermometer --example thermometer_connect -- {transport}\n{}\n",
                    line(&sample)
                );
                book_outputs::check(&format!("thermometer/connect_{transport}.txt"), &out);
                _ = stop.send(());
                device.await.expect("the device runs to its stop");
            }
        })
        .await;
}

/// What the figure and the transcript of "When the device goes away" show.
#[derive(Clone, Debug, PartialEq)]
enum Mark {
    /// The board is plugged in or out, or reset
    Plugged(&'static str),
    Connecting(u32),
    Failed {
        attempt: u32,
        retry_in: Duration,
    },
    Connected,
    Lost,
    Reconnected {
        attempts: u32,
    },
    /// A call made while there is no link, refused at once
    CallRefused,
}

type Log = Rc<RefCell<Vec<(Duration, Mark)>>>;

fn note(log: &Log, since: Instant, mark: Mark) {
    log.borrow_mut().push((since.elapsed(), mark));
}

/// One host that keeps trying, against a board plugged in late, reset and unplugged: returns what happened when.
async fn keep_trying_story() -> Vec<(Duration, Mark)> {
    const PATH: &str = "thermometer/keep_trying";
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let since = Instant::now();
            let log: Log = Rc::default();
            // --8<-- [start:keep_trying]
            let device = Thermometer::config(|c| {
                c.in_process_path(PATH.to_owned())
                    .no_ww_usb()
                    .keep_trying(KeepTrying::default())
            })
            .connect_keep_trying()
            .expect("the handle is made at once");
            let mut events = device.cmd().connection_events();
            // --8<-- [end:keep_trying]

            let recorder = {
                let log = log.clone();
                tokio::task::spawn_local(async move {
                    while let Ok(event) = events.recv().await {
                        let mark = match event {
                            ConnectionEvent::Connecting { attempt } => Mark::Connecting(attempt),
                            ConnectionEvent::AttemptFailed {
                                attempt, retry_in, ..
                            } => Mark::Failed { attempt, retry_in },
                            ConnectionEvent::Connected { .. } => Mark::Connected,
                            ConnectionEvent::Disconnected { .. } => Mark::Lost,
                            ConnectionEvent::Reconnected { attempts, .. } => {
                                Mark::Reconnected { attempts }
                            }
                            other => panic!("unexpected {other:?}"),
                        };
                        note(&log, since, mark);
                    }
                })
            };

            sleep(Duration::from_millis(350)).await;
            note(&log, since, Mark::Plugged("plugged in"));
            let plug = Plug::insert(PATH);
            sleep(Duration::from_millis(1650)).await;

            // a reset: the board is back after 250 ms
            note(&log, since, Mark::Plugged("reset"));
            plug.remove().await;
            sleep(Duration::from_millis(250)).await;
            note(&log, since, Mark::Plugged("plugged in"));
            let plug = Plug::insert(PATH);
            sleep(Duration::from_millis(2750)).await;

            // unplugged for 3 s
            note(&log, since, Mark::Plugged("unplugged"));
            plug.remove().await;
            sleep(Duration::from_millis(500)).await;
            let refused = device.read_once().call().await;
            assert!(matches!(refused, Err(Error::Disconnected)), "{refused:?}");
            note(&log, since, Mark::CallRefused);
            sleep(Duration::from_millis(2500)).await;
            note(&log, since, Mark::Plugged("plugged in"));
            let plug = Plug::insert(PATH);
            sleep(Duration::from_millis(3000)).await;

            device
                .cmd()
                .disconnect()
                .asynch()
                .await
                .expect("disconnect");
            recorder.abort();
            plug.remove().await;
            log.take()
        })
        .await
}

fn ms(at: Duration) -> u128 {
    at.as_millis()
}

/// The story as the transcript the page shows.
fn transcript(story: &[(Duration, Mark)]) -> String {
    let mut out = String::new();
    for (at, mark) in story {
        let what = match mark {
            Mark::Plugged(what) => format!("the board is {what}"),
            Mark::Connecting(_) => continue,
            Mark::Failed { attempt, retry_in } => {
                format!(
                    "attempt {attempt} failed, next in {} ms",
                    retry_in.as_millis()
                )
            }
            Mark::Connected => "connected".to_owned(),
            Mark::Lost => "link lost".to_owned(),
            Mark::Reconnected { attempts } => format!("connected again, after {attempts} attempts"),
            Mark::CallRefused => "a call: refused at once, Disconnected".to_owned(),
        };
        writeln!(out, "{:>6} ms  {what}", ms(*at)).expect("a string");
    }
    out
}

/// The story as a timeline: the board's presence, the link, and the attempts between.
fn timeline_svg(story: &[(Duration, Mark)]) -> String {
    const LEFT: f64 = 80.0;
    const PER_MS: f64 = 0.075;
    const WIDTH: f64 = 740.0;
    let x = |at: Duration| LEFT + at.as_millis() as f64 * PER_MS;
    // the axis runs a little past the last event
    let end = story.last().map_or(Duration::ZERO, |(at, _)| *at) + Duration::from_millis(500);

    // intervals of the board's presence and of the link
    let mut board: Vec<(Duration, Duration)> = vec![];
    let mut link: Vec<(Duration, Duration)> = vec![];
    let mut board_from = None;
    let mut link_from = None;
    for (at, mark) in story {
        match mark {
            Mark::Plugged("plugged in") => board_from = Some(*at),
            Mark::Plugged(_) => {
                if let Some(from) = board_from.take() {
                    board.push((from, *at));
                }
            }
            Mark::Connected | Mark::Reconnected { .. } => link_from = Some(*at),
            Mark::Lost => {
                if let Some(from) = link_from.take() {
                    link.push((from, *at));
                }
            }
            _ => {}
        }
    }
    if let Some(from) = board_from {
        board.push((from, end));
    }
    if let Some(from) = link_from {
        link.push((from, end));
    }

    let mut svg = String::new();
    let height = 285;
    writeln!(
        svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{height}" viewBox="0 0 {WIDTH} {height}" style="max-width: 100%; height: auto" font-family="Helvetica, Arial, sans-serif" font-size="13">
  <title>A host that keeps trying while a board is plugged in late, reset and unplugged</title>
  <desc>Three rows on a time axis. The board row is filled while the board is plugged in. The link row is filled while the host is connected. The attempts row has a mark for each connection attempt: red for a failed one, green for the one that connected. The waits between attempts double, from 100 ms up to 5 s, and start again at 100 ms after each connection.</desc>
  <rect width="{WIDTH}" height="{height}" rx="10" fill="#ffffff"/>
  <g fill="#222"><text x="10" y="64">board</text><text x="10" y="124">link</text><text x="10" y="184">attempts</text></g>"##
    )
    .expect("a string");
    for (row, intervals, colour) in [(45, &board, "#9fb8d6"), (105, &link, "#8cc9a0")] {
        writeln!(svg, r##"  <rect x="{LEFT}" y="{row}" width="{}" height="28" fill="#f2f4f7" stroke="#bbb"/>"##, end.as_millis() as f64 * PER_MS).expect("a string");
        for (from, to) in intervals {
            writeln!(
                svg,
                r##"  <rect x="{:.1}" y="{row}" width="{:.1}" height="28" fill="{colour}" stroke="#222"/>"##,
                x(*from),
                (x(*to) - x(*from)).max(1.0)
            )
            .expect("a string");
        }
    }
    for (at, mark) in story {
        let (colour, mark_x) = match mark {
            Mark::Failed { .. } => ("#b03a2e", x(*at)),
            Mark::Connected | Mark::Reconnected { .. } => ("#2e7d46", x(*at)),
            _ => continue,
        };
        writeln!(svg, r##"  <line x1="{mark_x:.1}" y1="165" x2="{mark_x:.1}" y2="195" stroke="{colour}" stroke-width="3"/>"##).expect("a string");
    }
    // the time axis, a tick for each second
    writeln!(
        svg,
        r##"  <line x1="{LEFT}" y1="205" x2="{:.1}" y2="205" stroke="#666"/>"##,
        x(end)
    )
    .expect("a string");
    for second in 0..=end.as_secs() {
        let tick_x = x(Duration::from_secs(second));
        writeln!(
            svg,
            r##"  <line x1="{tick_x:.1}" y1="205" x2="{tick_x:.1}" y2="211" stroke="#666"/>
  <text x="{tick_x:.1}" y="225" text-anchor="middle" fill="#555" font-size="11">{second} s</text>"##
        )
        .expect("a string");
    }
    // the events of the board, labelled under the axis
    let mut label_row = 0;
    for (at, mark) in story {
        if let Mark::Plugged(what) = mark {
            let text_x = x(*at);
            writeln!(
                svg,
                r##"  <line x1="{text_x:.1}" y1="35" x2="{text_x:.1}" y2="205" stroke="#666" stroke-dasharray="4 4"/>
  <text x="{text_x:.1}" y="{}" fill="#222" text-anchor="{anchor}">{what}, {} ms</text>"##,
                248 + (label_row % 2) * 16,
                ms(*at),
                anchor = if text_x > WIDTH - 130.0 { "end" } else { "start" }
            )
            .expect("a string");
            label_row += 1;
        }
    }
    writeln!(svg, "</svg>").expect("a string");
    svg
}

#[tokio::test(start_paused = true)]
async fn book_output_keep_trying() {
    let story = keep_trying_story().await;
    book_outputs::check("thermometer/keep_trying.txt", &transcript(&story));
    book_outputs::check("thermometer/keep_trying.svg", &timeline_svg(&story));
}
