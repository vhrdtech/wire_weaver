#[cfg(test)]
mod test {
    use mquote::mquote;
    use mtoken::ToTokens;

    #[test]
    pub fn interpolate() {
        let name = "MyStruct";
        let ts = mquote!(rust r#" struct Λname {} "#);
        assert_eq!(format!("{}", ts), "struct MyStruct { }")
    }

    #[test]
    pub fn interpolate_path() {
        struct AstNode {
            name: String,
        }
        let node = AstNode { name: "MyStruct".to_owned() };
        let ts = mquote!(rust r#" struct Λ{node.name} {} "#);
        assert_eq!(format!("{}", ts), "struct MyStruct { }")
    }

    #[test]
    pub fn repeat() {
        let numbers = vec![1, 2, 3, 4, 5];
        let ts = mquote!(rust r#" ⸨ ∀numbers ⸩,* "# debug);
        println!("{:?}", ts);
    }
}
