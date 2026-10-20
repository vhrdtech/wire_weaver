//! Shared helpers across `ww` subcommands that work on a [DynResource] (`observe`, `stream`).

use anyhow::{Context, Result, anyhow, bail};
use std::future::Future;
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
pub(crate) struct StopSignal(watch::Receiver<bool>);

impl StopSignal {
    /// Must be called inside the tokio runtime.
    pub(crate) fn install() -> Self {
        let (tx, rx) = watch::channel(false);
        let signals = Signals::register();
        tokio::spawn(async move {
            signals.wait().await;
            _ = tx.send(true);
        });
        StopSignal(rx)
    }

    /// Resolves once a stop was asked for (at once when it already was).
    pub(crate) async fn wait(&mut self) {
        // the sender lives in the task above until it has sent: an error would mean the runtime is going away
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
