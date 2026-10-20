//! Server scaffold: a starting point for the user side of a server, with a stub for every handler
//! [gen_server](crate::gen_server) expects to be implemented on the server struct.
//!
//! Handler names and signatures must stay in sync with `codegen/api_server.rs`.
use crate::codegen::omit::{Omissions, Omit};
use crate::codegen::ty_def::{TyPos, ty_def};
use crate::const_model::{ConstModel, ConstModelKind};
use crate::handler_model::{HandlerKind, HandlerModel};
use crate::method_model::{MethodModel, MethodModelKind};
use crate::property_model::{PropertyModel, PropertyModelKind};
use anyhow::{Context, Result, anyhow};
use convert_case::{Case, Casing};
use std::fmt::Write;
use ww_numeric::{NumericAnyTypeOwned, NumericBaseType};
use ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, Batching, Delivery, FlowControl,
    Multiplicity, PropertyAccess, StreamFnRef, TypeOwned,
};

/// Server scaffold generation configuration, same meaning as the `ww_codegen!` arguments.
pub struct ServerScaffoldConfig {
    /// Name of the server struct to generate.
    pub server_struct: String,
    pub no_alloc: bool,
    pub use_async: bool,
    /// Which handlers are `async fn` (SRV-21); empty means all follow `use_async`.
    pub handler_model: String,
    /// Empty means `_=immediate`.
    pub method_model: String,
    /// Empty means `_=get_set`.
    pub property_model: String,
    /// Empty means `_=const`.
    pub const_model: String,
    /// Path to the medium type passed to handlers in `wire_weaver::Context`, `()` if None.
    pub medium: Option<String>,
    /// `#[optional]` items or groups to leave out, `"i2c.pullups"` (API-16): no stubs for them, passed on to
    /// `ww_codegen!(.., omit = [..])`.
    pub omit: Vec<String>,
    /// Levels sampled through observe cells (API-50), by their `ObserveState` field (`root_foc`): passed on to
    /// `ww_codegen!(.., observe_cells = "..")`, with a `static` `<Level>Cells` each.
    pub observe_cells: Vec<String>,
}

