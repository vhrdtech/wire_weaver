use proc_macro::{TokenStream, TokenTree};
use std::path::PathBuf;
use ww_ast::file::FileSource;

#[proc_macro]
pub fn data_structures(input: TokenStream) -> TokenStream {
    // dbg!(&input);
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let mut input = input.into_iter();
    let Some(TokenTree::Literal(root_file_path)) = input.next() else {
        panic!("Provide WireWeaver root file as argument");
    };
    let root_file_path = root_file_path.to_string();
    let root_file_path = root_file_path
        .strip_prefix('\"')
        .unwrap()
        .strip_suffix('\"')
        .unwrap();
    let root_file_path: PathBuf = [manifest_dir.as_str(), root_file_path].iter().collect();
    let root_file_contents = std::fs::read_to_string(&root_file_path).unwrap();
    let syn_file = syn::parse_file(root_file_contents.as_str()).unwrap();
    dbg!(&syn_file);

    let source = FileSource::File(root_file_path);
    let (ww_file, warnings) = ww_ast::File::from_syn(source, syn_file).unwrap();
    dbg!(warnings);
    dbg!(&ww_file);

    let ts = ww_codegen::rust_no_std_file(&ww_file);
    eprintln!("{ts}");

    ts.into()
}
