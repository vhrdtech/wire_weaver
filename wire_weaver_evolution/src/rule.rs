//! Which evolution rule a breaking change breaks, with the docs page that explains it.

use std::fmt;

/// An evolution rule, as written down in `docs/evolution/rules.md` (and a few other pages).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rule {
    /// Traits, resources and types are never removed: clients and code using them break.
    Removed,
    /// A rename keeps the wire, but breaks Rust code using the old name.
    Renamed,
    /// Resources are identified by their position in a trait: new ones go at the end.
    Moved,
    /// A trait's `ww_global` id never changes and is never removed.
    GlobalId,
    /// A resource of the old version and the same one of the new version work together, both ways.
    Resource,
    /// A type written by either version is read by the other one.
    Type,
    /// A max length (`MaxLen<T, N>`) is only raised or removed, never lowered or added.
    MaxLen,
    /// Named and unnamed fields are different Rust types.
    FieldKind,
    /// An `#[optional]` item stays optional and in its group.
    Optional,
    /// An enum's catch-all for variants from a newer version (`#[future_version]`) is part of its Rust type.
    CatchAll,
    /// A marker type argument of a generic type (a unit, a prefix or scale of `ww_si::Fixed`) is its meaning: the
    /// same bytes, but read as another quantity.
    Meaning,
    /// A bound (`ww_si::Range` and friends) is part of the contract: values outside the reader's bound are refused.
    Bound,
}

impl Rule {
    /// The rule in one sentence.
    pub fn summary(&self) -> &'static str {
        match self {
            Rule::Removed => {
                "traits, resources and types are never removed, clients and code using them break"
            }
            Rule::Renamed => "a rename keeps the wire, but breaks Rust code using the old name",
            Rule::Moved => "resources are identified by position, new ones go at the end",
            Rule::GlobalId => "a trait's ww_global id never changes and is never removed",
            Rule::Resource => {
                "a resource works between the old and the new version, both ways (old client with a new device, \
                 new client with an old device)"
            }
            Rule::Type => "a type written by either version is read by the other one",
            Rule::MaxLen => "a max length is only raised or removed, never lowered or added",
            Rule::FieldKind => "named and unnamed fields are different Rust types",
            Rule::Optional => "an #[optional] item stays optional and in its group",
            Rule::CatchAll => {
                "an enum's catch-all for newer variants keeps the wire, but is part of its Rust type: it is \
                 never added, removed or renamed"
            }
            Rule::Meaning => {
                "a marker argument (unit, prefix or scale) is what the bytes mean: changing it keeps the bytes, \
                 but a reader takes one quantity for another (µs for ms)"
            }
            Rule::Bound => {
                "a bound is part of the contract: narrowed, the new version refuses what the old one writes; \
                 widened, the old version refuses what the new one writes"
            }
        }
    }

    /// Docs page and section explaining the rule, relative to the repository root.
    pub fn doc(&self) -> &'static str {
        match self {
            Rule::Removed | Rule::Moved | Rule::Resource => "docs/evolution/rules.md#api",
            Rule::Renamed => "docs/evolution/rules.md",
            Rule::GlobalId => "docs/api/addressing.md#global-trait-addressing-compactversion",
            Rule::Type => "docs/evolution/rules.md#data-types",
            Rule::MaxLen => "docs/evolution/rules.md#maximum-lengths-maxlent-n",
            Rule::FieldKind => "docs/evolution/rules.md#unsized-types",
            Rule::Optional => "docs/evolution/rules.md#optional-items",
            Rule::CatchAll => "docs/evolution/rules.md#unsized-types",
            Rule::Meaning => "docs/evolution/rules.md#transparent-wrappers-units-and-scales",
            Rule::Bound => "docs/evolution/rules.md#bounds",
        }
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.summary(), self.doc())
    }
}

/// A breaking change found, and the rule it breaks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub rule: Rule,
    /// What changed, e.g. `Api::led_on removed`.
    pub message: String,
}

impl Finding {
    pub fn new(rule: Rule, message: impl Into<String>) -> Self {
        Finding {
            rule,
            message: message.into(),
        }
    }

    /// Whether the message contains `s`.
    pub fn contains(&self, s: &str) -> bool {
        self.message.contains(s)
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\n    rule: {}", self.message, self.rule)
    }
}
