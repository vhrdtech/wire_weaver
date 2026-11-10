mod ast;
pub(crate) mod check;
mod diff;
pub use line::LineCommand;
mod line;
mod save;
mod scaffold;

use anyhow::Result;
use wire_weaver_core::ServerScaffoldConfig;

use clap::{Subcommand, ValueHint};
use std::path::PathBuf;

// parsed once from the command line, its size doesn't matter
#[allow(clippy::large_enum_variant)]
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

        /// The API root to scaffold: a trait name, or `module::Trait` in a crate with several roots
        /// (`wire_v2::Thermo`, EVO-37). Required when the crate defines more than one
        #[arg(long, alias = "name")]
        root: Option<String>,

        /// Server struct name
        #[arg(long, default_value = "Server")]
        server: String,

        /// Generate handlers for `no_alloc = false`, taking owned arguments (e.g. String instead of &str)
        #[arg(long)]
        alloc: bool,

        /// Generate async handlers
        #[arg(long)]
        use_async: bool,

        /// Which handlers are `async fn` and which plain `fn` with --use-async, e.g.
        /// "device.settings.*|meter.reading=async, _=sync"; empty: all follow --use-async (see ww_codegen! docs)
        #[arg(long, default_value = "")]
        handler_model: String,

        /// Method model, see ww_codegen! docs
        #[arg(long, default_value = "_=immediate")]
        method_model: String,

        /// Property model, see ww_codegen! docs
        #[arg(long, default_value = "_=get_set")]
        property_model: String,

        /// Const model, see ww_codegen! docs: `BOARD|IDS=fn` for `const` items whose value is fixed at boot or
        /// link time instead of a Rust const
        #[arg(long, default_value = "")]
        const_model: String,

        /// Medium type passed to handlers in `Context` (e.g. `crate::Medium`), see ww_codegen! docs
        #[arg(long)]
        medium: Option<String>,

        /// `#[optional]` items or groups to leave out (`i2c.pullups`, `reset`): no stubs, `omit = [..]` in
        /// ww_codegen!. Repeat or separate with commas
        #[arg(long, value_delimiter = ',', value_name = "TRAIT_ITEM.ITEM")]
        omit: Vec<String>,

        /// Levels with `+batched` properties to sample through observe cells (API-50), by their `ObserveState`
        /// field (`root_foc`): a static `<Level>Cells` per level and `observe_cells = ".."` in ww_codegen!.
        /// Repeat or separate with commas
        #[arg(long, value_delimiter = ',', value_name = "LEVEL")]
        observe_cells: Vec<String>,

        /// This firmware takes no samples of its `+batched` properties (SRV-56): `observe_samples = false` in
        /// ww_codegen!, no batcher, outbox or `Series` encoder; `Samples` mode is refused with `ModeNotSupported`
        #[arg(long)]
        no_observe_samples: bool,

        /// Introspection mode for ww_codegen!: `no_docs` or `with_docs`; absent: disabled
        #[arg(long, value_parser = ["no_docs", "with_docs", "disabled"])]
        introspect: Option<String>,

        /// Keep the introspection data compressed in flash (INT-16): `lz4`; absent: none
        #[arg(long, value_parser = ["lz4", "none"], requires = "introspect")]
        introspect_compression: Option<String>,

        /// Write to a file instead of stdout, refuses to overwrite an existing one
        #[arg(short, long, value_hint = ValueHint::FilePath)]
        output: Option<PathBuf>,
    },
    /// Save all traits and types defined in a crate into `<path>/api_snapshots/<crate>_<major>_<minor>_<patch>.ron`
    ///
    /// A crate with several API roots (`[package.metadata.wire_weaver.roots]`) gets one snapshot per root and wire
    /// version instead, `<crate>_<major>_<minor>_<patch>.<module path>.<Trait>@w<line>.<revision>.ron`: the root and
    /// what it reaches, checked against the root's previous snapshot. A root that didn't change since keeps that one.
    ///
    /// Snapshots are meant to be committed and kept unchanged, so that API bundles (e.g., device introspection data)
    /// can refer to these traits and types by crate name and version only, instead of carrying their definitions.
    /// Traits and types from other crates are only referenced, save a snapshot of each of those crates as well.
    ///
    /// Only the snapshot of the crate's current version is written: saved, or refreshed when the source changed since
    /// (a no-op if nothing changed). A released version's snapshot never changes: bump the crate version instead
    /// (compatible position for doc-only changes). The version is compared with the previous version's snapshot as
    /// `ww api check` does, and is only saved if bumped enough.
    Save {
        /// Path to crate which defines ww_trait's and/or data types
        #[arg(value_hint = ValueHint::DirPath)]
        path: PathBuf,

        /// Save even if traits or types changed without a sufficient version bump
        #[arg(long)]
        force: bool,

        /// Save the snapshot of this dependency of the crate at `path`, from the source Cargo builds it from (a git
        /// or registry dependency too): for a crate kept in another repository
        #[arg(long, value_name = "CRATE", requires = "snapshots_dir")]
        dep: Option<String>,

        /// Save into this directory instead of the crate's `api_snapshots/`
        #[arg(long, value_hint = ValueHint::DirPath)]
        snapshots_dir: Option<PathBuf>,
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

        /// Fail if there is no snapshot to compare with, instead of passing unchecked
        #[arg(long)]
        strict: bool,

        /// Require the snapshot of the crate's current version and the source to match it, and check the bump from
        /// the previous version's snapshot to it (implies --strict)
        #[arg(long)]
        require_current: bool,
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
    /// Start or end a wire line of a crate with several API roots (TOOL-33, EVO-37)
    #[command(subcommand)]
    Line(LineCommand),
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
            let mut bundle = wire_weaver_core::load(&path, name, false)?;
            // as `ww introspect` sees it: generic instances concrete, so a `Fixed` shows its unit (CLT-54)
            wire_weaver_client::ww_self::generics::prepare(&mut bundle)?;
            print!("{}", crate::api_tree::render(&bundle, skip_docs));
            if types {
                println!();
                print!("{}", crate::api_tree::render_types(&bundle, skip_docs));
            }
            Ok(())
        }
        ApiCommand::Scaffold {
            path,
            root,
            server,
            alloc,
            use_async,
            handler_model,
            method_model,
            property_model,
            const_model,
            medium,
            omit,
            observe_cells,
            no_observe_samples,
            introspect,
            introspect_compression,
            output,
        } => scaffold::scaffold(
            path,
            root,
            ServerScaffoldConfig {
                server_struct: server,
                no_alloc: !alloc,
                use_async,
                handler_model,
                method_model,
                property_model,
                const_model,
                medium,
                omit,
                observe_cells,
                observe_samples: !no_observe_samples,
                introspect: introspect.unwrap_or_default(),
                introspect_compression: introspect_compression.unwrap_or_default(),
            },
            output,
        ),
        ApiCommand::Save {
            path,
            force,
            dep,
            snapshots_dir,
        } => save::save(path, force, dep, snapshots_dir),
        ApiCommand::Check {
            path,
            against,
            strict,
            require_current,
        } => check::check(path, against, strict, require_current),
        ApiCommand::Diff { path, against } => diff::diff(path, against),
        ApiCommand::Line(cmd) => line::line(cmd),
        ApiCommand::Ast { path, name } => ast::print_ast(path, name),
    }
}
