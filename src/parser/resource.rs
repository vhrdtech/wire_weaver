use yaml_rust::Yaml;

use crate::ast;
use crate::ast::{ResourceName, ResourceKind};
use crate::types::Type;
use crate::types::Numeric;

pub fn parse_name(name: &str) -> ResourceName {

}

pub fn parse_resource(name: &str, tok_tree: &Yaml) -> ast::Resource {
    dbg!(tok_tree);



    ast::Resource{
        id: Some(0u32),
        name: ResourceName::Terminal("res".to_string()),
        children: Vec::new(),
        kind: Some(ResourceKind::Property),
        r#type: Some(Type::Numeric(Numeric::U8))
    }
}