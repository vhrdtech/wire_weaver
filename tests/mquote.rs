#[cfg(test)]
mod test {
    use mquote::mquote;
    use mtoken::ToTokens;

    #[test]
    pub fn interpolation() {
        let name = "MyStruct";
        let ts = mquote!(rust r#" struct #name {} "#);
        println!("{:?}", ts);
    }
}
