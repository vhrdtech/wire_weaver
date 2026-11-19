//! A tap on a client's own connection for tests, printed and saved to a file at once: the same [Monitor], [LiveFeed]
//! and [TapeWriter] as `ww monitor` and `ww capture`, run on a thread beside the client.

use super::*;
use std::thread::JoinHandle;
use wire_weaver_client::ww_tape::TapeWriter;
use wire_weaver_client::ww_tape_fs::{FileSink, writer::create};

/// The running tap of one client: its recorder (for the client's config) and the thread printing and writing.
pub(crate) struct Attached {
    recorder: Recorder,
    stop: Arc<AtomicBool>,
    thread: JoinHandle<Result<PathBuf>>,
}

impl Attached {
    /// Prints `view` to `out` and saves everything, frames included, to a new file at `capture`. `resolver`: the
    /// connection's, the monitor decodes the device's API through it.
    pub(crate) fn start(
        view: ViewArgs,
        capture: &Path,
        out: Box<dyn Write + Send>,
        resolver: &'static Resolver,
    ) -> Result<Self> {
        let mut writer = create(capture, SyncPolicy::default()).map_err(anyhow::Error::new)?;
        let (mut feed, recorder) = LiveFeed::new(LiveTap::new(Layer::Frame));
        writer.set_clock(recorder.tape_clock());
        let stop = Arc::new(AtomicBool::new(false));
        let thread = std::thread::spawn({
            let stop = stop.clone();
            move || run(&mut feed, &stop, view, writer, out, resolver)
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
    pub(crate) fn finish(self) -> Result<PathBuf> {
        self.recorder.flush();
        self.stop.store(true, Ordering::Relaxed);
        self.thread
            .join()
            .map_err(|_| anyhow!("the tap thread panicked"))?
    }
}

fn run(
    feed: &mut LiveFeed,
    stop: &AtomicBool,
    view: ViewArgs,
    mut writer: TapeWriter<FileSink>,
    mut out: Box<dyn Write + Send>,
    resolver: &'static Resolver,
) -> Result<PathBuf> {
    let mut monitor = Monitor::with_frames(view, false, true, resolver);
    writer.set_producer(super::PRODUCER);
    let mut defs = Definitions::new();
    // a failed write ends the capture: the file stays as it is (its valid prefix reads)
    let result = consume_idle(feed, stop, |b| {
        let Some(b) = b else {
            writer.tick()?;
            return Ok(());
        };
        writer.relay_block(&mut defs, b.bytes())?;
        wire_weaver_client::ww_tap::recorder::drop_closed_links(&mut defs, b);
        monitor.block(b, &mut out)?;
        Ok(())
    });
    monitor.finish(&mut out)?;
    let current = writer.sink().current_path().to_path_buf();
    result.with_context(|| format!("capture stopped, {} kept as it is", current.display()))?;
    let path = writer.sink().path().to_path_buf();
    writer.finish()?;
    Ok(path)
}
