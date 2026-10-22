//! Shared helpers across `ww` subcommands that work on a [DynResource] (`observe`, `stream`).

use anyhow::{Context, Result, anyhow, bail};
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;
use tokio::sync::watch;
use wire_weaver_client::DynResource;

/// `path`, or "API root" when empty, for a message.
pub(crate) fn display_path(path: &str) -> &str {
    if path.is_empty() { "API root" } else { path }
}

/// Walk `path` (e.g. `motor` or `bank[0].level`) from `root` by names and indices, the same syntax
/// `DynResource::path()` prints.
pub(crate) fn resolve(root: DynResource, path: &str) -> Result<DynResource> {
    let mut res = root;
    if path.is_empty() {
        return Ok(res);
    }
    for segment in path.split('.') {
        let name_end = segment.find('[').unwrap_or(segment.len());
        let (name, mut rest) = (&segment[..name_end], &segment[name_end..]);
        if !name.is_empty() {
            res = res.child(name)?;
        }
        while let Some(after_bracket) = rest.strip_prefix('[') {
            let close = after_bracket
                .find(']')
                .ok_or_else(|| anyhow!("'{segment}': missing ']'"))?;
            let index: u32 = after_bracket[..close]
                .parse()
                .with_context(|| format!("'{segment}': expected an integer index"))?;
            res = res.index(index)?;
            rest = &after_bracket[close + 1..];
        }
        if !rest.is_empty() {
            bail!("'{segment}': unexpected text after the index");
        }
    }
    Ok(res)
}

/// Tells that the process was asked to stop: Ctrl-C (SIGINT) or, on Unix, SIGTERM (`timeout`, a service manager,
/// `kill`). Installed before connecting so no signal is lost on the way, and so the default action (dying at once,
/// with a `--capture` file still `.partial`) is off: whoever holds it ends what it does and cleans up.
#[derive(Clone)]
pub(crate) struct StopSignal(watch::Receiver<bool>, Arc<watch::Sender<bool>>);

impl StopSignal {
    /// Must be called inside the tokio runtime.
    pub(crate) fn install() -> Self {
        let (tx, rx) = watch::channel(false);
        let tx = Arc::new(tx);
        let signals = Signals::register();
        let on_signal = tx.clone();
        tokio::spawn(async move {
            signals.wait().await;
            _ = on_signal.send(true);
        });
        StopSignal(rx, tx)
    }

    /// Asks for a stop, as a signal does: `q` typed in a live view ([QuitKey]).
    pub(crate) fn stop(&self) {
        _ = self.1.send(true);
    }

    /// Resolves once a stop was asked for (at once when it already was).
    pub(crate) async fn wait(&mut self) {
        // every clone holds a sender: an error would mean the runtime is going away
        _ = self.0.wait_for(|stopped| *stopped).await;
    }

    /// Runs `fut`, or gives up on it when a stop is asked for: `None` then.
    pub(crate) async fn guard<T>(&mut self, fut: impl Future<Output = T>) -> Option<T> {
        tokio::select! {
            r = fut => Some(r),
            _ = self.wait() => None,
        }
    }
}

#[cfg(unix)]
struct Signals {
    interrupt: Option<tokio::signal::unix::Signal>,
    terminate: Option<tokio::signal::unix::Signal>,
}

#[cfg(unix)]
impl Signals {
    fn register() -> Self {
        use tokio::signal::unix::{SignalKind, signal};
        Signals {
            interrupt: signal(SignalKind::interrupt()).ok(),
            terminate: signal(SignalKind::terminate()).ok(),
        }
    }

    async fn wait(mut self) {
        async fn one(s: &mut Option<tokio::signal::unix::Signal>) {
            match s {
                Some(s) => _ = s.recv().await,
                None => std::future::pending().await,
            }
        }
        tokio::select! {
            _ = one(&mut self.interrupt) => {}
            _ = one(&mut self.terminate) => {}
        }
    }
}

