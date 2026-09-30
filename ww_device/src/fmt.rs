//! Logging macros forwarding to defmt when the `defmt` feature is enabled, no-op otherwise.
#![allow(unused_macros)]

macro_rules! log_impl {
    ($level:ident, $($arg:expr),+ $(,)?) => {{
        #[cfg(feature = "defmt")]
        defmt::$level!($($arg),+);
        #[cfg(not(feature = "defmt"))]
        {
            let _ = ($(&$arg),+);
        }
    }};
}

macro_rules! trace {
    ($($arg:expr),+ $(,)?) => { log_impl!(trace, $($arg),+) };
}

macro_rules! debug {
    ($($arg:expr),+ $(,)?) => { log_impl!(debug, $($arg),+) };
}

macro_rules! info {
    ($($arg:expr),+ $(,)?) => { log_impl!(info, $($arg),+) };
}

macro_rules! warn {
    ($($arg:expr),+ $(,)?) => { log_impl!(warn, $($arg),+) };
}

macro_rules! error {
    ($($arg:expr),+ $(,)?) => { log_impl!(error, $($arg),+) };
}
