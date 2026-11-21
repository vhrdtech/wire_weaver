use crate::pattern_model::{PatternKind, parse_items};
use anyhow::Error;
use regex::Regex;

#[derive(Default, Debug)]
pub struct PropertyModel {
    pub default: Option<PropertyModelKind>,
    pub items: Vec<PropertyModelItem>,
}

#[derive(Debug)]
pub struct PropertyModelItem {
    pub regex: Regex,
    pub model: PropertyModelKind,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PropertyModelKind {
    GetSet,
    ValueOnChanged,
}

impl PatternKind for PropertyModelKind {
    const MODEL: &'static str = "property_model";
    const KEYWORDS: &'static str = "`get_set` or `value_on_changed`";

    fn from_keyword(keyword: &str) -> Option<Self> {
        match keyword {
            "get_set" => Some(PropertyModelKind::GetSet),
            "value_on_changed" => Some(PropertyModelKind::ValueOnChanged),
            _ => None,
        }
    }
}

impl PropertyModel {
    /// Parses `"pattern=keyword, _=keyword"`, the syntax of [crate::pattern_model]; a pattern matches anywhere in a
    /// property's name.
    pub fn parse(input: &str) -> Result<PropertyModel, Error> {
        let (default, written) = parse_items::<PropertyModelKind>(input)?;
        let items = written
            .into_iter()
            .map(|item| {
                Ok(PropertyModelItem {
                    regex: item.regex_anywhere()?,
                    model: item.kind,
                })
            })
            .collect::<Result<_, Error>>()?;
        Ok(PropertyModel { default, items })
    }

    pub fn pick(&self, property_path: &str) -> Option<PropertyModelKind> {
        for item in &self.items {
            if item.regex.is_match(property_path) {
                return Some(item.model);
            }
        }
        self.default
    }
}

#[cfg(test)]
mod tests {
    use crate::property_model::{PropertyModel, PropertyModelKind};

    #[test]
    fn property_model_parse() {
        let m = PropertyModel::parse(".*en=get_set, _=value_on_changed").unwrap();
        assert_eq!(m.pick("led_en"), Some(PropertyModelKind::GetSet));
        assert_eq!(m.pick("speed"), Some(PropertyModelKind::ValueOnChanged));
    }
}
