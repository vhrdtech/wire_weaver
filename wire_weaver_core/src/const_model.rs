//! How a server provides the value of each `const` item of a `#[ww_trait]` (API-36), the `const_model = "..."`
//! argument of `ww_codegen!`: as an associated const of the generated `<Trait>Consts` trait (`const`, the default),
//! or as a `fn` of that trait, for a value fixed at boot or link time (OTP, chip id, build id).
use crate::pattern_model::{PatternKind, parse_items};
use anyhow::Error;
use regex::Regex;

#[derive(Default, Debug)]
pub struct ConstModel {
    pub default: Option<ConstModelKind>,
    pub items: Vec<ConstModelItem>,
}

#[derive(Debug)]
pub struct ConstModelItem {
    pub regex: Regex,
    pub model: ConstModelKind,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ConstModelKind {
    /// `const NAME: T = ..;` in the server's `<Trait>Consts` impl, a Rust const
    Const,
    /// `fn name(&self) -> T` in the server's `<Trait>Consts` impl, a value fixed at boot or link time
    Fn,
}

impl PatternKind for ConstModelKind {
    const MODEL: &'static str = "const_model";
    const KEYWORDS: &'static str = "`const` or `fn`";

    fn from_keyword(keyword: &str) -> Option<Self> {
        match keyword {
            "const" => Some(ConstModelKind::Const),
            "fn" => Some(ConstModelKind::Fn),
            _ => None,
        }
    }
}

impl ConstModel {
    /// Parses `"pattern=keyword, _=keyword"`, the syntax of [crate::pattern_model]; a pattern matches a const's whole name.
    pub fn parse(input: &str) -> Result<ConstModel, Error> {
        let (default, written) = parse_items::<ConstModelKind>(input)?;
        let items = written
            .into_iter()
            .map(|item| {
                Ok(ConstModelItem {
                    regex: item.regex_whole()?,
                    model: item.kind,
                })
            })
            .collect::<Result<_, Error>>()?;
        Ok(ConstModel { default, items })
    }

    /// Model of the const item `ident` (as in the trait, `BOARD`), `const` unless picked otherwise.
    pub fn pick(&self, ident: &str) -> ConstModelKind {
        for item in &self.items {
            if item.regex.is_match(ident) {
                return item.model;
            }
        }
        self.default.unwrap_or(ConstModelKind::Const)
    }
}

#[cfg(test)]
mod tests {
    use super::{ConstModel, ConstModelKind};

    #[test]
    fn const_model_parse() {
        let m = ConstModel::parse("BOARD|IDS=fn, _=const").unwrap();
        assert_eq!(m.pick("BOARD"), ConstModelKind::Fn);
        assert_eq!(m.pick("IDS"), ConstModelKind::Fn);
        assert_eq!(m.pick("BOARD_ID"), ConstModelKind::Const);
        assert_eq!(m.pick("FIRMWARE"), ConstModelKind::Const);
        assert_eq!(ConstModel::default().pick("X"), ConstModelKind::Const);
        assert!(ConstModel::parse("X=get_set").is_err());
    }
}
