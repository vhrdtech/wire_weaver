use crate::pattern_model::{PatternKind, parse_items};
use anyhow::Error;
use regex::Regex;

#[derive(Default, Debug)]
pub struct MethodModel {
    pub default: Option<MethodModelKind>,
    pub items: Vec<MethodModelItem>,
}

#[derive(Debug)]
pub struct MethodModelItem {
    pub regex: Regex,
    pub model: MethodModelKind,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum MethodModelKind {
    Immediate,
    Deferred,
}

impl PatternKind for MethodModelKind {
    const MODEL: &'static str = "method_model";
    const KEYWORDS: &'static str = "`immediate` or `deferred`";

    fn from_keyword(keyword: &str) -> Option<Self> {
        match keyword {
            "immediate" => Some(MethodModelKind::Immediate),
            "deferred" => Some(MethodModelKind::Deferred),
            _ => None,
        }
    }
}

impl MethodModel {
    /// Parses `"pattern=keyword, _=keyword"`, the syntax of [crate::pattern_model]; a pattern matches anywhere in a method's
    /// name.
    pub fn parse(input: &str) -> Result<MethodModel, Error> {
        let (default, written) = parse_items::<MethodModelKind>(input)?;
        let items = written
            .into_iter()
            .map(|item| {
                Ok(MethodModelItem {
                    regex: item.regex_anywhere()?,
                    model: item.kind,
                })
            })
            .collect::<Result<_, Error>>()?;
        Ok(MethodModel { default, items })
    }

    pub fn pick(&self, method_path: &str) -> Option<MethodModelKind> {
        for item in &self.items {
            if item.regex.is_match(method_path) {
                return Some(item.model);
            }
        }
        self.default
    }
}

#[cfg(test)]
mod tests {
    use crate::method_model::{MethodModel, MethodModelKind};

    #[test]
    fn method_model_parse() {
        let m = MethodModel::parse(".*move=deferred, _=immediate").unwrap();
        assert_eq!(m.pick("motor_move_to"), Some(MethodModelKind::Deferred));
        assert_eq!(m.pick("stop"), Some(MethodModelKind::Immediate));
    }
}