#[cfg(not(unix))]
struct Signals;

#[cfg(not(unix))]
impl Signals {
    fn register() -> Self {
        Signals
    }

    async fn wait(self) {
        _ = tokio::signal::ctrl_c().await;
    }
}

/// [StopSignal::guard] when there is a signal to watch, else just `fut`.
pub(crate) async fn guard<T>(stop: Option<StopSignal>, fut: impl Future<Output = T>) -> Option<T> {
    match stop {
        Some(mut stop) => stop.guard(fut).await,
        None => Some(fut.await),
    }
}

/// `q` typed in the terminal asks the [StopSignal] for a stop, beside Ctrl+C: for live views that run until stopped
/// (`ww sync`). Keys are read on a thread of their own, without echo or waiting for Enter; output, Ctrl+C and the
/// other control keys work as before, and the terminal is put back as it was on drop.
pub(crate) struct QuitKey {
    done: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    #[cfg(unix)]
    saved: rustix::termios::Termios,
}

impl QuitKey {
    /// Listens while it lives; `None` when stdin is not a terminal (or on a platform without it).
    pub(crate) fn listen(stop: &StopSignal) -> Option<Self> {
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            return None;
        }
        let done = Arc::new(AtomicBool::new(false));
        #[cfg(unix)]
        let saved = {
            use rustix::termios::{
                LocalModes, OptionalActions, SpecialCodeIndex, tcgetattr, tcsetattr,
            };
            let stdin = std::io::stdin();
            let saved = tcgetattr(&stdin).ok()?;
            let mut keys = saved.clone();
            keys.local_modes -= LocalModes::ICANON | LocalModes::ECHO;
            // a read returns after a key or 100 ms without one, so the thread sees `done`
            keys.special_codes[SpecialCodeIndex::VMIN] = 0;
            keys.special_codes[SpecialCodeIndex::VTIME] = 1;
            tcsetattr(&stdin, OptionalActions::Now, &keys).ok()?;
            saved
        };
        let thread = {
            let (done, stop) = (done.clone(), stop.clone());
            std::thread::spawn(move || {
                while !done.load(Ordering::Relaxed) {
                    if matches!(read_key(), Some('q' | 'Q')) {
                        stop.stop();
                        return;
                    }
                }
            })
        };
        Some(QuitKey {
            done,
            thread: Some(thread),
            #[cfg(unix)]
            saved,
        })
    }
}

impl Drop for QuitKey {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            _ = thread.join();
        }
        #[cfg(unix)]
        {
            use rustix::termios::{OptionalActions, tcsetattr};
            _ = tcsetattr(std::io::stdin(), OptionalActions::Now, &self.saved);
        }
    }
}

/// A key typed, or `None` after about 100 ms without one.
#[cfg(unix)]
fn read_key() -> Option<char> {
    let mut byte = [0u8];
    match rustix::io::read(std::io::stdin(), &mut byte) {
        Ok(1) => Some(byte[0] as char),
        // no key: VTIME ran out; an error (stdin closed): don't spin
        Ok(_) => None,
        Err(_) => {
            std::thread::sleep(Duration::from_millis(100));
            None
        }
    }
}

/// A key typed, or `None` after about 100 ms without one.
#[cfg(windows)]
fn read_key() -> Option<char> {
    use crossterm::event::{Event, KeyCode, KeyEventKind, poll, read};
    match poll(Duration::from_millis(100)) {
        Ok(true) => match read() {
            Ok(Event::Key(k)) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Char(c) => Some(c),
                _ => None,
            },
            _ => None,
        },
        Ok(false) => None,
        Err(_) => {
            std::thread::sleep(Duration::from_millis(100));
            None
        }
    }
}

/// A key typed, or `None` after about 100 ms without one.
#[cfg(not(any(unix, windows)))]
fn read_key() -> Option<char> {
    std::thread::sleep(Duration::from_millis(100));
    None
}
