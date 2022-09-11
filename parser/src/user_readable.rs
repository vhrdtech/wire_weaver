use crate::lexer::Rule;

pub fn rule_names(rule: &Rule) -> String {
    match rule {
        Rule::file => "definition or inner_attribute".to_owned(),
        Rule::xpi_block => "/xpi_block".to_owned(),
        Rule::xpi_impl => "use XpiTrait;".to_owned(),
        _ => format!("{:?}", rule)
    }
}