//! What the book shows of blinky, checked against `docs/generated/blinky/`: the handlers `ww api scaffold` writes for
//! its API, in the chapter "Your own project".

use std::path::Path;

use wire_weaver_cli::cmd::api::scaffold;
use wire_weaver_core::ServerScaffoldConfig;

/// `ww api scaffold examples/blinky_api --use-async`: the options the CLI takes when only `--use-async` is given.
#[test]
fn book_output_scaffold() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::env::set_current_dir(&root).expect("the repository root");
    let config = ServerScaffoldConfig {
        server_struct: "Server".to_owned(),
        no_alloc: true,
        use_async: true,
        handler_model: String::new(),
        receiver_model: String::new(),
        method_model: String::new(),
        property_model: String::new(),
        const_model: String::new(),
        medium: None,
        omit: vec![],
        observe_cells: vec![],
        observe_samples: true,
        introspect: String::new(),
        introspect_compression: String::new(),
    };
    let scaffold =
        scaffold::render("examples/blinky_api".into(), None, &config, false).expect("scaffolds");
    assert!(scaffold.hints.is_empty(), "{:?}", scaffold.hints);
    book_outputs::check("blinky/scaffold.txt", &scaffold.code);
}