/// Generates Rust source of a server struct, an impl block with a stub for every handler, each returning
/// `Unimplemented` (or doing nothing, where there is no result to return), and a `ww_codegen!` invocation
/// generating the server dispatcher for them.
///
/// The output is not formatted, run it through rustfmt.
pub fn gen_server_scaffold(
    api_bundle: &ApiBundleOwned,
    config: &ServerScaffoldConfig,
) -> Result<String> {
    let method_model_str = if config.method_model.is_empty() {
        "_=immediate"
    } else {
        config.method_model.as_str()
    };
    let property_model_str = if config.property_model.is_empty() {
        "_=get_set"
    } else {
        config.property_model.as_str()
    };
    let method_model =
        MethodModel::parse(method_model_str).context("failed to parse method model")?;
    let property_model =
        PropertyModel::parse(property_model_str).context("failed to parse property model")?;
    let const_model =
        ConstModel::parse(&config.const_model).context("failed to parse const model")?;
    let handler_model =
        HandlerModel::parse(&config.handler_model).context("failed to parse handler model")?;

    let omit: Vec<Omit> = config.omit.iter().map(Omit::always).collect();
    let omissions = Omissions::resolve(api_bundle, &omit).map_err(|e| anyhow!(e))?;
    let mut observed = super::observe::Observe::collect(api_bundle, &omissions, &property_model);
    if !observed.use_cells(&config.observe_cells).is_empty() {
        return Err(anyhow!(
            "--observe-cells: {:?} names a level without +batched properties; levels: {}",
            config.observe_cells,
            observed.batched_levels().join(", ")
        ));
    }
    let cells = !config.observe_cells.is_empty();
    let observe_state = if cells {
        "api_impl::ObserveState<api_impl::ObserveBatches<'static>>"
    } else {
        "api_impl::ObserveState"
    };
    let observe = observed.is_enabled();
    let observe_batched = observed.has_batched();

    let root = &api_bundle.root;
    let crate_name = root.crate_name(api_bundle)?.replace('-', "_");
    let out_trait = if config.use_async {
        "EventOut"
    } else {
        "BlockingEventOut"
    };
    let cx = match &config.medium {
        Some(medium) => format!("_cx: &mut Context<'_, impl {out_trait}, {medium}>, "),
        None => format!("_cx: &mut Context<'_, impl {out_trait}>, "),
    };
    let mut s = Scaffold {
        api_bundle,
        no_alloc: config.no_alloc,
        use_async: config.use_async,
        cx,
        handler_model,
        current: String::new(),
        method_model,
        property_model,
        const_model,
        omissions,
        fields: String::new(),
        consts: String::new(),
        handlers: String::new(),
        positioned: false,
    };
    s.level(root, None, "root".into(), 0, "", &[], None)?;
    if s.positioned {
        writeln!(
            s.fields,
            "/// Sessions of counted and reliable streams opened so far, see `new_stream_session`
             pub stream_sessions: u32,"
        )?;
        writeln!(
            s.handlers,
            "/// Number of a new session of a counted or reliable stream, when a host opens one.\n\
             fn new_stream_session(&mut self) -> u32 {{\n\
             // TODO: put a boot counter into the high bits (or use a random number), so that a cursor from\n\
             // before a reboot doesn't match a new session\n\
             self.stream_sessions = self.stream_sessions.wrapping_add(1);\n\
             self.stream_sessions\n\
             }}\n"
        )?;
    }

    if cells {
        let readers: Vec<String> = config
            .observe_cells
            .iter()
            .map(|l| format!("{}.reader().unwrap()", l.to_uppercase()))
            .collect();
        writeln!(
            s.fields,
            "/// Observed properties (SRV-18): what hosts observe and when it is due, see `poll_properties`;
             /// `ObserveState::new(limits, api_impl::ObserveBatches::new(ww_stream::batch::DEFAULT_POLICY, {}))`
             /// (on a firmware `ObserveBatches::init(STATIC_CELL.uninit(), ..)`, built in place, and
             /// `ObserveState<&'static mut ObserveBatches<'static>>`); the levels on observe cells take samples in place through their cells' writer, the others with
             /// `observe.samplers().<level>.sample(t, ..)` (API-39, API-50)
             pub observe: {observe_state},",
            readers.join(", ")
        )?;
    } else if observe_batched {
        writeln!(
            s.fields,
            "/// Observed properties (SRV-18): what hosts observe and when it is due, see `poll_properties`;
             /// `ObserveState::new(limits, api_impl::ObserveBatches::new(ww_stream::batch::DEFAULT_POLICY))` (on a
             /// firmware `ObserveBatches::init(STATIC_CELL.uninit(), ..)`, built in place in a `static`), take
             /// samples of `+batched` properties with `observe.samplers().<level>.sample(t, ..)` (or `split()` the
             /// batches for an interrupt) (API-39)
             pub observe: api_impl::ObserveState,"
        )?;
    } else if observe {
        writeln!(
            s.fields,
            "/// Observed properties (SRV-18): what hosts observe and when it is due, see `poll_properties`
             pub observe: api_impl::ObserveState,"
        )?;
    }
    if observe {
        writeln!(
            s.handlers,
            "/// Settings and send state of observed properties, for `Observe` requests and `poll_properties`.\n\
             fn observe_state(&mut self) -> &mut {observe_state} {{\n\
             &mut self.observe\n\
             }}\n"
        )?;
    }

    let server = &config.server_struct;
    let medium = match &config.medium {
        Some(medium) => format!("medium = {medium:?},\n"),
        None => String::new(),
    };
    let omit = if config.omit.is_empty() {
        String::new()
    } else {
        format!("omit = {:?},\n", config.omit)
    };
    let const_model = if config.const_model.is_empty() {
        String::new()
    } else {
        format!("const_model = {:?},\n", config.const_model)
    };
    // samples per batch of `+batched` properties: the device's choice
    let observe_batch = if observe_batched {
        format!("observe_batch = {DEFAULT_BATCH},\n")
    } else {
        String::new()
    };
    let observe_cells = if cells {
        format!("observe_cells = {:?},\n", config.observe_cells.join(", "))
    } else {
        String::new()
    };
    // a static ring per cells level: the control loop or interrupt keeps its writer
    let mut statics = String::new();
    if observe {
        writeln!(
            statics,
            "/// Changes made outside the server state and the server task's wake (SRV-25):\n\
             /// `OBSERVE.mark_changed_<name>()` from any task or interrupt. Give it to `observe.with_signal(&OBSERVE)`;\n\
             /// the server task waits on `OBSERVE.wait()` next to `wait_until(observe.next_wake(now))`.\n\
             pub static OBSERVE: api_impl::ObserveSignal = api_impl::ObserveSignal::new();\n"
        )?;
    }
    for level in &config.observe_cells {
        let ty = format!("{}Cells", level.to_case(Case::Pascal));
        writeln!(
            statics,
            "/// Observe cells of `{level}` (API-50): rows the control loop or interrupt writes in place with\n\
             /// `{name}.writer()` (once, kept), `poll_properties` reads through `{name}.reader()`. 32 rows wait\n\
             /// between two polls.\n\
             pub static {name}: api_impl::{ty}<32> = api_impl::{ty}::new();\n",
            name = level.to_uppercase()
        )?;
    }
    let trait_name = &root.trait_name;
    let mut out = String::new();
    writeln!(
        out,
        "// Server scaffold for `{crate_name}::{trait_name}`, generated by `ww api scaffold`.\n\
         // Every handler is a stub returning Unimplemented, replace them with real implementations.\n\
         use wire_weaver::prelude::*;\n"
    )?;
    out.push_str(&statics);
    writeln!(out, "pub struct {server} {{\n{}}}\n", s.fields)?;
    writeln!(out, "impl {server} {{\n{}}}\n", s.handlers)?;
    if !s.consts.is_empty() {
        writeln!(
            out,
            "/// Values of the `const` items, served without a handler.\n\
             impl api_impl::{trait_name}Consts for {server} {{\n{}}}\n",
            s.consts
        )?;
    }
    let handler_model = if config.handler_model.is_empty() {
        String::new()
    } else {
        format!("handler_model = {:?},\n", config.handler_model)
    };
    writeln!(
        out,
        "mod api_impl {{\n\
         wire_weaver::ww_codegen!(\n\
         {crate_name} :: {trait_name} for super::{server},\n\
         server = true, no_alloc = {}, use_async = {},\n\
         {handler_model}method_model = {method_model_str:?},\n\
         property_model = {property_model_str:?},\n\
         {const_model}{omit}{medium}{observe_batch}{observe_cells});\n\
         }}",
        config.no_alloc, config.use_async
    )?;
    Ok(out)
}

