use proc_macro::{TokenStream, TokenTree};
use std::path::PathBuf;
use wire_weaver_core::ast::file::File;
use wire_weaver_core::ast::file::FileSource;

mod shrink_wrap;

#[proc_macro]
pub fn wire_weaver(input: TokenStream) -> TokenStream {
    // dbg!(&input);
    let mut input = input.into_iter();
    let Some(TokenTree::Literal(contents_or_path)) = input.next() else {
        panic!("Provide WireWeaver root file as argument");
    };
    let flags: Vec<_> = input
        .filter_map(|tt| {
            if let TokenTree::Ident(ident) = tt {
                Some(ident.to_string())
            } else {
                None
            }
        })
        .collect();
    let contents_or_path = contents_or_path.to_string();
    let (root_file_path, root_file_contents) = if contents_or_path.starts_with('\"') {
        let root_file_path = contents_or_path
            .strip_prefix('\"')
            .unwrap()
            .strip_suffix('\"')
            .unwrap();
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("Cargo manifest dir");
        let root_file_path: PathBuf = [manifest_dir.as_str(), root_file_path].iter().collect();
        let root_file_contents = std::fs::read_to_string(&root_file_path).unwrap();
        (root_file_path, root_file_contents)
    } else {
        let contents = contents_or_path
            .strip_prefix("r#\"")
            .unwrap()
            .strip_suffix("\"#")
            .unwrap();
        ("inline".into(), contents.to_string())
    };
    let syn_file = syn::parse_file(root_file_contents.as_str()).unwrap();
    if flags.iter().any(|f| f.as_str() == "dbg_syn") {
        dbg!(&syn_file);
    }

    let source = FileSource::File(root_file_path);
    let (ww_file, warnings) = File::from_syn(source, syn_file).unwrap();
    dbg!(warnings);
    if flags.iter().any(|f| f.as_str() == "dbg_ds") {
        dbg!(&ww_file);
    }

    let ts = wire_weaver_core::codegen::rust_no_std_file(&ww_file);
    if flags.iter().any(|f| f.as_str() == "dbg_gen") {
        eprintln!("{ts}");
    }

    ts.into()
}

/// Use Rust definition of an enum or struct to derive shrink wrap wire format.
/// Created for internal use in API code generation and introspection (wire format of WireWeaver itself).
/// Probably should not be used directly in user code.
#[proc_macro_derive(ShrinkWrap)]
pub fn shrink_wrap_serdes(item: TokenStream) -> TokenStream {
    let ts = shrink_wrap::shrink_wrap(item);
    ts.into()
}

#[proc_macro_attribute]
pub fn wire_weaver_api(attr: TokenStream, item: TokenStream) -> TokenStream {
    eprintln!("{attr:?}");
    eprintln!("{item:?}");
    item
}
