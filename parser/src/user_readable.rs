
use crate::lexer::Rule;

pub fn rule_names(rule: &Rule) -> String {
    match rule {
        Rule::file => "definition or inner_attribute".to_owned(),
        Rule::xpi_block => "rs <resource_kind>".to_owned(),
        Rule::xpi_impl => "use XpiTrait;".to_owned(),
        Rule::ty => "type name".to_owned(),
        Rule::resource_cell_ty => "Cell< (wo|rw)? (+stream|+observe)? type_name >".to_owned(),
        Rule::access_mode => "`const`, `ro`, `rw`, `wo`".to_owned(),
        Rule::xpi_serial => "serial number (e.g. #123)".to_owned(),
        Rule::call_arguments => "`(_)`".to_owned(),
        Rule::index_arguments => "`[_]`".to_owned(),
        Rule::generics => "`<_>`".to_owned(),
        Rule::punct_semicolon => "`;`".to_owned(),

        _ => format!("{:?}", rule),
    }
}