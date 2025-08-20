use std::time::Duration;

#[cfg(not(any(
    feature = "default-timeout-100ms",
    feature = "default-timeout-250ms",
    feature = "default-timeout-1s"
)))]
compile_error!("Select one of the default-timeout-x features");

#[cfg(all(
    feature = "default-timeout-100ms",
    any(feature = "default-timeout-250ms", feature = "default-timeout-1s")
))]
compile_error!("Select only one of the default-timeout-x features");
#[cfg(all(
    feature = "default-timeout-250ms",
    any(feature = "default-timeout-100ms", feature = "default-timeout-1s")
))]
compile_error!("Select only one of the default-timeout-x features");
#[cfg(all(
    feature = "default-timeout-1s",
    any(feature = "default-timeout-100ms", feature = "default-timeout-250ms")
))]
compile_error!("Select only one of the default-timeout-x features");

#[cfg(feature = "default-timeout-100ms")]
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(100);
#[cfg(feature = "default-timeout-250ms")]
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(250);
#[cfg(feature = "default-timeout-1s")]
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(1_000);

#[derive(Copy, Clone)]
pub enum Timeout {
    /// Default timeout set via feature flags of wire_weaver_client_server crate.
    Default,
    /// Specified timeout in milliseconds.
    Millis(u64),
}

impl Timeout {
    pub(crate) fn timeout(&self) -> Duration {
        match self {
            Timeout::Default => DEFAULT_TIMEOUT,
            Timeout::Millis(millis) => Duration::from_millis(*millis),
        }
    }
}
