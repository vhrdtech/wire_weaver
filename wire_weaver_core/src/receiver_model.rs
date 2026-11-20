//! `receiver_model = ".."` of `ww_codegen!` (SRV-65): which items of a server run side by side through `&self`
//! handlers and which run alone through `&mut self` ones.
//!
//! Every item is `exclusive` unless the model says `shared`. Only methods (a call) and readable properties (a
//! read) can be shared, a key companion's `index_of` and `key` included; writes, streams, sinks and observing stay
//! exclusive whatever the model says. `receiver_model = "sensors.*|status=shared"`, `receiver_model =
//! "_=shared, motor.*=exclusive"`. The pattern syntax is [crate::pattern_model]'s.
//!
//! A shared item's method handler or getter takes `&self`, as does the `valid_indices_*` of every array on its path.
//! The server then also gets `process_shared_bytes(&self, ..)`, the dispatch of shared calls and reads, and
//! `is_shared(bytes)`, which tells a runtime which of the two entries a request goes to.
use crate::pattern_model::{PatternKind, PatternModel, PatternModelItem};

/// Which items are shared, by item path.
pub type ReceiverModel = PatternModel<ReceiverKind>;
/// One `pattern=shared|exclusive` of a [ReceiverModel].
pub type ReceiverModelItem = PatternModelItem<ReceiverKind>;

/// How an item's handlers take the server.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ReceiverKind {
    /// `&self`: calls and reads run side by side, through `process_shared_bytes`.
    Shared,
    /// `&mut self`: the request runs alone, through `process_request_bytes`.
    Exclusive,
}

impl PatternKind for ReceiverKind {
    const MODEL: &'static str = "receiver_model";
    const KEYWORDS: &'static str = "`shared` or `exclusive`";

    fn from_keyword(keyword: &str) -> Option<Self> {
        match keyword {
            "shared" => Some(ReceiverKind::Shared),
            "exclusive" => Some(ReceiverKind::Exclusive),
            _ => None,
        }
    }
}

impl ReceiverModel {
    /// Whether the item at `path` (dotted) is shared; `exclusive` when nothing matches.
    pub fn is_shared(&self, path: &str) -> bool {
        self.pick_written(path) == Some(ReceiverKind::Shared)
    }

    /// Errors for patterns that match none of `shareable`, the paths of the methods and readable properties.
    pub fn check<'a>(&self, shareable: impl Iterator<Item = &'a str> + Clone) -> Vec<String> {
        self.unmatched(
            shareable,
            "method or readable property of the API (only those can be shared)",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::ReceiverModel;

    #[test]
    fn default_is_exclusive() {
        let m = ReceiverModel::parse("sensors.*=shared").unwrap();
        assert!(m.is_shared("sensors.temp"));
        assert!(!m.is_shared("motor.speed"));
        assert!(!ReceiverModel::parse("").unwrap().is_shared("x"));
        let m = ReceiverModel::parse("motor.*=exclusive, _=shared").unwrap();
        assert!(!m.is_shared("motor.speed"));
        assert!(m.is_shared("status"));
    }

    #[test]
    fn check_and_errors() {
        let m = ReceiverModel::parse("status=shared, events=shared").unwrap();
        let e = m.check(["status"].into_iter());
        assert_eq!(e.len(), 1);
        assert!(e[0].contains("`events`"), "{}", e[0]);
        assert!(ReceiverModel::parse("a=mutable").is_err());
    }
}
