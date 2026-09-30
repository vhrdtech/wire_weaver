//! Logging macros: `defmt` or `log`, depending on the enabled feature, no-op otherwise.
//! If both features are enabled, `defmt` is used.
//!
//! Re-exported with `use` instead of being wrapped in `macro_rules`, so that the backend's macro is expanded
//! directly at the call site: defmt takes the log location from the span of a static it generates, which
//! would otherwise point to this file. Import with `use crate::fmt::*;`, format strings must be valid for both
//! backends (`{}` and `{:?}`, no defmt-specific `{=u8}` and the like).
#![allow(unused_imports)]

#[cfg(feature = "defmt")]
pub(crate) use defmt::{debug, error, info, trace, warn};

#[cfg(all(feature = "log", not(feature = "defmt")))]
pub(crate) use log::{debug, error, info, trace, warn};

#[cfg(not(any(feature = "defmt", feature = "log")))]
mod noop {
    macro_rules! noop {
        ($s:literal $(, $x:expr)* $(,)?) => {{
            let _ = ($(&$x),*);
        }};
    }
    pub(crate) use noop as trace;
    pub(crate) use noop as debug;
    pub(crate) use noop as info;
    pub(crate) use noop as warn;
    pub(crate) use noop as error;
}
#[cfg(not(any(feature = "defmt", feature = "log")))]
pub(crate) use noop::{debug, error, info, trace, warn};