struct Scaffold<'a> {
    api_bundle: &'a ApiBundleOwned,
    no_alloc: bool,
    use_async: bool,
    /// `_cx: &mut Context<..>, ` argument every handler takes
    cx: String,
    handler_model: HandlerModel,
    /// Dotted path of the item being scaffolded, for `handler_model`
    current: String,
    method_model: MethodModel,
    property_model: PropertyModel,
    const_model: ConstModel,
    omissions: Omissions,
    fields: String,
    /// Items of the `<Trait>Consts` impl
    consts: String,
    handlers: String,
    /// A counted or reliable stream was scaffolded: `new_stream_session` is needed
    positioned: bool,
}

impl Scaffold<'_> {
    /// Mirrors `process_request_inner_recursive` and `level_matchers`: `prefix` is the chain of trait item
    /// idents handler names are prefixed with, `level_name_chain` is `root_<..>` used in valid indices handler
    /// names, `index_len` is the length of the index chain passed to handlers on this level, `item_path` the trait
    /// item idents from the root (for omitted items), `optional` what `omit = [..]` names to leave out the level's
    /// `#[optional]` `ww_impl!` (API-54).
    #[allow(clippy::too_many_arguments)]
    fn level(
        &mut self,
        level: &ApiLevelOwned,
        prefix: Option<&str>,
        level_name_chain: String,
        index_len: usize,
        path: &str,
        item_path: &[String],
        optional: Option<String>,
    ) -> Result<()> {
        let omitted = self.omissions.level(item_path);
        let crate_name = level.crate_name(self.api_bundle)?;
        let at = if path.is_empty() { "root" } else { path };
        writeln!(
            self.handlers,
            "// ---- {crate_name}::{} at {at} ----",
            level.trait_name
        )?;
        if let Some(name) = optional {
            writeln!(
                self.handlers,
                "// optional, all handlers of {at} can be left out with omit = [\"{name}\"]"
            )?;
        }
        writeln!(self.handlers)?;
        for item in &level.items {
            if omitted.contains_key(&item.id.0) {
                continue;
            }
            // an optional trait gets its note in its own section, and here above its valid indices if an array
            if let Some(optional) = &item.optional
                && !(matches!(item.kind, ApiItemKindOwned::Trait { .. })
                    && matches!(item.multiplicity, Multiplicity::Flat))
            {
                let name = optional.group.as_ref().unwrap_or(&item.ident);
                let is_const = matches!(
                    item.kind,
                    ApiItemKindOwned::Property {
                        access: PropertyAccess::Const,
                        ..
                    }
                );
                let out = if is_const {
                    &mut self.consts
                } else {
                    &mut self.handlers
                };
                writeln!(
                    out,
                    "// optional, can be left out with omit = [\"{}\"]",
                    item_path
                        .iter()
                        .chain([name])
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(".")
                )?;
            }
            let is_array = matches!(item.multiplicity, Multiplicity::Array { .. });
            if is_array {
                self.valid_indices(item, &level_name_chain, index_len, path)?;
            }
            let item_index_len = index_len + is_array as usize;
            let name = match prefix {
                Some(prefix) => format!("{prefix}_{}", item.ident),
                None => item.ident.clone(),
            };
            self.current = format!(
                "{path}{}{}",
                if path.is_empty() { "" } else { "." },
                item.ident
            );
            match &item.kind {
                ApiItemKindOwned::Method {
                    args, return_ty, ..
                } => self.method(item, &name, item_index_len, args, return_ty)?,
                ApiItemKindOwned::Property {
                    ty,
                    access,
                    write_err_ty,
                    ..
                } => self.property(item, &name, item_index_len, ty, *access, write_err_ty)?,
                ApiItemKindOwned::Stream { is_up: true, .. }
                | ApiItemKindOwned::StreamFn { .. } => {
                    let stream = item.kind.as_stream_fn().expect("a stream");
                    self.stream_fn(item, &name, item_index_len, stream)?
                }
                ApiItemKindOwned::Stream { ty, is_up: false } => {
                    self.stream(item, &name, item_index_len, ty, false)?
                }
                ApiItemKindOwned::FlowSink { ty, flow } => {
                    self.flow(item, &name, item_index_len, *flow)?;
                    self.stream(item, &name, item_index_len, ty, false)?
                }
                ApiItemKindOwned::Trait { .. } => {}
            }
        }
        for item in &level.items {
            if !matches!(item.kind, ApiItemKindOwned::Trait { .. })
                || omitted.contains_key(&item.id.0)
            {
                continue;
            }
            let is_array = matches!(item.multiplicity, Multiplicity::Array { .. });
            let child = item.get_as_level(self.api_bundle)?;
            let optional = item.optional.as_ref().map(|optional| {
                let name = optional.group.as_ref().unwrap_or(&item.ident);
                item_path
                    .iter()
                    .chain([name])
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(".")
            });
            let child_prefix = match prefix {
                Some(prefix) => format!("{prefix}_{}", item.ident),
                None => item.ident.clone(),
            };
            let child_path = format!(
                "{path}{}{}{}",
                if path.is_empty() { "" } else { "." },
                item.ident,
                if is_array { "[]" } else { "" }
            );
            let mut child_item_path = item_path.to_vec();
            child_item_path.push(item.ident.clone());
            self.level(
                child,
                Some(&child_prefix),
                format!("{level_name_chain}_{}", item.ident),
                index_len + is_array as usize,
                &child_path,
                &child_item_path,
                optional,
            )?;
        }
        Ok(())
    }

    fn valid_indices(
        &mut self,
        item: &ApiItemOwned,
        level_name_chain: &str,
        index_len: usize,
        path: &str,
    ) -> Result<()> {
        writeln!(
            self.handlers,
            "/// Valid indices of `{path}{}{}[]`.\n\
             fn valid_indices_{level_name_chain}_{}(&mut self, {}) -> ValidIndices<'_> {{\n\
             // TODO: no index is valid until implemented\n\
             ValidIndices::range_u32(0..0)\n\
             }}\n",
            if path.is_empty() { "" } else { "." },
            item.ident,
            item.ident.to_case(Case::Snake),
            index_chain_arg(index_len),
        )?;
        Ok(())
    }

    fn method(
        &mut self,
        item: &ApiItemOwned,
        name: &str,
        index_len: usize,
        args: &[ww_self::ArgumentOwned],
        return_ty: &Option<TypeOwned>,
    ) -> Result<()> {
        let mut args_def = String::new();
        for arg in args {
            // same as the args struct fields handlers are called with, see api_common::args_structs
            let ty = self.ty(&arg.ty, !self.no_alloc)?;
            write!(args_def, "_{}: {ty}, ", arg.ident)?;
        }
        let return_ty = match return_ty {
            Some(ty) => self.ty(ty, false)?,
            None => "()".into(),
        };
        self.docs(item)?;
        if self.method_model.pick(name) == Some(MethodModelKind::Deferred) {
            writeln!(
                self.handlers,
                "// method_model: deferred, return Deferred and send the result later"
            )?;
        }
        writeln!(
            self.handlers,
            "{}fn {name}(&mut self, {}{}{args_def}) -> RpcResult<{return_ty}> {{\n\
             Unimplemented.into()\n\
             }}\n",
            self.maybe_async(),
            self.cx,
            index_chain_arg(index_len),
        )?;
        Ok(())
    }

    fn property(
        &mut self,
        item: &ApiItemOwned,
        name: &str,
        index_len: usize,
        ty: &TypeOwned,
        access: PropertyAccess,
        write_err_ty: &Option<TypeOwned>,
    ) -> Result<()> {
        if matches!(access, PropertyAccess::Const) {
            return self.const_item(item, name, index_len, ty);
        }
        let value_ty = self.ty(ty, false)?;
        let err_ty = match write_err_ty {
            Some(ty) => self.ty(ty, false)?,
            None => "()".into(),
        };
        let is_writable = matches!(
            access,
            PropertyAccess::WriteOnly | PropertyAccess::ReadWrite { .. }
        );
        let is_readable = !matches!(access, PropertyAccess::WriteOnly);
        let idx = index_chain_arg(index_len);
        let cx = self.cx.clone();
        let maybe_async = self.maybe_async();
        match self
            .property_model
            .pick(&item.ident)
            .unwrap_or(PropertyModelKind::GetSet)
        {
            PropertyModelKind::GetSet => {
                if is_writable {
                    self.docs(item)?;
                    writeln!(
                        self.handlers,
                        "{maybe_async}fn set_{name}(&mut self, {cx}{idx}_value: {value_ty}) -> SetResult<{err_ty}> {{\n\
                         Unimplemented.into()\n\
                         }}\n",
                    )?;
                }
                if is_readable {
                    self.docs(item)?;
                    writeln!(
                        self.handlers,
                        "{maybe_async}fn get_{name}(&mut self, {cx}{idx}) -> GetResult<{value_ty}, {err_ty}> {{\n\
                         Unimplemented.into()\n\
                         }}\n",
                    )?;
                }
            }
            PropertyModelKind::ValueOnChanged => {
                // generated code reads and writes `self.<name>[index_chain[0]]..` directly
                let field_ty = value_ty.replace("'_", "'static");
                for line in &item.docs {
                    writeln!(self.fields, "///{}", doc_line(line))?;
                }
                if index_len == 0 {
                    writeln!(self.fields, "pub {name}: {field_ty},")?;
                } else {
                    let mut array_ty = field_ty;
                    for i in (0..index_len).rev() {
                        array_ty = format!("[{array_ty}; N{i}]");
                    }
                    let indices: String = (0..index_len).map(|i| format!("[i{i}]")).collect();
                    writeln!(
                        self.fields,
                        "// TODO: indexed as self.{name}{indices}\n// pub {name}: {array_ty},"
                    )?;
                }
                if is_writable {
                    self.docs(item)?;
                    writeln!(
                        self.handlers,
                        "/// Called after `{name}` was changed by a client.\n\
                         {maybe_async}fn changed_{name}(&mut self, {cx}{idx}) {{}}\n",
                    )?;
                }
            }
        }
        Ok(())
    }

    /// A `const` item: an associated const or a `fn` of the `<Trait>Consts` impl, as `api_server::consts_trait`.
    fn const_item(
        &mut self,
        item: &ApiItemOwned,
        name: &str,
        index_len: usize,
        ty: &TypeOwned,
    ) -> Result<()> {
        for line in &item.docs {
            writeln!(self.consts, "///{}", doc_line(line))?;
        }
        match self.const_model.pick(&item.ident) {
            ConstModelKind::Const => {
                let ty = ty_def(self.api_bundle, ty, false, TyPos::Static)?.to_string();
                writeln!(
                    self.consts,
                    "// TODO: a Rust const, or const_model = \"{}=fn\" for a value fixed at boot or link time\n\
                     const {}: {ty} = todo!();\n",
                    item.ident,
                    name.to_case(Case::UpperSnake),
                )?;
            }
            ConstModelKind::Fn => {
                let ty = self.ty(ty, false)?;
                writeln!(
                    self.consts,
                    "fn {}(&self, {}) -> {ty} {{\n\
                     todo!()\n\
                     }}\n",
                    name.to_case(Case::Snake),
                    index_chain_arg(index_len),
                )?;
            }
        }
        Ok(())
    }

    fn stream(
        &mut self,
        item: &ApiItemOwned,
        name: &str,
        index_len: usize,
        ty: &TypeOwned,
        is_up: bool,
    ) -> Result<()> {
        let idx = index_chain_arg(index_len);
        let cx = self.cx.clone();
        let maybe_async = self.maybe_async();
        self.docs(item)?;
        writeln!(
            self.handlers,
            "{maybe_async}fn sideband_{name}(&mut self, {cx}{idx}_sideband: ww_client_server::StreamSideband) -> Option<ww_client_server::StreamSideband> {{\n\
             // TODO: None sends no reply\n\
             None\n\
             }}\n",
        )?;
        if !is_up {
            // same special cases as api_server::handle_stream
            let arg = match ty {
                TypeOwned::Tuple(elements) if elements.is_empty() => "_value: ()".into(),
                TypeOwned::Vec(inner)
                    if matches!(
                        inner.as_ref(),
                        TypeOwned::NumericAny(NumericAnyTypeOwned::Base(NumericBaseType::U8))
                    ) =>
                {
                    "_data: &wire_weaver::shrink_wrap::tail_bytes::TailBytes<'_>".into()
                }
                TypeOwned::MaxLen { .. } if ty.is_byte_slice(self.api_bundle)? => {
                    "_data: &wire_weaver::shrink_wrap::tail_bytes::TailBytes<'_>".into()
                }
                _ => format!("_value: {}", self.ty(ty, false)?),
            };
            self.docs(item)?;
            writeln!(
                self.handlers,
                "{maybe_async}fn write_{name}(&mut self, {cx}{idx}{arg}) {{}}\n",
            )?;
        }
        Ok(())
    }

    /// A stream (device to host) as a stream function: its open handler, `close_`, `sideband_` for plain ones,
    /// and the state accessor and field of a counted, reliable or batched one.
    fn stream_fn(
        &mut self,
        item: &ApiItemOwned,
        name: &str,
        index_len: usize,
        stream: StreamFnRef<'_>,
    ) -> Result<()> {
        let idx = index_chain_arg(index_len);
        let cx = self.cx.clone();
        let maybe_async = self.maybe_async();
        let mut args_def = String::new();
        for arg in stream.args {
            // same as the args struct fields handlers are called with, see api_common::args_structs
            let ty = self.ty(&arg.ty, !self.no_alloc)?;
            write!(args_def, "_{}: {ty}, ", arg.ident)?;
        }
        let (return_ty, body) = match stream.return_ty {
            Some(ty) => (self.ty(ty, false)?, "Unimplemented.into()"),
            // nothing to decide: accept the open
            None => ("()".into(), "RpcResult::Ready(())"),
        };
        self.docs(item)?;
        writeln!(
            self.handlers,
            "///\n\
             /// Opens the stream (a new session): `Ok` / `Ready` starts it.\n\
             {maybe_async}fn {name}(&mut self, {cx}{idx}{args_def}) -> RpcResult<{return_ty}> {{\n\
             {body}\n\
             }}\n\
             \n\
             /// The host closed the stream.\n\
             {maybe_async}fn close_{name}(&mut self, {cx}{idx}) {{}}\n",
        )?;
        if stream.delivery == Delivery::Plain {
            writeln!(
                self.handlers,
                "{maybe_async}fn sideband_{name}(&mut self, {cx}{idx}_sideband: ww_client_server::StreamSideband) -> Option<ww_client_server::StreamSideband> {{\n\
                 // TODO: None sends no reply\n\
                 None\n\
                 }}\n",
            )?;
            if stream.batched.is_none() {
                return Ok(());
            }
        }
        let (delivery_state, kind, keeps) = match stream.delivery {
            Delivery::Counted => ("ww_stream::tx::Counted", "counted", "positions, sessions"),
            Delivery::Reliable => (
                "ww_stream::tx::Reliable<ww_stream::store::RamRing<4096>>",
                "reliable",
                "positions, sessions, store",
            ),
            _ => ("ww_stream::batch::Plain", "plain", "open or not"),
        };
        let (state, kind, keeps) = match stream.batched {
            Some(batching) => {
                let item = self.ty(stream.ty, false)?;
                let item = if batching == Batching::Untimed {
                    item
                } else {
                    format!("ww_stream::batch::Timed<{item}>")
                };
                (
                    // items per batch: the device's choice, the host doesn't know it
                    format!("ww_stream::batch::Batched<{delivery_state}, {item}, {DEFAULT_BATCH}>"),
                    format!("batched {kind}"),
                    format!(
                        "{keeps}, batches of its items; `Batched::new(.., ww_stream::batch::DEFAULT_POLICY)`"
                    ),
                )
            }
            None => (
                delivery_state.to_string(),
                kind.to_string(),
                keeps.to_string(),
            ),
        };
        if stream.delivery != Delivery::Plain {
            self.positioned = true;
        }
        let body = if index_len == 0 {
            writeln!(
                self.fields,
                "/// State of the {kind} stream `{name}` ({keeps})\n\
                 pub {name}_stream: {state},",
            )?;
            format!("&mut self.{name}_stream")
        } else {
            writeln!(
                self.fields,
                "// TODO: one {state} per index of `{name}`\n// pub {name}_stream: [{state}; N],"
            )?;
            format!(
                "todo!(\"stream state of {} for index {{_index_chain:?}}\")",
                item.ident
            )
        };
        writeln!(
            self.handlers,
            "/// State of `{}`: generated code opens, resumes, closes it and feeds it the host's \
             acknowledgements; producers send through it.\n\
             {maybe_async}fn stream_{name}(&mut self, {idx}) -> &mut {state} {{\n\
             {body}\n\
             }}\n",
            item.ident
        )?;
        Ok(())
    }

    /// Flow control state accessor of a sink, generated code keeps the state up to date through it.
    fn flow(
        &mut self,
        item: &ApiItemOwned,
        name: &str,
        index_len: usize,
        flow: FlowControl,
    ) -> Result<()> {
        match flow {
            FlowControl::Credit => {
                let idx = index_chain_arg(index_len);
                let state = "ww_stream::sink::SinkCredit";
                let body = if index_len == 0 {
                    writeln!(
                        self.fields,
                        "/// Credit sink state of `{name}`: how much the device can buffer, `free()` it after use
                         pub {name}_credit: {state},",
                    )?;
                    format!("&mut self.{name}_credit")
                } else {
                    writeln!(
                        self.fields,
                        "// TODO: one {state} per index of `{name}`
// pub {name}_credit: [{state}; N],"
                    )?;
                    format!(
                        "todo!(\"credit state of {} for index {{_index_chain:?}}\")",
                        item.ident
                    )
                };
                writeln!(
                    self.handlers,
                    "/// Credit state of `{}`: generated code opens and closes it, feeds it the host's writes and queries and sends its acknowledgements, see docs/api/credit_sinks.md.
                     fn credit_{name}(&mut self, {idx}) -> &mut {state} {{
                     {body}
                     }}
",
                    item.ident
                )?;
            }
        }
        Ok(())
    }

    fn docs(&mut self, item: &ApiItemOwned) -> Result<()> {
        for line in &item.docs {
            writeln!(self.handlers, "///{}", doc_line(line))?;
        }
        Ok(())
    }

    fn ty(&self, ty: &TypeOwned, alloc: bool) -> Result<String> {
        Ok(ty_def(self.api_bundle, ty, alloc, TyPos::Arg)?.to_string())
    }

    /// `async ` for the handlers of the current item if `handler_model` (or `use_async`) makes them async.
    fn maybe_async(&self) -> &'static str {
        match self.handler_model.pick(&self.current, self.use_async) {
            HandlerKind::Async => "async ",
            HandlerKind::Sync => "",
        }
    }
}

