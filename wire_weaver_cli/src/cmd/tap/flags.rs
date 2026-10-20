//! `--monitor` and `--capture`, global flags of every command that opens a device connection: the TR-14 tap on that
//! command's own client, printed to stderr (stdout stays the command's) and/or saved to a file. Same [Monitor],
//! [LiveFeed] and [TapeWriter] as `ww monitor` and `ww capture`, run on a thread beside the command.

use super::*;
use std::thread::JoinHandle;

#[derive(Args, Clone, Default)]
#[command(next_help_heading = "Link tap (commands that connect to a device)")]
pub(crate) struct TapFlags {
    /// Print this command's own link traffic to stderr, as `ww monitor` prints it (default layer: link)
    ///
    /// Works on every command that connects: `ww -l blinky --monitor=api info` shows the requests it made and the
    /// replies paired with them, without a ww server. The value needs `=`.
    #[arg(
        long,
        global = true,
        value_enum,
        value_name = "LAYER",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "link"
    )]
    pub monitor: Option<Layer>,

    /// With --monitor: also print the bytes of every frame and message, in hex
    #[arg(long = "monitor-bytes", global = true)]
    pub tap_bytes: bool,

    /// With --monitor=api: every message with its bit map and field table
    #[arg(long = "monitor-bits", global = true)]
    pub tap_bits: bool,

    /// Save this command's own link traffic to FILE, to read with `ww explain FILE` (a new file, written as
    /// FILE.partial and renamed when done; `ww capture --append` adds to one)
    #[arg(long, global = true, value_name = "FILE", value_hint = ValueHint::FilePath)]
    pub capture: Option<PathBuf>,
}

impl TapFlags {
    pub(crate) fn active(&self) -> bool {
        self.monitor.is_some() || self.capture.is_some()
    }

    /// `--bytes`/`--bits` only mean something printed.
    pub(crate) fn check(&self) -> Result<()> {
        if self.monitor.is_none() && (self.tap_bytes || self.tap_bits) {
            return Err(anyhow!(
                "--monitor-bytes and --monitor-bits go with --monitor"
            ));
        }
        Ok(())
    }

    /// What the recorder gives: everything for a capture (frames included), else what the print needs.
    fn layer(&self) -> Layer {
        if self.capture.is_some() {
            Layer::Frame
        } else {
            self.monitor.unwrap_or_default()
        }
    }
}

/// What a finished tap did, for the one line after the command.
pub(crate) struct Summary {
    capture: Option<(PathBuf, u64, u64)>,
}

/// The running tap of one command: its recorder (for the client's config) and the thread printing and writing.
pub(crate) struct Attached {
    recorder: Recorder,
    stop: Arc<AtomicBool>,
    thread: JoinHandle<Result<Summary>>,
}

impl Attached {
    /// Monitor output to stderr.
    pub(crate) fn start(flags: &TapFlags) -> Result<Self> {
        Self::start_with(flags, Box::new(std::io::stderr()))
    }

    /// Opens the capture file first, so a refusal comes before any connecting.
    pub(crate) fn start_with(flags: &TapFlags, out: Box<dyn Write + Send>) -> Result<Self> {
        flags.check()?;
        let writer = flags
            .capture
            .as_ref()
            .map(|path| {
                TapeWriter::create(path, SyncPolicy::default()).map_err(|e| match e {
                    WriteError::Exists(p) => anyhow!(
                        "{} exists: another name for --capture, or `ww capture --append` to add to it",
                        p.display()
                    ),
                    e => anyhow!("{}: {e}", path.display()),
                })
            })
            .transpose()?;
        let view = flags.monitor.map(|layer| ViewArgs {
            layer,
            bytes: flags.tap_bytes,
            bits: flags.tap_bits,
        });
        let frames = flags.layer() == Layer::Frame;
        let (mut feed, recorder) = LiveFeed::new(LiveTap::new(flags.layer()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = std::thread::spawn({
            let stop = stop.clone();
            move || run(&mut feed, &stop, view, frames, writer, out)
        });
        Ok(Attached {
            recorder,
            stop,
            thread,
        })
    }

    pub(crate) fn recorder(&self) -> Recorder {
        self.recorder.clone()
    }

    /// The API bundles a capture needs, once connected.
    pub(crate) fn add_bundles(&self, device: &DynClient) {
        add_bundles(&self.recorder, device);
    }

    /// After the connection ended: let the thread drain what was recorded, print the capture line to stderr.
    pub(crate) fn finish(self) -> Result<()> {
        self.recorder.flush();
        self.stop.store(true, Ordering::Relaxed);
        let summary = self
            .thread
            .join()
            .map_err(|_| anyhow!("the tap thread panicked"))??;
        if let Some((path, blocks, bytes)) = summary.capture {
            eprintln!(
                "{} {blocks} blocks, {} to {}",
                style("captured").for_stderr().green(),
                bytes.human_count_bytes(),
                path.display()
            );
        }
        Ok(())
    }
}

fn run(
    feed: &mut LiveFeed,
    stop: &AtomicBool,
    view: Option<ViewArgs>,
    frames: bool,
    mut writer: Option<TapeWriter>,
    mut out: Box<dyn Write + Send>,
) -> Result<Summary> {
    // colours follow the stream the text goes to, not stdout
    let mut monitor = view.map(|v| Monitor::with_frames(v, false, frames).for_stderr());
    // a failed write ends the capture: the file stays as it is (its valid prefix reads)
    let result = consume_idle(feed, stop, |b| {
        let Some(b) = b else {
            if let Some(w) = &mut writer {
                w.tick()?;
            }
            return Ok(());
        };
        if let Some(w) = &mut writer {
            w.write_block(b.bytes())?;
        }
        if let Some(m) = &mut monitor {
            m.block(b, &mut out)?;
        }
        Ok(())
    });
    if let Some(m) = &mut monitor {
        m.finish(&mut out)?;
    }
    let Some(writer) = writer else {
        result?;
        return Ok(Summary { capture: None });
    };
    let (blocks, bytes) = (writer.blocks(), writer.bytes());
    let current = writer.current_path().to_path_buf();
    result.with_context(|| format!("capture stopped, {} kept as it is", current.display()))?;
    let path = writer.finish()?;
    Ok(Summary {
        capture: Some((path, blocks, bytes)),
    })
}
