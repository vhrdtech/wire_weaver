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
        let ts = mquote!(rust r#" ⸨ ∀numbers ⸩,* "#);
        assert_eq!(format!("{}", ts), "1, 2, 3, 4, 5");
    }

    #[test]
    pub fn repeat_inside_group() {
        let numbers = vec![1, 2, 3, 4, 5];
        let ts = mquote!(rust r#" [ ⸨ ∀numbers ⸩,* ] "#);
        assert_eq!(format!("{}", ts), "[1, 2, 3, 4, 5]");
    }

    #[test]
    pub fn repeat_over_two() {
        let numbers1 = vec![1, 2, 3, 4, 5];
        let numbers2 = vec![6, 7, 8, 9, 0];
        let ts = mquote!(rust r#" [ ⸨ ∀numbers1 + ∀numbers2 ⸩,* ] "#);
        assert_eq!(format!("{}", ts), "[1 + 6, 2 + 7, 3 + 8, 4 + 9, 5 + 0]");
    }
}
