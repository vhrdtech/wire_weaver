//! The `"pattern=keyword, _=keyword"` models of `ww_codegen!` that pick a kind per API item by its dotted path
//! (`device.settings.vbus_mode`): `handler_model` (SRV-21) and `receiver_model` (SRV-65).
//!
//! The first pattern that matches the whole path decides, `_` is the default for the rest. A pattern is a regex
//! without `,`, `=` or whitespace; `.` also matches the dot between levels.
use anyhow::Error;
use pest::Parser;
use regex::Regex;

mod private {
    use pest_derive::Parser;

    #[derive(Parser)]
    #[grammar = "grammar/pattern_model.pest"]
    pub(super) struct PatternModelParser;
}
use private::Rule;

/// A kind a pattern model picks, written as a keyword after `=`.
pub trait PatternKind: Copy + Sized {
    /// Name of the model in error messages: `handler_model`.
    const MODEL: &'static str;
    /// The keywords this kind is written as, for the error message of an unknown one.
    const KEYWORDS: &'static str;

    /// The kind written as `keyword`, `None` for an unknown keyword.
    fn from_keyword(keyword: &str) -> Option<Self>;
}

/// Patterns in the order written, and the kind of `_`.
#[derive(Debug)]
pub struct PatternModel<K> {
    pub default: Option<K>,
    pub items: Vec<PatternModelItem<K>>,
}

impl<K> Default for PatternModel<K> {
    fn default() -> Self {
        Self {
            default: None,
            items: vec![],
        }
    }
}

#[derive(Debug)]
pub struct PatternModelItem<K> {
    /// The pattern as written, for error messages.
    pub pattern: String,
    /// The pattern anchored at both ends.
    pub regex: Regex,
    pub kind: K,
}

impl<K: PatternKind> PatternModel<K> {
    pub fn parse(input: &str) -> Result<Self, Error> {
        let pairs = private::PatternModelParser::parse(Rule::pattern_model, input)
            .map_err(|e| Error::msg(format!("{}: {e}", K::MODEL)))?;
        let mut model = Self::default();
        for item in pairs {
            if item.as_rule() != Rule::item {
                continue;
            }
            let mut item = item.into_inner();
            let pattern = item.next().expect("the grammar has a pattern").as_str();
            let keyword = item.next().expect("the grammar has a model").as_str();
            let kind = K::from_keyword(keyword).ok_or_else(|| {
                Error::msg(format!(
                    "{}: unknown `{keyword}` in `{pattern}={keyword}`, expected {}",
                    K::MODEL,
                    K::KEYWORDS
                ))
            })?;
            if pattern == "_" {
                if model.default.is_some() {
                    return Err(Error::msg(format!(
                        "multiple defaults (`_=..`) in {}",
                        K::MODEL
                    )));
                }
                model.default = Some(kind);
                continue;
            }
            let regex = Regex::new(&format!("^(?:{pattern})$"))?;
            model.items.push(PatternModelItem {
                pattern: pattern.to_owned(),
                regex,
                kind,
            });
        }
        Ok(model)
    }

    /// Whether nothing was written: no pattern and no `_`.
    pub fn is_empty(&self) -> bool {
        self.default.is_none() && self.items.is_empty()
    }

    /// Kind of the item at `path` (dotted, `device.settings.vbus_mode`): the first matching pattern, else `_`.
    pub fn pick_written(&self, path: &str) -> Option<K> {
        self.items
            .iter()
            .find(|item| item.regex.is_match(path))
            .map(|item| item.kind)
            .or(self.default)
    }

    /// Errors for patterns that match none of `paths` (a typo, or an item that was renamed); `what` names the
    /// items `paths` holds.
    pub fn unmatched<'a>(
        &self,
        paths: impl Iterator<Item = &'a str> + Clone,
        what: &str,
    ) -> Vec<String> {
        self.items
            .iter()
            .filter(|item| !paths.clone().any(|p| item.regex.is_match(p)))
            .map(|item| format!("{}: `{}` matches no {what}", K::MODEL, item.pattern))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{PatternKind, PatternModel};

    #[derive(Copy, Clone, Debug, PartialEq)]
    enum Color {
        Red,
        Blue,
    }

    impl PatternKind for Color {
        const MODEL: &'static str = "color_model";
        const KEYWORDS: &'static str = "`red` or `blue`";

        fn from_keyword(keyword: &str) -> Option<Self> {
            match keyword {
                "red" => Some(Color::Red),
                "blue" => Some(Color::Blue),
                _ => None,
            }
        }
    }

    #[test]
    fn parse_and_pick() {
        let m = PatternModel::<Color>::parse("a.*|b=red, _=blue").unwrap();
        assert_eq!(m.pick_written("a.x.y"), Some(Color::Red));
        assert_eq!(m.pick_written("b"), Some(Color::Red));
        // anchored: a longer name is not matched by a shorter pattern
        assert_eq!(m.pick_written("bc"), Some(Color::Blue));
        assert!(PatternModel::<Color>::parse("").unwrap().is_empty());
        assert_eq!(
            PatternModel::<Color>::parse("").unwrap().pick_written("a"),
            None
        );
    }

    #[test]
    fn errors() {
        assert!(PatternModel::<Color>::parse("_=red, _=blue").is_err());
        let e = PatternModel::<Color>::parse("a=green")
            .unwrap_err()
            .to_string();
        assert!(e.contains("color_model") && e.contains("`green`"), "{e}");
        assert!(PatternModel::<Color>::parse("a(=red").is_err());
    }

    #[test]
    fn unmatched() {
        let m = PatternModel::<Color>::parse("a=red, z=blue").unwrap();
        let e = m.unmatched(["a", "b"].into_iter(), "item");
        assert_eq!(e, ["color_model: `z` matches no item"]);
    }
}
