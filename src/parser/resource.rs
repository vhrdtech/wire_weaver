use yaml_rust::Yaml;
use nom;

use crate::ast;
use crate::ast::{ResourceName, ResourceKind};
use crate::types::Type;
use crate::types::Numeric;

struct ResourceNameRaw<'a> {
    left_part: Option<&'a str>,
    set: Option<&'a str>,
    right_part: Option<&'a str>,
    r#type: Option<&'a str>,
    id: Option<u32>
}

fn get_slash(i: &str) -> nom::IResult<&str, &str> {
    nom::bytes::complete::tag("/")(i)
}

fn parse_name(name: &str) -> ResourceNameRaw {
    let res = get_slash("/abcd");
    println!("{:?}", res);
    ResourceNameRaw {
        left_part: Some(&name[1..3]),
        set: None,
        right_part: None,
        r#type: None,
        id: None
    }
}

pub fn parse_resource(name: &str, tok_tree: &Yaml) -> ast::Resource {
    //dbg!(tok_tree);

    let r = parse_name("TEC(register)[]");

    ast::Resource{
        id: Some(0u32),
        name: ResourceName::Terminal("res".to_string()),
        children: Vec::new(),
        kind: Some(ResourceKind::Property),
        r#type: Some(Type::Numeric(Numeric::U8))
    }
}

#[cfg(test)]
mod tests {
    use crate::parser::resource::parse_name;

    #[test]
    fn parse_name_works() {
        let name1 = "/abcd";
        let name1_p = parse_name(&name1);
        assert_eq!(name1_p.left_part, Some("ab"));
    }
}