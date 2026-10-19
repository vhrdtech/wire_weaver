//! `handler_model = ".."` of `ww_codegen!` (SRV-21): which user handlers of a server are `async fn` and which are
//! plain `fn`.
//!
//! Without it every handler follows `use_async`. With it, each item (method, property, stream, sink) is matched by
//! its dotted path in the API tree (`device.settings.vbus_mode`), the first pattern that matches the whole path
//! decides, `_` is the default for the rest (`use_async` when there is no `_`):
//! `handler_model = "device.settings.*|meter.reading=async, _=sync"`. A pattern is a regex without `,`, `=` or
//! whitespace; `.` also matches the dot between levels.
use anyhow::Error;
use pest::Parser;
use regex::Regex;

mod private {
    use pest_derive::Parser;

    #[derive(Parser)]
    #[grammar = "grammar/handler_model.pest"]
    pub(super) struct HandlerModelParser;
}
use private::Rule;

#[derive(Default, Debug)]
pub struct HandlerModel {
    pub default: Option<HandlerKind>,
    pub items: Vec<HandlerModelItem>,
}

#[derive(Debug)]
pub struct HandlerModelItem {
    /// The pattern as written, for error messages.
    pub pattern: String,
    /// The pattern anchored at both ends.
    pub regex: Regex,
    pub kind: HandlerKind,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HandlerKind {
    Sync,
    Async,
}

impl HandlerModel {
    pub fn parse(input: &str) -> Result<HandlerModel, Error> {
        let pairs = match private::HandlerModelParser::parse(Rule::handler_model, input) {
            Ok(pairs) => pairs,
            Err(e) => return Err(Error::msg(format!("{e}"))),
        };
        let mut handler_model = HandlerModel::default();
        for item in pairs {
            if item.as_rule() != Rule::item {
                continue;
            }
            let mut item = item.into_inner();
            let pattern = item.next().unwrap().as_str();
            let kind = match item.next().unwrap().into_inner().next().unwrap().as_rule() {
                Rule::sync => HandlerKind::Sync,
                Rule::async_ => HandlerKind::Async,
                _ => unreachable!(),
            };
            if pattern == "_" {
                if handler_model.default.is_some() {
                    return Err(Error::msg("multiple defaults (`_=..`) in handler_model"));
                }
                handler_model.default = Some(kind);
                continue;
            }
            let regex = Regex::new(&format!("^(?:{pattern})$"))?;
            handler_model.items.push(HandlerModelItem {
                pattern: pattern.to_string(),
                regex,
                kind,
            });
        }
        Ok(handler_model)
    }

    /// Kind of the handlers of the item at `path` (dotted, `device.settings.vbus_mode`): the first matching
    /// pattern, else `_`, else what `use_async` says.
    pub fn pick(&self, path: &str, use_async: bool) -> HandlerKind {
        for item in &self.items {
            if item.regex.is_match(path) {
                return item.kind;
            }
        }
        self.default.unwrap_or(if use_async {
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
        let mut errors = vec![];
        for item in &self.items {
            if !paths.clone().any(|p| item.regex.is_match(p)) {
                errors.push(format!(
                    "handler_model: `{}` matches no method, property or stream of the API",
                    item.pattern
                ));
            }
            if !use_async && item.kind == HandlerKind::Async {
                errors.push(format!(
                    "handler_model: `{}=async` in a blocking server (use_async = false): its handlers can't be async",
                    item.pattern
                ));
            }
        }
        if !use_async && self.default == Some(HandlerKind::Async) {
            errors.push(
                "handler_model: `_=async` in a blocking server (use_async = false): its handlers can't be async"
                    .into(),
            );
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
    }

    #[test]
    fn errors() {
        assert!(HandlerModel::parse("_=sync, _=async").is_err());
        assert!(HandlerModel::parse("a=maybe").is_err());
        assert!(HandlerModel::parse("a(=sync").is_err());
    }
}
