use proc_macro::{TokenStream, TokenTree};
use std::path::PathBuf;

#[proc_macro]
pub fn data_structures(input: TokenStream) -> TokenStream {
    // dbg!(&input);
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let mut input = input.into_iter();
    let Some(TokenTree::Literal(root_file_path)) = input.next() else {
        panic!("Provide WireWeaver root file as argument");
    };
    let root_file_path = root_file_path.to_string();
    let root_file_path = root_file_path.strip_prefix('\"').unwrap().strip_suffix('\"').unwrap();
    let root_file_path: PathBuf = [manifest_dir.as_str(), root_file_path].iter().collect();
    let root_file_contents = std::fs::read_to_string(root_file_path).unwrap();
    let x = syn::parse_file(root_file_contents.as_str()).unwrap();
    dbg!(x);
    "".parse().unwrap()
}