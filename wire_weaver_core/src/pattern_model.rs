//! The `"pattern=keyword, _=keyword"` models of `ww_codegen!` that pick a kind per API item by its dotted path
//! (`device.settings.vbus_mode`): `handler_model` (SRV-21) and `receiver_model` (SRV-65).
//!
//! The first pattern that matches the whole path decides, `_` is the default for the rest. A pattern is a regex
//! without `=` or whitespace; `.` also matches the dot between levels.
//!
//! The syntax, shared with `method_model`, `property_model` and `const_model`: `pattern=keyword` items separated by
//! `,`, a trailing `,` allowed, whitespace (newlines too) allowed around each token. A pattern holds no whitespace
//! and no `=` and doesn't start with `,`; `_` as the pattern sets the default, at most once. Errors name the model
//! and the `line:column` of the mistake.
use anyhow::Error;
use regex::Regex;

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
        let (default, written) = parse_items::<K>(input)?;
        let items = written
            .into_iter()
            .map(|item| {
                Ok(PatternModelItem {
                    pattern: item.pattern.to_owned(),
                    regex: item.regex_whole()?,
                    kind: item.kind,
                })
            })
            .collect::<Result<_, Error>>()?;
        Ok(Self { default, items })
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

/// One `pattern=keyword` of a model string, `_=..` excluded.
pub(crate) struct WrittenItem<'a, K> {
    pub(crate) pattern: &'a str,
    pub(crate) kind: K,
}

impl<K: PatternKind> WrittenItem<'_, K> {
    /// The pattern as a regex that must match the whole path.
    pub(crate) fn regex_whole(&self) -> Result<Regex, Error> {
        self.regex(&format!("^(?:{})$", self.pattern))
    }

    /// The pattern as a regex that matches anywhere in the path.
    pub(crate) fn regex_anywhere(&self) -> Result<Regex, Error> {
        self.regex(self.pattern)
    }

    fn regex(&self, regex: &str) -> Result<Regex, Error> {
        Regex::new(regex).map_err(|e| {
            Error::msg(format!(
                "{}: invalid pattern `{}`: {e}",
                K::MODEL,
                self.pattern
            ))
        })
    }
}

/// Splits a model string (the syntax in the [module docs](self)) into the kind of `_` and the other items in the
/// order written.
pub(crate) fn parse_items<K: PatternKind>(
    input: &str,
) -> Result<(Option<K>, Vec<WrittenItem<'_, K>>), Error> {
    let mut default = None;
    let mut items = vec![];
    let mut rest = input.trim_start();
    while !rest.is_empty() {
        let at = |rest: &str| Location(input, input.len() - rest.len());
        let pattern_end = rest
            .find(|c: char| c == '=' || c.is_whitespace())
            .unwrap_or(rest.len());
        let (pattern, after) = rest.split_at(pattern_end);
        if pattern.is_empty() {
            return Err(syntax::<K>(at(rest), "expected a pattern before `=`"));
        }
        if pattern.starts_with(',') {
            return Err(syntax::<K>(
                at(rest),
                "expected a `pattern=keyword` item, found `,`",
            ));
        }
        let Some(after) = after.trim_start().strip_prefix('=') else {
            let found = after.trim_start();
            return Err(syntax::<K>(
                at(found),
                &format!(
                    "expected `=` after `{pattern}`, found {}; a pattern holds no whitespace",
                    describe(found)
                ),
            ));
        };
        let after = after.trim_start();
        let keyword_end = after
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        let (keyword, after) = after.split_at(keyword_end);
        if keyword.is_empty() {
            return Err(syntax::<K>(
                at(after),
                &format!(
                    "expected {} after `{pattern}=`, found {}",
                    K::KEYWORDS,
                    describe(after)
                ),
            ));
        }
        let Some(kind) = K::from_keyword(keyword) else {
            return Err(syntax::<K>(
                at(keyword),
                &format!(
                    "unknown `{keyword}` in `{pattern}={keyword}`, expected {}",
                    K::KEYWORDS
                ),
            ));
        };
        if pattern == "_" {
            if default.is_some() {
                return Err(syntax::<K>(at(rest), "multiple defaults (`_=..`)"));
            }
            default = Some(kind);
        } else {
            items.push(WrittenItem { pattern, kind });
        }
        let after = after.trim_start();
        rest = match after.strip_prefix(',') {
            Some(next) => next.trim_start(),
            None if after.is_empty() => after,
            None => {
                return Err(syntax::<K>(
                    at(after),
                    &format!(
                        "expected `,` after `{pattern}={keyword}`, found {}",
                        describe(after)
                    ),
                ));
            }
        };
    }
    Ok((default, items))
}

/// A byte offset into a model string, shown as `line:column` (both from 1, the column in chars).
struct Location<'a>(&'a str, usize);

impl core::fmt::Display for Location<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let before = &self.0[..self.1];
        let line = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let column = before[line_start..].chars().count() + 1;
        write!(f, "{line}:{column}")
    }
}

fn syntax<K: PatternKind>(at: Location<'_>, message: &str) -> Error {
    Error::msg(format!("{} at {at}: {message}", K::MODEL))
}

/// What the parser found where it expected something else: the next char, or the end.
fn describe(rest: &str) -> String {
    rest.chars()
        .next()
        .map_or_else(|| "the end".to_owned(), |c| format!("`{c}`"))
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
