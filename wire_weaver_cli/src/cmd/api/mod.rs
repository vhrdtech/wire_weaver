mod ast;
mod check;
mod diff;
mod save;
mod scaffold;

use anyhow::Result;
use wire_weaver_core::ServerScaffoldConfig;

use clap::{Subcommand, ValueHint};
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum ApiCommand {
    /// Print API tree
    Tree {
        /// Path to crate which defines ww_trait
        #[arg(value_hint = ValueHint::DirPath)]
        path: PathBuf,

        /// Optional trait name if more than one is present
        #[arg(long)]
        name: Option<String>,

        /// Do not print documentation for each resource
        #[arg(short('d'), long)]
        skip_docs: bool,

        /// Also print all types the API refers to
        #[arg(short('t'), long)]
        types: bool,
    },
    /// Generate server side scaffold: a server struct, a stub for every handler and a `ww_codegen!` invocation
    ///
    /// Every method, property getter and setter stub returns Unimplemented, stream and on-changed handlers do
    /// nothing and no array index is valid, so the generated code compiles and answers every method and property
    /// request right away.
    /// Handler signatures depend on the options below, pass the same ones `ww_codegen!` is going to use.
    Scaffold {
        /// Path to crate which defines ww_trait
        #[arg(value_hint = ValueHint::DirPath)]
        path: PathBuf,

        /// Optional trait name if more than one is present
        #[arg(long)]
        name: Option<String>,

        /// Server struct name
        #[arg(long, default_value = "Server")]
        server: String,

        /// Generate handlers for `no_alloc = false`, taking owned arguments (e.g. String instead of &str)
        #[arg(long)]
        alloc: bool,

        /// Generate async handlers
        #[arg(long)]
        use_async: bool,

        /// Method model, see ww_codegen! docs
        #[arg(long, default_value = "_=immediate")]
        method_model: String,

        /// Property model, see ww_codegen! docs
        #[arg(long, default_value = "_=get_set")]
        property_model: String,

        /// Medium type passed to handlers in `Context` (e.g. `crate::Medium`), see ww_codegen! docs
        #[arg(long)]
        medium: Option<String>,

        /// Write to a file instead of stdout, refuses to overwrite an existing one
        #[arg(short, long, value_hint = ValueHint::FilePath)]
        output: Option<PathBuf>,
    },
    /// Save all traits and types defined in a crate into `<path>/api_snapshots/<crate>_<major>_<minor>_<patch>.ron`
    ///
    /// Snapshots are meant to be committed and kept unchanged, so that API bundles (e.g., device introspection data)
    /// can refer to these traits and types by crate name and version only, instead of carrying their definitions.
    /// Traits and types from other crates are only referenced, save a snapshot of each of those crates as well.
    ///
    /// Saving again is a no-op if nothing changed. Any change is an error, doc comments included:
    /// bump the crate version instead (compatible position for doc-only changes). A new version is compared with
    /// the previous snapshot as `ww api check` does, and is only saved if bumped enough.
    Save {
        /// Path to crate which defines ww_trait's and/or data types
        #[arg(value_hint = ValueHint::DirPath)]
        path: PathBuf,

        /// Save even if traits or types changed without a sufficient version bump
        #[arg(long)]
        force: bool,
    },
    /// Check that the crate version is bumped enough for the changes made since the latest saved snapshot
    ///
    /// Traits and types defined in the crate are compared with the newest snapshot in `<path>/api_snapshots/` that is
    /// not newer than the crate version (see docs/evolution/rules.md). Breaking changes require a bump of the
    /// breaking position (minor before 1.0, major after), any other change, doc comments included, a bump of the
    /// compatible position (patch before 1.0, minor after).
    Check {
        /// Path to crate which defines ww_trait's and/or data types, or to a snapshot, which is then compared with
        /// the previous snapshot in the same directory
        #[arg(value_hint = ValueHint::AnyPath)]
        path: PathBuf,

        /// Compare with this snapshot instead of the latest one saved in the crate
        #[arg(long, value_hint = ValueHint::FilePath)]
        against: Option<PathBuf>,
    },
    /// List every change made since the latest saved snapshot, doc comments included
    ///
    /// Compares the same versions as `ww api check`, and prints each added, removed or changed trait, resource,
    /// type, field, variant, doc comment and dependency version, followed by the verdict of `ww api check`. Unlike
    /// it, doesn't fail if the version is not bumped enough.
    Diff {
        /// Path to crate which defines ww_trait's and/or data types, or to a snapshot, which is then compared with
        /// the previous snapshot in the same directory
        #[arg(value_hint = ValueHint::AnyPath)]
        path: PathBuf,

        /// Compare with this snapshot instead of the latest one saved in the crate
        #[arg(long, value_hint = ValueHint::FilePath)]
        against: Option<PathBuf>,
    },
    /// Print AST
    Ast {
        /// Path to crate which defines ww_trait
        #[arg(value_hint = ValueHint::DirPath)]
        path: PathBuf,

        /// Optional trait name if more than one is present
        #[arg(long)]
        name: Option<String>,
    },
}
pub(crate) fn api(cmd: ApiCommand) -> Result<()> {
    match cmd {
        ApiCommand::Tree {
            path,
            name,
            skip_docs,
            types,
        } => {
            let bundle = wire_weaver_core::load(&path, name, false)?;
            print!("{}", crate::api_tree::render(&bundle, skip_docs));
            if types {
                println!();
                print!("{}", crate::api_tree::render_types(&bundle, skip_docs));
            }
            Ok(())
        }
        ApiCommand::Scaffold {
            path,
            name,
            server,
            alloc,
            use_async,
            method_model,
            property_model,
            medium,
            output,
        } => scaffold::scaffold(
            path,
            name,
            ServerScaffoldConfig {
                server_struct: server,
                no_alloc: !alloc,
                use_async,
                method_model,
                property_model,
                medium,
            },
            output,
        ),
        ApiCommand::Save { path, force } => save::save(path, force),
        ApiCommand::Check { path, against } => check::check(path, against),
        ApiCommand::Diff { path, against } => diff::diff(path, against),
        ApiCommand::Ast { path, name } => ast::print_ast(path, name),
    }
}
