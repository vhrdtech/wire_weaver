use anyhow::Error;
use pest::Parser;
use regex::Regex;

mod private {
    use pest_derive::Parser;

    #[derive(Parser)]
    #[grammar = "grammar/method_model.pest"]
    pub(super) struct MethodModelParser;
}
use private::Rule;

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

impl MethodModel {
    pub fn parse(input: &str) -> Result<MethodModel, Error> {
        let pairs = match private::MethodModelParser::parse(Rule::method_model, input) {
            Ok(pairs) => pairs,
            Err(e) => return Err(Error::msg(format!("{e}"))),
        };

        let mut property_model = MethodModel::default();
        for item in pairs.into_iter() {
            let mut item = item.into_inner();
            let regex = item.next().unwrap().as_str();
            let model = item.next().unwrap().into_inner().next().unwrap();
            let model = match model.as_rule() {
                Rule::immediate => MethodModelKind::Immediate,
                Rule::deferred => MethodModelKind::Deferred,
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
            property_model.items.push(MethodModelItem { regex, model });
        }

        Ok(property_model)
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
    use crate::method_model::MethodModel;

    #[test]
    fn property_model_parse() {
        let m = MethodModel::parse(".*move=deferred, _=immediate").unwrap();
        println!("{:?}", m);
    }
}
