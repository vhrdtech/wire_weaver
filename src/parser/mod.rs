use yaml_rust::Yaml;

use crate::ast;
use crate::ast::{ResourceName, ResourceKind};
use crate::types::Type;
use crate::types::Numeric;

pub fn parse_resource(tok_tree: &Yaml) -> ast::Resource {

     ast::Resource{
         id: Some(0u32),
         name: ResourceName::Terminal("res".to_string()),
         children: Vec::new(),
         kind: ResourceKind::Property,
         r#type: Type::Numeric(Numeric::U8)
     }
}
