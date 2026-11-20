//! `handler_model = ".."` of `ww_codegen!` (SRV-21): which user handlers of a server are `async fn` and which are
//! plain `fn`.
//!
//! Without it every handler follows `use_async`. With it, each item (method, property, stream, sink) is matched by
//! its dotted path in the API tree (`device.settings.vbus_mode`), the first pattern that matches the whole path
//! decides, `_` is the default for the rest (`use_async` when there is no `_`):
//! `handler_model = "device.settings.*|meter.reading=async, _=sync"`. The pattern syntax is
//! [crate::pattern_model]'s.
use crate::pattern_model::{PatternKind, PatternModel, PatternModelItem};

/// Which handlers are `async fn`, by item path.
pub type HandlerModel = PatternModel<HandlerKind>;
/// One `pattern=sync|async` of a [HandlerModel].
pub type HandlerModelItem = PatternModelItem<HandlerKind>;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HandlerKind {
    Sync,
    Async,
}

impl PatternKind for HandlerKind {
    const MODEL: &'static str = "handler_model";
    const KEYWORDS: &'static str = "`sync` or `async`";

    fn from_keyword(keyword: &str) -> Option<Self> {
        match keyword {
            "sync" => Some(HandlerKind::Sync),
            "async" => Some(HandlerKind::Async),
            _ => None,
        }
    }
}

impl HandlerModel {
    /// Kind of the handlers of the item at `path` (dotted, `device.settings.vbus_mode`): the first matching
    /// pattern, else `_`, else what `use_async` says.
    pub fn pick(&self, path: &str, use_async: bool) -> HandlerKind {
        self.pick_written(path).unwrap_or(if use_async {
            HandlerKind::Async
        } else {
            HandlerKind::Sync
        })
    }

    /// Errors for patterns that match none of `paths` (a typo, or an item that was renamed), and for `async`
    /// handlers in a blocking server.
    pub fn check<'a>(
        &self,
        paths: impl Iterator<Item = &'a str> + Clone,
        use_async: bool,
    ) -> Vec<String> {
        let mut errors = self.unmatched(paths, "method, property or stream of the API");
        if !use_async {
            let async_patterns = self
                .items
                .iter()
                .filter(|item| item.kind == HandlerKind::Async)
                .map(|item| item.pattern.as_str())
                .chain((self.default == Some(HandlerKind::Async)).then_some("_"));
            errors.extend(async_patterns.map(|pattern| {
                format!(
                    "handler_model: `{pattern}=async` in a blocking server (use_async = false): its handlers can't be async"
                )
            }));
        }
        errors
    }
}

#[cfg(test)]
mod tests {
    use super::{HandlerKind, HandlerModel};

    #[test]
    fn parse_and_pick() {
        let m = HandlerModel::parse("device.settings.*|meter.reading=async, _=sync").unwrap();
        assert_eq!(
            m.pick("device.settings.vbus_mode", true),
            HandlerKind::Async
        );
        assert_eq!(m.pick("meter.reading", true), HandlerKind::Async);
        // anchored: a longer name is not matched by a shorter pattern
        assert_eq!(m.pick("meter.reading_raw", true), HandlerKind::Sync);
        assert_eq!(m.pick("plug.vbus", true), HandlerKind::Sync);
    }

    #[test]
    fn default_follows_use_async() {
        let m = HandlerModel::parse("plug.vbus_cycle=async").unwrap();
        assert_eq!(m.pick("plug.vbus_cycle", true), HandlerKind::Async);
        assert_eq!(m.pick("plug.vbus", true), HandlerKind::Async);
        assert_eq!(m.pick("plug.vbus", false), HandlerKind::Sync);
        let m = HandlerModel::parse("").unwrap();
        assert_eq!(m.pick("x", true), HandlerKind::Async);
    }

    #[test]
    fn check_reports_unmatched_and_async_in_blocking() {
        let m = HandlerModel::parse("plug.vbus_cycle=async, nothing.here=sync").unwrap();
        let paths = ["plug.vbus_cycle", "plug.vbus"];
        assert_eq!(m.check(paths.iter().copied(), true).len(), 1);
        assert_eq!(m.check(paths.iter().copied(), false).len(), 2);
        let m = HandlerModel::parse("_=async").unwrap();
        assert_eq!(m.check(paths.iter().copied(), false).len(), 1);
    }

    #[test]
    fn errors() {
        assert!(HandlerModel::parse("_=sync, _=async").is_err());
        assert!(HandlerModel::parse("a=maybe").is_err());
        assert!(HandlerModel::parse("a(=sync").is_err());
    }
}
