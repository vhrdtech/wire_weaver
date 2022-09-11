use crate::lexer::Rule;

pub fn rule_names(rule: &Rule) -> String {
    match rule {
        Rule::file => "definition or inner_attribute".to_owned(),
        Rule::xpi_block => "/xpi_block".to_owned(),
        Rule::xpi_impl => "use XpiTrait;".to_owned(),
        Rule::any_ty => "type name".to_owned(),
        Rule::resource_cell_ty => "Cell< (wo|wr)? (+stream|+observe)? type_name >".to_owned(),
        Rule::access_mode => "const|ro|rw|wo".to_owned(),
        _ => format!("{:?}", rule),
    }
}
