use anyhow::Error;
use pest::Parser;
use regex::Regex;

mod private {
    use pest_derive::Parser;

    #[derive(Parser)]
    #[grammar = "grammar/property_model.pest"]
    pub(super) struct PropertyModelParser;
}

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

#[derive(Debug, Copy, Clone)]
pub enum PropertyModelKind {
    GetSet,
    ValueOnChanged,
}

impl PropertyModel {
    pub fn parse(input: &str) -> Result<PropertyModel, Error> {
        use private::Rule;
        let pairs = match private::PropertyModelParser::parse(Rule::property_model, input) {
            Ok(pairs) => pairs,
            Err(e) => return Err(Error::msg(format!("{e}"))),
        };

        let mut property_model = PropertyModel {
            default: None,
            items: vec![],
        };
        for item in pairs.into_iter() {
            let mut item = item.into_inner();
            let regex = item.next().unwrap().as_str();
            let model = item.next().unwrap().into_inner().next().unwrap();
            let model = match model.as_rule() {
                Rule::get_set => PropertyModelKind::GetSet,
                Rule::value_on_changed => PropertyModelKind::ValueOnChanged,
                _ => unreachable!(),
            };
            if regex == "_" {
                if property_model.default.is_some() {
                    return Err(Error::msg("Multiple default values"));
                }
                property_model.default = Some(model);
                continue;
            }
            let regex = Regex::new(regex)?;
            property_model
                .items
                .push(PropertyModelItem { regex, model });
        }

        Ok(property_model)
    }

    pub fn pick(&self, property_name: &str) -> Option<PropertyModelKind> {
        for item in &self.items {
            if item.regex.is_match(property_name) {
                return Some(item.model);
            }
        }
        self.default
    }
}

#[cfg(test)]
mod tests {
    use crate::property_model::PropertyModel;

    #[test]
    fn property_model_parse() {
        let m = PropertyModel::parse(".*en=get_set, _=value_on_changed").unwrap();
        println!("{:?}", m);
    }
}
