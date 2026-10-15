//! How a server provides the value of each `const` item of a `#[ww_trait]` (API-36), the `const_model = "..."`
//! argument of `ww_codegen!`: as an associated const of the generated `<Trait>Consts` trait (`const`, the default),
//! or as a `fn` of that trait, for a value fixed at boot or link time (OTP, chip id, build id).
use anyhow::Error;
use pest::Parser;
use regex::Regex;

mod private {
    use pest_derive::Parser;

    #[derive(Parser)]
    #[grammar = "grammar/const_model.pest"]
    pub(super) struct ConstModelParser;
}

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

impl ConstModel {
    pub fn parse(input: &str) -> Result<ConstModel, Error> {
        use private::Rule;
        let pairs = match private::ConstModelParser::parse(Rule::const_model, input) {
            Ok(pairs) => pairs,
            Err(e) => return Err(Error::msg(format!("{e}"))),
        };

        let mut const_model = ConstModel::default();
        for item in pairs.into_iter() {
            if item.as_rule() == Rule::EOI {
                continue;
            }
            let mut item = item.into_inner();
            let regex = item.next().unwrap().as_str();
            let model = item.next().unwrap().into_inner().next().unwrap();
            let model = match model.as_rule() {
                Rule::value => ConstModelKind::Const,
                Rule::fn_ => ConstModelKind::Fn,
                _ => unreachable!(),
            };
            if regex == "_" {
                if const_model.default.is_some() {
                    return Err(Error::msg("Multiple default values"));
                }
                const_model.default = Some(model);
                continue;
            }
            let regex = Regex::new(&format!("^(?:{regex})$"))?;
            const_model.items.push(ConstModelItem { regex, model });
        }

        Ok(const_model)
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