fn index_chain_arg(len: usize) -> String {
    if len == 0 {
        String::new()
    } else {
        format!("_index_chain: [UNib32; {len}], ")
    }
}

fn doc_line(line: &str) -> String {
    if line.is_empty() || line.starts_with(' ') {
        line.to_string()
    } else {
        format!(" {line}")
    }
}

/// Items per batch the scaffold writes into a batched stream's state, `ww_stream::batch::DEFAULT_BATCH` (this crate
/// doesn't depend on `ww_stream`).
pub(crate) const DEFAULT_BATCH: usize = 32;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn config(observe_cells: &[&str]) -> ServerScaffoldConfig {
        ServerScaffoldConfig {
            server_struct: "Server".into(),
            no_alloc: true,
            use_async: true,
            handler_model: String::new(),
            method_model: String::new(),
            property_model: String::new(),
            const_model: String::new(),
            medium: None,
            omit: vec![],
            observe_cells: observe_cells.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// `handler_model` (SRV-21): each item's handlers are `async fn` or `fn` as the model says, the rest follow
    /// `use_async`, and the model goes into the `ww_codegen!` call.
    #[test]
    fn handler_model() {
        let crate_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/observe_api");
        let bundle = crate::load(&crate_path, Some("Observe".into()), false).unwrap();
        let mixed = ServerScaffoldConfig {
            handler_model: "temp|motor.speed=async, _=sync".into(),
            ..config(&[])
        };
        let code = gen_server_scaffold(&bundle, &mixed).unwrap();
        for expected in [
            "async fn get_temp(",
            "async fn get_motor_speed(",
            "\nfn get_motor_current(",
            "\nfn get_label(",
            "\nfn set_temp(",
            "handler_model = \"temp|motor.speed=async, _=sync\",",
        ] {
            assert!(code.contains(expected), "{expected}\n{code}");
        }
        // without a model everything follows use_async
        let code = gen_server_scaffold(&bundle, &config(&[])).unwrap();
        assert!(code.contains("async fn get_motor_current(") && !code.contains("handler_model"));
    }

    /// `observe_cells` (API-50): a static ring per level, its reader in `ObserveBatches::new`, the argument in
    /// `ww_codegen!`; a level without `+batched` properties is an error naming the ones there are.
    #[test]
    fn observe_cells() {
        let crate_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/observe_batched_api");
        let bundle = crate::load(&crate_path, Some("ObserveBatched".into()), false).unwrap();
        let code = gen_server_scaffold(&bundle, &config(&["root"])).unwrap();
        for expected in [
            "pub static ROOT: api_impl::RootCells<32> = api_impl::RootCells::new();",
            "ObserveBatches::new(ww_stream::batch::DEFAULT_POLICY, ROOT.reader().unwrap())",
            "pub observe: api_impl::ObserveState<api_impl::ObserveBatches<'static>>,",
            "observe_cells = \"root\",",
        ] {
            assert!(code.contains(expected), "{expected}\n{code}");
        }
        let plain = gen_server_scaffold(&bundle, &config(&[])).unwrap();
        assert!(!plain.contains("observe_cells") && !plain.contains("Cells<"));
        let e = gen_server_scaffold(&bundle, &config(&["root_nope"])).unwrap_err();
        assert!(e.to_string().contains("levels: root_slow, root"), "{e}");
    }
}
