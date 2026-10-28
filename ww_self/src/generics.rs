//! Generic structs and enums in a bundle (API-43): checking definitions and their uses, and instantiating them.
//!
//! A generic definition (`struct Timed<T> { t: Second, value: T }`) is in the types array once, with its
//! [generics](crate::ItemStruct::generics); its fields refer to the parameters with [Type::Param](TypeOwned::Param),
//! [Length::Param] and [GenericArg::ConstParam](GenericArgOwned::ConstParam). Each use is a
//! [Type::Instance](TypeOwned::Instance) with the arguments.
//!
//! Dynamic users of a bundle (value decoding and encoding, compatibility checks, the ww CLI, Python) work on
//! concrete types: [prepare] checks a bundle and replaces every instance with an [OutOfLine](TypeOwned::OutOfLine)
//! reference to a concrete definition, the generic one with the arguments filled in (`Timed<Ampere>`), added to the
//! types array once per distinct instance. Generic definitions stay in the array as they are.
//!
//! A bundle received from a device is untrusted input: [validate] bounds everything a walk over its types does,
//! and [instantiate] bounds the work and the size of what it adds. A malformed bundle is an error, never a panic.

use crate::{
    ApiBundleOwned, ApiItemKindOwned, ApiLevelLocationOwned, ApiLevelOwned, FieldOwned,
    FieldsOwned, GenericArgOwned, GenericParamKindOwned, GenericParamOwned, InstanceOfOwned,
    ItemEnumOwned, ItemStructOwned, Length, TypeLocationOwned, TypeOwned, ValueOwned,
};
use anyhow::{Result, anyhow, bail};
use shrink_wrap::{SerializeShrinkWrapOwned, UNib32};
use std::collections::HashMap;
use ww_numeric::{NumericAnyTypeOwned, NumericBaseType};

/// Deepest nesting of types allowed in a bundle, counted along the longest path through type definitions (each
/// `Vec`, `Option`, field, out-of-line reference, ... is one level) that doesn't go through a definition twice. Walks
/// over types (compatibility checks, layouts) that stop at a definition already on their path recurse this deep at
/// most once a bundle passed [validate]. Walks over values of self-referential types (dynamic decoding) go as deep as
/// the value, they bound their own depth.
pub const MAX_DEPTH: usize = 96;
/// Most concrete definitions [instantiate] adds to a bundle.
pub const MAX_INSTANCES: usize = 4096;
/// Most type nodes [instantiate] creates in total (and [substitute] in one call).
pub const MAX_NODES: usize = 1 << 20;
/// Longest name of a concrete definition [instantiate] adds (`Timed<Ampere>`), in bytes: names repeat the names of
/// their arguments, nested instances would otherwise name a type with gigabytes.
pub const MAX_NAME_LEN: usize = 1024;

/// Checks `bundle` ([validate]), instantiates every use of a generic type ([instantiate]) and checks the result
/// again. Run on every bundle from outside (a device's introspection data, a file) before using it dynamically,
/// after skipped definitions are put back ([crate::inline]).
pub fn prepare(bundle: &mut ApiBundleOwned) -> Result<()> {
    for location in &bundle.types {
        if let TypeLocationOwned::InLine { ty, .. } = location
            && instance_of(ty).is_some()
        {
            bail!(
                "type {} says which instance it is, only instantiating on the host does that",
                def_ty_name(ty)
            );
        }
    }
    validate(bundle)?;
    instantiate(bundle)?;
    validate(bundle)
}

/// Parameters of a generic definition as written, `<T, const N: u64>`; empty without parameters.
pub fn params_human_name(params: &[GenericParamOwned], bundle: &ApiBundleOwned) -> Result<String> {
    if params.is_empty() {
        return Ok(String::new());
    }
    let mut names = Vec::with_capacity(params.len());
    for p in params {
        names.push(match &p.kind {
            GenericParamKindOwned::Type => p.ident.clone(),
            GenericParamKindOwned::Const { ty } => {
                format!("const {}: {}", p.ident, ty.human_name(false, bundle)?)
            }
        });
    }
    Ok(format!("<{}>", names.join(", ")))
}

// ---------------------------------------------------------------------------------------------------------------------
// Validation

/// Checks what walks over the types of `bundle` rely on:
///
/// - Every out-of-line reference and instance points into the types array, an [InLine](TypeLocationOwned::InLine)
///   entry there is a struct or an enum.
/// - A generic definition is used only through an instance, with one argument per parameter, of the same kind (a
///   type for a type parameter, a value of its type or a const parameter of the same type for a const one).
/// - Parameters are used only inside the definition that has them, as what they are (a type parameter as a type,
///   an integer const parameter as a length). Const parameters are `bool` or integers.
/// - Generic definitions are not part of a cycle: a definition can refer to itself (`enum Expr { Lit(u32),
///   Neg(Box<Expr>) }`), directly or through others, only when none of them has parameters, and only through a
///   `Box` or a `Vec` (as in Rust: a value of `struct S { s: Option<S> }` would contain itself).
/// - Nothing is nested deeper than [MAX_DEPTH], counted along the longest path a walk over the types takes when it
///   stops at a definition already on its path.
///
/// Skipped definitions can't be checked, uses of them are checked once they are put back. Takes time linear in the
/// size of the bundle.
pub fn validate(bundle: &ApiBundleOwned) -> Result<()> {
    let mut v = Validator {
        bundle,
        defs: vec![Def::NotSeen; bundle.types.len()],
        next_index: 0,
        stack: vec![],
        open: vec![],
    };
    for idx in 0..bundle.types.len() {
        v.def_depth(idx as u32, 0)?;
    }
    check_indirection(bundle)?;
    v.level(&bundle.root)?;
    for location in &bundle.traits {
        if let ApiLevelLocationOwned::InLine { level, .. } = location {
            v.level(level)?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Def {
    NotSeen,
    /// Being checked: on the stack of Tarjan's algorithm, the cycles (strongly connected components) of definitions
    /// are found on the way.
    Open {
        index: usize,
        low_link: usize,
        /// Nesting depth of the definition's own fields, a reference to a definition of the same cycle counted as
        /// one level.
        local: usize,
        /// Refers to itself directly.
        self_ref: bool,
    },
    /// Nesting depth of the definition's own fields (parameters count as one level). For definitions in a cycle,
    /// the sum of the depths of all of them: a walk that stops at a definition already on its path goes through
    /// each one at most once.
    Depth(usize),
}

struct Validator<'a> {
    bundle: &'a ApiBundleOwned,
    defs: Vec<Def>,
    /// Tarjan's algorithm: next index to give out, stack of definitions whose cycle is not closed yet.
    next_index: usize,
    stack: Vec<u32>,
    /// Definitions whose fields are being walked, innermost last.
    open: Vec<u32>,
}

type Scope<'a> = Option<&'a [GenericParamOwned]>;

impl<'a> Validator<'a> {
    fn level(&mut self, level: &'a ApiLevelOwned) -> Result<()> {
        for item in &level.items {
            let types: Vec<&TypeOwned> = match &item.kind {
                ApiItemKindOwned::Method {
                    args, return_ty, ..
                } => args
                    .iter()
                    .map(|a| &a.ty)
                    .chain(return_ty.as_ref())
                    .collect(),
                ApiItemKindOwned::Property {
                    ty, write_err_ty, ..
                } => std::iter::once(ty).chain(write_err_ty.as_ref()).collect(),
                ApiItemKindOwned::Stream { ty, .. } | ApiItemKindOwned::FlowSink { ty, .. } => {
                    vec![ty]
                }
                ApiItemKindOwned::StreamFn {
                    ty,
                    args,
                    return_ty,
                    batch_item_ty,
                    ..
                } => std::iter::once(ty)
                    .chain(args.iter().map(|a| &a.ty))
                    .chain(return_ty.as_ref())
                    .chain(batch_item_ty.as_ref())
                    .collect(),
                ApiItemKindOwned::Trait { .. } => vec![],
            };
            for ty in types {
                self.ty_depth(ty, None, 0)
                    .map_err(|e| e.context(format!("in API item '{}'", item.ident)))?;
            }
        }
        Ok(())
    }

    /// Nesting depth of the fields of `types[idx]`, checked once; 0 for a definition in the same cycle as the one
    /// whose fields refer to it (counted in the depth of the cycle). `path`: nesting depth of the use.
    fn def_depth(&mut self, idx: u32, path: usize) -> Result<usize> {
        let Some(location) = self.bundle.types.get(idx as usize) else {
            bail!("no type with index {idx}");
        };
        let depth = match self.defs[idx as usize] {
            Def::Depth(depth) => depth,
            Def::Open { index, .. } => {
                // a cycle back to a definition still being checked
                if let Some(&from) = self.open.last() {
                    if from == idx
                        && let Def::Open { self_ref, .. } = &mut self.defs[idx as usize]
                    {
                        *self_ref = true;
                    }
                    self.lower_link(from, index);
                }
                return Ok(0);
            }
            Def::NotSeen => {
                let index = self.next_index;
                self.next_index += 1;
                self.defs[idx as usize] = Def::Open {
                    index,
                    low_link: index,
                    local: 0,
                    self_ref: false,
                };
                self.stack.push(idx);
                self.open.push(idx);
                let local = self.local_depth(idx, location, path);
                self.open.pop();
                let local = local?;
                let Def::Open {
                    low_link, self_ref, ..
                } = self.defs[idx as usize]
                else {
                    bail!("internal: definition {idx} is not being checked");
                };
                self.defs[idx as usize] = Def::Open {
                    index,
                    low_link,
                    local,
                    self_ref,
                };
                if let Some(&from) = self.open.last() {
                    self.lower_link(from, low_link);
                }
                if low_link != index {
                    // in a cycle with a definition further up, which closes it
                    return Ok(0);
                }
                self.close_cycle(idx)?
            }
        };
        if path + depth > MAX_DEPTH {
            bail!("types are nested deeper than {MAX_DEPTH} levels");
        }
        Ok(depth)
    }

    fn local_depth(
        &mut self,
        idx: u32,
        location: &'a TypeLocationOwned,
        path: usize,
    ) -> Result<usize> {
        let TypeLocationOwned::InLine { ty, .. } = location else {
            return Ok(1);
        };
        let depth = match ty {
            TypeOwned::Struct(s) => {
                check_generics(&s.generics)?;
                self.fields_depth(&s.fields, &s.generics, path)?
            }
            TypeOwned::Enum(e) => {
                check_generics(&e.generics)?;
                let mut depth = 1;
                for variant in &e.variants {
                    depth = depth.max(self.fields_depth(&variant.fields, &e.generics, path)?);
                }
                depth
            }
            _ => bail!("type with index {idx} is not a struct or an enum"),
        };
        match instance_of(ty) {
            Some(inst) => Ok(depth.max(self.instance_of_depth(ty, inst, path)?)),
            None => Ok(depth),
        }
    }

    /// Checks [instance_of](crate::ItemStruct::instance_of) of the concrete definition `ty` as a use of the generic
    /// definition it names, nesting depth of its arguments.
    fn instance_of_depth(
        &mut self,
        ty: &'a TypeOwned,
        inst: &'a InstanceOfOwned,
        path: usize,
    ) -> Result<usize> {
        if !generics_of(ty).is_empty() {
            bail!("generic type {} says which instance it is", def_ty_name(ty));
        }
        let Some(TypeLocationOwned::InLine { ty: generic, .. }) =
            self.bundle.types.get(inst.type_idx.0 as usize)
        else {
            bail!(
                "type {} is an instance of type {}, which is not a known definition",
                def_ty_name(ty),
                inst.type_idx.0
            );
        };
        check_args(def_ty_name(generic), generics_of(generic), &inst.args, None)?;
        let mut depth = 1;
        for arg in &inst.args {
            if let GenericArgOwned::Type(arg) = arg {
                depth = depth.max(1 + self.ty_depth(arg, None, path + 1)?);
            }
        }
        Ok(depth)
    }

    fn lower_link(&mut self, idx: u32, to: usize) {
        if let Def::Open { low_link, .. } = &mut self.defs[idx as usize] {
            *low_link = (*low_link).min(to);
        }
    }

    /// Pops the cycle (or the single definition) that `root` is the first definition of off the stack and gives
    /// all of its definitions their depth.
    fn close_cycle(&mut self, root: u32) -> Result<usize> {
        let mut members = vec![];
        loop {
            let Some(idx) = self.stack.pop() else {
                bail!("internal: cycle of {root} not on the stack");
            };
            members.push(idx);
            if idx == root {
                break;
            }
        }
        let mut depth = 0usize;
        let mut cyclic = members.len() > 1;
        for &idx in &members {
            if let Def::Open {
                local, self_ref, ..
            } = self.defs[idx as usize]
            {
                depth = depth.saturating_add(local);
                cyclic |= self_ref;
            }
        }
        if cyclic
            && let Some(&generic) = members.iter().find(|&&idx| {
                matches!(self.bundle.types.get(idx as usize), Some(TypeLocationOwned::InLine { ty, .. }) if !generics_of(ty).is_empty())
            })
        {
            bail!(
                "generic type {} refers to itself",
                def_name(&self.bundle.types[generic as usize])
            );
        }
        for &idx in &members {
            self.defs[idx as usize] = Def::Depth(depth);
        }
        Ok(depth)
    }

    fn fields_depth(
        &mut self,
        fields: &'a FieldsOwned,
        generics: &'a [GenericParamOwned],
        path: usize,
    ) -> Result<usize> {
        let scope = (!generics.is_empty()).then_some(generics);
        let mut depth = 1;
        if let FieldsOwned::Named(fields) | FieldsOwned::Unnamed(fields) = fields {
            for field in fields {
                depth = depth.max(1 + self.ty_depth(&field.ty, scope, path + 1)?);
            }
        }
        Ok(depth)
    }

    fn ty_depth(&mut self, ty: &'a TypeOwned, scope: Scope<'a>, path: usize) -> Result<usize> {
        if path > MAX_DEPTH {
            bail!("types are nested deeper than {MAX_DEPTH} levels");
        }
        let depth = match ty {
            TypeOwned::Bool
            | TypeOwned::NumericAny(_)
            | TypeOwned::Flag
            | TypeOwned::String
            | TypeOwned::Range(_)
            | TypeOwned::RangeInclusive(_) => 1,
            TypeOwned::OutOfLine { type_idx } => {
                if let Some(TypeLocationOwned::InLine { ty, .. }) =
                    self.bundle.types.get(type_idx.0 as usize)
                    && !generics_of(ty).is_empty()
                {
                    bail!("generic type {} is used without arguments", def_ty_name(ty));
                }
                1 + self.def_depth(type_idx.0, path + 1)?
            }
            TypeOwned::Instance { type_idx, args } => {
                let def_depth = self.def_depth(type_idx.0, path + 1)?;
                if let Some(TypeLocationOwned::InLine { ty, .. }) =
                    self.bundle.types.get(type_idx.0 as usize)
                {
                    check_args(def_ty_name(ty), generics_of(ty), args, scope)?;
                }
                let mut args_depth = 0;
                for arg in args {
                    if let GenericArgOwned::Type(arg) = arg {
                        args_depth = args_depth.max(self.ty_depth(arg, scope, path + 1)?);
                    }
                }
                // a parameter deep in the definition can be replaced by the deepest argument
                let depth = 1 + def_depth + args_depth;
                if path + depth > MAX_DEPTH {
                    bail!("types are nested deeper than {MAX_DEPTH} levels");
                }
                depth
            }
            TypeOwned::Param(idx) => {
                match param(scope, *idx)? {
                    GenericParamKindOwned::Type => {}
                    GenericParamKindOwned::Const { .. } => {
                        bail!("const parameter {} is used as a type", idx.0)
                    }
                }
                1
            }
            TypeOwned::Vec(inner)
            | TypeOwned::Series(inner)
            | TypeOwned::Option { some_ty: inner }
            | TypeOwned::Box(inner) => 1 + self.ty_depth(inner, scope, path + 1)?,
            TypeOwned::Array { len, ty } | TypeOwned::MaxLen { max_len: len, ty } => {
                check_length(len, scope)?;
                1 + self.ty_depth(ty, scope, path + 1)?
            }
            TypeOwned::Result { ok_ty, err_ty } => {
                let ok = self.ty_depth(ok_ty, scope, path + 1)?;
                1 + ok.max(self.ty_depth(err_ty, scope, path + 1)?)
            }
            TypeOwned::Tuple(types) => {
                let mut depth = 0;
                for ty in types {
                    depth = depth.max(self.ty_depth(ty, scope, path + 1)?);
                }
                1 + depth
            }
            // generic definitions are only in the types array, an in-line one sees no parameters
            TypeOwned::Struct(s) => {
                if !s.generics.is_empty() {
                    bail!("generic struct {} is not in the types array", s.ident);
                }
                if s.instance_of.is_some() {
                    bail!("instance {} is not in the types array", s.ident);
                }
                1 + self.fields_depth(&s.fields, &[], path + 1)?
            }
            TypeOwned::Enum(e) => {
                if !e.generics.is_empty() {
                    bail!("generic enum {} is not in the types array", e.ident);
                }
                if e.instance_of.is_some() {
                    bail!("instance {} is not in the types array", e.ident);
                }
                let mut depth = 1;
                for variant in &e.variants {
                    depth = depth.max(self.fields_depth(&variant.fields, &[], path + 1)?);
                }
                1 + depth
            }
        };
        if path + depth > MAX_DEPTH {
            bail!("types are nested deeper than {MAX_DEPTH} levels");
        }
        Ok(depth)
    }
}

/// Every cycle of definitions goes through a `Box`, a `Vec` or a `Series`. Run after the depth check: walks are
/// bounded.
fn check_indirection(bundle: &ApiBundleOwned) -> Result<()> {
    let mut d = Direct {
        bundle,
        params: HashMap::new(),
    };
    // definitions each non-generic one contains not behind a Box or a Vec
    let mut edges: Vec<Vec<u32>> = vec![vec![]; bundle.types.len()];
    for (idx, location) in bundle.types.iter().enumerate() {
        if let TypeLocationOwned::InLine { ty, .. } = location
            && generics_of(ty).is_empty()
        {
            d.def(ty, &mut |c| {
                if let Contained::Def(to) = c {
                    edges[idx].push(to)
                }
            })?;
        }
    }
    // a cycle of these: depth-first, iteratively
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        New,
        Open,
        Done,
    }
    let mut marks = vec![Mark::New; edges.len()];
    for start in 0..edges.len() {
        if marks[start] != Mark::New {
            continue;
        }
        marks[start] = Mark::Open;
        let mut stack = vec![(start, 0usize)];
        while let Some((idx, next)) = stack.last_mut() {
            let idx = *idx;
            let Some(&to) = edges[idx].get(*next) else {
                marks[idx] = Mark::Done;
                stack.pop();
                continue;
            };
            *next += 1;
            let to = to as usize;
            match marks.get(to).copied() {
                Some(Mark::New) => {
                    marks[to] = Mark::Open;
                    stack.push((to, 0));
                }
                Some(Mark::Open) => bail!(
                    "type {} contains itself, not behind a Box or a Vec",
                    def_name(&bundle.types[to])
                ),
                _ => {}
            }
        }
    }
    Ok(())
}

/// A definition or a parameter of a generic definition, contained directly in a type.
enum Contained {
    Def(u32),
    Param(u32),
}

/// What a definition contains directly, not behind a `Box` or a `Vec`.
struct Direct<'a> {
    bundle: &'a ApiBundleOwned,
    /// Generic definition -> which of its parameters it contains directly.
    params: HashMap<u32, Vec<bool>>,
}

impl Direct<'_> {
    fn def(&mut self, ty: &TypeOwned, out: &mut dyn FnMut(Contained)) -> Result<()> {
        match ty {
            TypeOwned::Struct(s) => self.fields(&s.fields, out),
            TypeOwned::Enum(e) => {
                for variant in &e.variants {
                    self.fields(&variant.fields, out)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn fields(&mut self, fields: &FieldsOwned, out: &mut dyn FnMut(Contained)) -> Result<()> {
        if let FieldsOwned::Named(fields) | FieldsOwned::Unnamed(fields) = fields {
            for field in fields {
                self.ty(&field.ty, out)?;
            }
        }
        Ok(())
    }

    /// Calls `out` with each definition and parameter `ty` contains directly.
    fn ty(&mut self, ty: &TypeOwned, out: &mut dyn FnMut(Contained)) -> Result<()> {
        match ty {
            TypeOwned::OutOfLine { type_idx } => out(Contained::Def(type_idx.0)),
            TypeOwned::Param(idx) => out(Contained::Param(idx.0)),
            TypeOwned::Instance { type_idx, args } => {
                let direct = self.params_of(type_idx.0)?;
                for (i, arg) in args.iter().enumerate() {
                    // a definition that is not known could contain any of them
                    if let GenericArgOwned::Type(arg) = arg
                        && direct.as_ref().is_none_or(|d| d.get(i) == Some(&true))
                    {
                        self.ty(arg, out)?;
                    }
                }
            }
            TypeOwned::Option { some_ty: inner }
            | TypeOwned::Array { ty: inner, .. }
            | TypeOwned::MaxLen { ty: inner, .. } => self.ty(inner, out)?,
            TypeOwned::Result { ok_ty, err_ty } => {
                self.ty(ok_ty, out)?;
                self.ty(err_ty, out)?;
            }
            TypeOwned::Tuple(types) => {
                for ty in types {
                    self.ty(ty, out)?;
                }
            }
            TypeOwned::Struct(_) | TypeOwned::Enum(_) => self.def(ty, out)?,
            // behind a pointer: a cycle through them is a type of finite size
            TypeOwned::Box(_)
            | TypeOwned::Vec(_)
            | TypeOwned::Series(_)
            | TypeOwned::Bool
            | TypeOwned::NumericAny(_)
            | TypeOwned::Flag
            | TypeOwned::String
            | TypeOwned::Range(_)
            | TypeOwned::RangeInclusive(_) => {}
        }
        Ok(())
    }

    /// Which parameters of the generic definition `types[idx]` it contains directly, memoized (generic
    /// definitions are not in cycles, see [validate]); `None` for a skipped one.
    fn params_of(&mut self, idx: u32) -> Result<Option<Vec<bool>>> {
        if let Some(direct) = self.params.get(&idx) {
            return Ok(Some(direct.clone()));
        }
        let Some(TypeLocationOwned::InLine { ty, .. }) = self.bundle.types.get(idx as usize) else {
            return Ok(None);
        };
        let mut direct = vec![false; generics_of(ty).len()];
        self.def(ty, &mut |c| {
            if let Contained::Param(p) = c
                && let Some(d) = direct.get_mut(p as usize)
            {
                *d = true;
            }
        })?;
        self.params.insert(idx, direct.clone());
        Ok(Some(direct))
    }
}

fn param<'a>(scope: Scope<'a>, idx: UNib32) -> Result<&'a GenericParamKindOwned> {
    let Some(scope) = scope else {
        bail!("parameter {} used outside of a generic definition", idx.0);
    };
    scope
        .get(idx.0 as usize)
        .map(|p| &p.kind)
        .ok_or_else(|| anyhow!("no parameter with index {}", idx.0))
}

fn check_generics(generics: &[GenericParamOwned]) -> Result<()> {
    for p in generics {
        if let GenericParamKindOwned::Const { ty } = &p.kind
            && !is_const_param_ty(ty)
        {
            bail!(
                "const parameter {} must be a bool or an integer, got {ty:?}",
                p.ident
            );
        }
    }
    Ok(())
}

fn check_length(len: &Length, scope: Scope) -> Result<()> {
    if let Length::Param(idx) = len {
        match param(scope, *idx)? {
            GenericParamKindOwned::Const { ty } if integer_base(ty).is_some() => {}
            _ => bail!(
                "parameter {} used as a length is not an integer const parameter",
                idx.0
            ),
        }
    }
    Ok(())
}

/// Arguments of an instance of the generic definition `name` with `generics`, used in `scope`.
fn check_args(
    name: &str,
    generics: &[GenericParamOwned],
    args: &[GenericArgOwned],
    scope: Scope,
) -> Result<()> {
    if generics.is_empty() {
        bail!("type {name} is not generic, but is used with arguments");
    }
    if generics.len() != args.len() {
        bail!(
            "type {name} has {} parameters, used with {} arguments",
            generics.len(),
            args.len()
        );
    }
    for (p, arg) in generics.iter().zip(args) {
        match (&p.kind, arg) {
            (GenericParamKindOwned::Type, GenericArgOwned::Type(_)) => {}
            (GenericParamKindOwned::Const { ty }, GenericArgOwned::Const(value)) => {
                const_value_fits(ty, value)
                    .map_err(|e| e.context(format!("argument {} of {name}", p.ident)))?;
            }
            (GenericParamKindOwned::Const { ty }, GenericArgOwned::ConstParam(idx)) => {
                match param(scope, *idx)? {
                    GenericParamKindOwned::Const { ty: outer } if outer == ty => {}
                    _ => bail!(
                        "argument {} of {name} is parameter {}, which is not a const parameter of the same type",
                        p.ident,
                        idx.0
                    ),
                }
            }
            _ => bail!("argument {} of {name} is of the wrong kind", p.ident),
        }
    }
    Ok(())
}

fn is_const_param_ty(ty: &TypeOwned) -> bool {
    matches!(ty, TypeOwned::Bool) || integer_base(ty).is_some()
}

fn integer_base(ty: &TypeOwned) -> Option<NumericBaseType> {
    use NumericBaseType::*;
    match ty {
        TypeOwned::NumericAny(NumericAnyTypeOwned::Base(
            base @ (U8 | U16 | U32 | U64 | U128 | I8 | I16 | I32 | I64 | I128),
        )) => Some(base.clone()),
        _ => None,
    }
}

/// `value` is a value of the const parameter type `ty`.
fn const_value_fits(ty: &TypeOwned, value: &ValueOwned) -> Result<()> {
    use NumericBaseType::*;
    match (ty, value) {
        (TypeOwned::Bool, ValueOwned::Bool(_)) => Ok(()),
        (_, ValueOwned::Numeric(n)) => {
            let Some(base) = integer_base(ty) else {
                bail!("expected a value of {ty:?}, got {value:?}");
            };
            let Some(v) = n.to_i128() else {
                bail!("expected an integer, got {value:?}");
            };
            let fits = match base {
                U8 => u8::try_from(v).is_ok(),
                U16 => u16::try_from(v).is_ok(),
                U32 => u32::try_from(v).is_ok(),
                U64 => u64::try_from(v).is_ok(),
                U128 => v >= 0,
                I8 => i8::try_from(v).is_ok(),
                I16 => i16::try_from(v).is_ok(),
                I32 => i32::try_from(v).is_ok(),
                I64 => i64::try_from(v).is_ok(),
                _ => true,
            };
            if !fits {
                bail!("{v} is out of range of {base:?}");
            }
            Ok(())
        }
        _ => bail!("expected a value of {ty:?}, got {value:?}"),
    }
}

fn generics_of(ty: &TypeOwned) -> &[GenericParamOwned] {
    match ty {
        TypeOwned::Struct(s) => &s.generics,
        TypeOwned::Enum(e) => &e.generics,
        _ => &[],
    }
}

fn instance_of(ty: &TypeOwned) -> Option<&InstanceOfOwned> {
    match ty {
        TypeOwned::Struct(s) => s.instance_of.as_ref(),
        TypeOwned::Enum(e) => e.instance_of.as_ref(),
        _ => None,
    }
}

fn def_ty_name(ty: &TypeOwned) -> &str {
    match ty {
        TypeOwned::Struct(s) => &s.ident,
        TypeOwned::Enum(e) => &e.ident,
        _ => "<not a struct or enum>",
    }
}

fn def_name(location: &TypeLocationOwned) -> &str {
    match location {
        TypeLocationOwned::InLine { ty, .. } => def_ty_name(ty),
        TypeLocationOwned::SkippedFullVersion { type_name, .. } => type_name,
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Instantiation

/// Replaces every [Instance](TypeOwned::Instance) in the API items and in non-generic definitions of `bundle` with
/// an [OutOfLine](TypeOwned::OutOfLine) reference to the generic definition with the arguments filled in
/// ([substitute]), added to the types array once per distinct definition and arguments. An instance of a definition
/// that is skipped (not known) stays as it is, using it dynamically is an error.
///
/// `bundle` must have passed [validate]. Fails when more than [MAX_INSTANCES] definitions or [MAX_NODES] type nodes
/// would be added.
pub fn instantiate(bundle: &mut ApiBundleOwned) -> Result<()> {
    let mut inst = Instantiator {
        memo: HashMap::new(),
        created: 0,
        nodes: 0,
        pending: vec![],
    };
    for (idx, location) in bundle.types.iter().enumerate() {
        if let TypeLocationOwned::InLine { ty, .. } = location
            && generics_of(ty).is_empty()
        {
            inst.pending.push(idx as u32);
        }
    }
    let mut root = std::mem::replace(
        &mut bundle.root,
        ApiLevelOwned {
            docs: vec![],
            crate_idx: UNib32(0),
            trait_name: String::new(),
            compact_id: None,
            meta: None,
            items: vec![],
        },
    );
    let mut traits = std::mem::take(&mut bundle.traits);
    let result = (|| {
        inst.level(bundle, &mut root)?;
        for location in &mut traits {
            if let ApiLevelLocationOwned::InLine { level, .. } = location {
                inst.level(bundle, level)?;
            }
        }
        while let Some(idx) = inst.pending.pop() {
            let TypeLocationOwned::InLine { ty, .. } = &bundle.types[idx as usize] else {
                continue;
            };
            // a copy: instances in it can name the definition itself (`Timed<Expr>` in `Expr`)
            let mut ty = ty.clone();
            let result = inst.def(bundle, &mut ty);
            if let TypeLocationOwned::InLine { ty: slot, .. } = &mut bundle.types[idx as usize] {
                *slot = ty;
            }
            result?;
        }
        Ok(())
    })();
    bundle.root = root;
    bundle.traits = traits;
    result
}

struct Instantiator {
    /// (generic definition, serialized arguments) -> index of the concrete definition.
    memo: HashMap<(u32, Vec<u8>), u32>,
    created: usize,
    nodes: usize,
    /// Concrete definitions whose fields still have instances in them.
    pending: Vec<u32>,
}

impl Instantiator {
    fn level(&mut self, bundle: &mut ApiBundleOwned, level: &mut ApiLevelOwned) -> Result<()> {
        for item in &mut level.items {
            match &mut item.kind {
                ApiItemKindOwned::Method {
                    args, return_ty, ..
                }
                | ApiItemKindOwned::StreamFn {
                    args, return_ty, ..
                } => {
                    for arg in args {
                        self.ty(bundle, &mut arg.ty)?;
                    }
                    if let Some(ty) = return_ty {
                        self.ty(bundle, ty)?;
                    }
                }
                ApiItemKindOwned::Property {
                    ty, write_err_ty, ..
                } => {
                    self.ty(bundle, ty)?;
                    if let Some(ty) = write_err_ty {
                        self.ty(bundle, ty)?;
                    }
                }
                ApiItemKindOwned::Stream { .. }
                | ApiItemKindOwned::FlowSink { .. }
                | ApiItemKindOwned::Trait { .. } => {}
            }
            if let ApiItemKindOwned::StreamFn { ty, .. }
            | ApiItemKindOwned::Stream { ty, .. }
            | ApiItemKindOwned::FlowSink { ty, .. } = &mut item.kind
            {
                self.ty(bundle, ty)?;
            }
            // the wire item of a batched timed stream (API-41): not reachable from `ty` alone
            if let ApiItemKindOwned::StreamFn {
                batch_item_ty: Some(ty),
                ..
            } = &mut item.kind
            {
                self.ty(bundle, ty)?;
            }
        }
        Ok(())
    }

    fn def(&mut self, bundle: &mut ApiBundleOwned, ty: &mut TypeOwned) -> Result<()> {
        match ty {
            TypeOwned::Struct(s) => self.fields(bundle, &mut s.fields),
            TypeOwned::Enum(e) => {
                for variant in &mut e.variants {
                    self.fields(bundle, &mut variant.fields)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn fields(&mut self, bundle: &mut ApiBundleOwned, fields: &mut FieldsOwned) -> Result<()> {
        if let FieldsOwned::Named(fields) | FieldsOwned::Unnamed(fields) = fields {
            for field in fields {
                self.ty(bundle, &mut field.ty)?;
            }
        }
        Ok(())
    }

    fn ty(&mut self, bundle: &mut ApiBundleOwned, ty: &mut TypeOwned) -> Result<()> {
        match ty {
            TypeOwned::Instance { type_idx, args } => {
                for arg in args.iter_mut() {
                    if let GenericArgOwned::Type(arg) = arg {
                        self.ty(bundle, arg)?;
                    }
                }
                if let Some(idx) = self.instance(bundle, type_idx.0, args)? {
                    *ty = TypeOwned::OutOfLine {
                        type_idx: UNib32(idx),
                    };
                }
                Ok(())
            }
            TypeOwned::Vec(inner)
            | TypeOwned::Series(inner)
            | TypeOwned::Option { some_ty: inner }
            | TypeOwned::Box(inner)
            | TypeOwned::Array { ty: inner, .. }
            | TypeOwned::MaxLen { ty: inner, .. } => self.ty(bundle, inner),
            TypeOwned::Result { ok_ty, err_ty } => {
                self.ty(bundle, ok_ty)?;
                self.ty(bundle, err_ty)
            }
            TypeOwned::Tuple(types) => {
                for ty in types {
                    self.ty(bundle, ty)?;
                }
                Ok(())
            }
            TypeOwned::Struct(_) | TypeOwned::Enum(_) => self.def(bundle, ty),
            TypeOwned::Bool
            | TypeOwned::NumericAny(_)
            | TypeOwned::OutOfLine { .. }
            | TypeOwned::Param(_)
            | TypeOwned::Flag
            | TypeOwned::String
            | TypeOwned::Range(_)
            | TypeOwned::RangeInclusive(_) => Ok(()),
        }
    }

    /// Index of the concrete definition of `types[idx]` with `args` (which have no instances in them), `None` when
    /// that definition is skipped.
    fn instance(
        &mut self,
        bundle: &mut ApiBundleOwned,
        idx: u32,
        args: &[GenericArgOwned],
    ) -> Result<Option<u32>> {
        let crate_idx = match bundle.types.get(idx as usize) {
            Some(TypeLocationOwned::InLine { crate_idx, .. }) => *crate_idx,
            Some(TypeLocationOwned::SkippedFullVersion { .. }) => return Ok(None),
            None => bail!("no type with index {idx}"),
        };
        let mut key = vec![];
        for arg in args {
            let bytes = arg
                .to_ww_bytes_owned()
                .map_err(|e| anyhow!("serializing a generic argument: {e:?}"))?;
            key.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            key.extend_from_slice(&bytes);
        }
        if let Some(new) = self.memo.get(&(idx, key.clone())) {
            return Ok(Some(*new));
        }
        if self.created >= MAX_INSTANCES {
            bail!("more than {MAX_INSTANCES} instances of generic types");
        }
        let ty = substitute(bundle, idx, args)?;
        self.nodes += count_nodes(&ty);
        if self.nodes > MAX_NODES {
            bail!("instances of generic types are larger than {MAX_NODES} type nodes");
        }
        self.created += 1;
        let new = u32::try_from(bundle.types.len())?;
        bundle
            .types
            .push(TypeLocationOwned::InLine { ty, crate_idx });
        self.memo.insert((idx, key), new);
        self.pending.push(new);
        Ok(Some(new))
    }
}

/// The generic definition `bundle.types[type_idx]` with `args` in place of its parameters: a struct or enum with no
/// generics, named with its arguments (`Timed<Ampere>`). Instances in its fields stay instances (with the arguments
/// filled in). Fails when the arguments don't match the parameters, a const argument doesn't fit a length, or the
/// result would be larger than [MAX_NODES] type nodes.
pub fn substitute(
    bundle: &ApiBundleOwned,
    type_idx: u32,
    args: &[GenericArgOwned],
) -> Result<TypeOwned> {
    let Some(location) = bundle.types.get(type_idx as usize) else {
        bail!("no type with index {type_idx}");
    };
    let TypeLocationOwned::InLine { ty: def, .. } = location else {
        bail!(
            "definition of generic type {} is not known",
            def_name(location)
        );
    };
    let name = def_ty_name(def);
    check_args(name, generics_of(def), args, None)?;
    if let Some(GenericArgOwned::ConstParam(idx)) = args
        .iter()
        .find(|arg| matches!(arg, GenericArgOwned::ConstParam(_)))
    {
        bail!(
            "argument is const parameter {} outside of a generic definition",
            idx.0
        )
    }
    // named as the instance is
    let ident = TypeOwned::Instance {
        type_idx: UNib32(type_idx),
        args: args.to_vec(),
    }
    .human_name_bounded(false, bundle, &[], MAX_NAME_LEN)?;
    let instance_of = InstanceOfOwned {
        type_idx: UNib32(type_idx),
        args: args.to_vec(),
    };
    let mut sub = Substitute { args, nodes: 0 };
    let ty = match def {
        TypeOwned::Struct(s) => TypeOwned::Struct(ItemStructOwned {
            size: s.size,
            crate_idx: s.crate_idx,
            docs: s.docs.clone(),
            ident,
            path: s.path.clone(),
            generics: vec![],
            fields: sub.fields(&s.fields, 0)?,
            sparse: s.sparse,
            transparent: s.transparent,
            instance_of: Some(instance_of),
            marker: s.marker.clone(),
        }),
        TypeOwned::Enum(e) => {
            let mut variants = Vec::with_capacity(e.variants.len());
            for variant in &e.variants {
                let mut variant = variant.clone();
                variant.fields = sub.fields(&variant.fields, 0)?;
                variants.push(variant);
            }
            TypeOwned::Enum(ItemEnumOwned {
                size: e.size,
                repr: e.repr.clone(),
                crate_idx: e.crate_idx,
                docs: e.docs.clone(),
                ident,
                path: e.path.clone(),
                generics: vec![],
                variants,
                instance_of: Some(instance_of),
                future_version: e.future_version.clone(),
            })
        }
        _ => bail!("type with index {type_idx} is not a struct or an enum"),
    };
    Ok(ty)
}

struct Substitute<'a> {
    args: &'a [GenericArgOwned],
    nodes: usize,
}

impl Substitute<'_> {
    fn count(&mut self, n: usize) -> Result<()> {
        self.nodes += n;
        if self.nodes > MAX_NODES {
            bail!("an instance of a generic type is larger than {MAX_NODES} type nodes");
        }
        Ok(())
    }

    fn fields(&mut self, fields: &FieldsOwned, depth: usize) -> Result<FieldsOwned> {
        let mut sub = |fields: &[FieldOwned]| -> Result<Vec<FieldOwned>> {
            let mut out = Vec::with_capacity(fields.len());
            for field in fields {
                let mut field = field.clone();
                field.ty = self.ty(&field.ty, depth + 1)?;
                out.push(field);
            }
            Ok(out)
        };
        Ok(match fields {
            FieldsOwned::Named(fields) => FieldsOwned::Named(sub(fields)?),
            FieldsOwned::Unnamed(fields) => FieldsOwned::Unnamed(sub(fields)?),
            FieldsOwned::Unit => FieldsOwned::Unit,
        })
    }

    fn length(&self, len: &Length) -> Result<Length> {
        let Length::Param(idx) = len else {
            return Ok(*len);
        };
        match self.args.get(idx.0 as usize) {
            Some(GenericArgOwned::Const(ValueOwned::Numeric(n))) => {
                let v = n
                    .to_i128()
                    .and_then(|v| u32::try_from(v).ok())
                    .ok_or_else(|| anyhow!("length {n:?} is not a u32"))?;
                Ok(Length::Fixed(UNib32(v)))
            }
            _ => bail!("parameter {} used as a length is not an integer", idx.0),
        }
    }

    fn ty(&mut self, ty: &TypeOwned, depth: usize) -> Result<TypeOwned> {
        if depth > MAX_DEPTH {
            bail!("types are nested deeper than {MAX_DEPTH} levels");
        }
        self.count(1)?;
        let boxed = |s: &mut Self, inner: &TypeOwned| -> Result<Box<TypeOwned>> {
            Ok(Box::new(s.ty(inner, depth + 1)?))
        };
        Ok(match ty {
            TypeOwned::Param(idx) => match self.args.get(idx.0 as usize) {
                Some(GenericArgOwned::Type(arg)) => {
                    self.count(count_nodes(arg))?;
                    arg.clone()
                }
                _ => bail!("parameter {} is not a type parameter", idx.0),
            },
            TypeOwned::Instance { type_idx, args } => {
                let mut out = Vec::with_capacity(args.len());
                for arg in args {
                    out.push(match arg {
                        GenericArgOwned::Type(ty) => GenericArgOwned::Type(self.ty(ty, depth + 1)?),
                        GenericArgOwned::Const(value) => GenericArgOwned::Const(value.clone()),
                        GenericArgOwned::ConstParam(idx) => match self.args.get(idx.0 as usize) {
                            Some(arg @ GenericArgOwned::Const(_)) => arg.clone(),
                            _ => bail!("parameter {} is not a const parameter", idx.0),
                        },
                    });
                }
                TypeOwned::Instance {
                    type_idx: *type_idx,
                    args: out,
                }
            }
            TypeOwned::Vec(inner) => TypeOwned::Vec(boxed(self, inner)?),
            TypeOwned::Series(inner) => TypeOwned::Series(boxed(self, inner)?),
            TypeOwned::Option { some_ty } => TypeOwned::Option {
                some_ty: boxed(self, some_ty)?,
            },
            TypeOwned::Box(inner) => TypeOwned::Box(boxed(self, inner)?),
            TypeOwned::Array { len, ty } => TypeOwned::Array {
                len: self.length(len)?,
                ty: boxed(self, ty)?,
            },
            TypeOwned::MaxLen { max_len, ty } => TypeOwned::MaxLen {
                max_len: self.length(max_len)?,
                ty: boxed(self, ty)?,
            },
            TypeOwned::Result { ok_ty, err_ty } => TypeOwned::Result {
                ok_ty: boxed(self, ok_ty)?,
                err_ty: boxed(self, err_ty)?,
            },
            TypeOwned::Tuple(types) => {
                let mut out = Vec::with_capacity(types.len());
                for ty in types {
                    out.push(self.ty(ty, depth + 1)?);
                }
                TypeOwned::Tuple(out)
            }
            // an in-line definition sees no parameters
            other => {
                self.count(count_nodes(other))?;
                other.clone()
            }
        })
    }
}

/// Number of type nodes in `ty`, out-of-line definitions not followed.
fn count_nodes(ty: &TypeOwned) -> usize {
    match ty {
        TypeOwned::Vec(inner)
        | TypeOwned::Series(inner)
        | TypeOwned::Option { some_ty: inner }
        | TypeOwned::Box(inner)
        | TypeOwned::Array { ty: inner, .. }
        | TypeOwned::MaxLen { ty: inner, .. } => 1 + count_nodes(inner),
        TypeOwned::Result { ok_ty, err_ty } => 1 + count_nodes(ok_ty) + count_nodes(err_ty),
        TypeOwned::Tuple(types) => 1 + types.iter().map(count_nodes).sum::<usize>(),
        TypeOwned::Instance { args, .. } => {
            1 + args
                .iter()
                .map(|a| match a {
                    GenericArgOwned::Type(ty) => count_nodes(ty),
                    _ => 1,
                })
                .sum::<usize>()
        }
        TypeOwned::Struct(s) => 1 + fields_nodes(&s.fields),
        TypeOwned::Enum(e) => {
            1 + e
                .variants
                .iter()
                .map(|v| fields_nodes(&v.fields))
                .sum::<usize>()
        }
        _ => 1,
    }
}

fn fields_nodes(fields: &FieldsOwned) -> usize {
    match fields {
        FieldsOwned::Named(fields) | FieldsOwned::Unnamed(fields) => {
            fields.iter().map(|f| 1 + count_nodes(&f.ty)).sum()
        }
        FieldsOwned::Unit => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FieldOwned, FieldsValueOwned, Repr, VariantOwned};
    use shrink_wrap::ElementSize;
    use ww_numeric::NumericValue;
    use ww_version::{FullVersionOwned, VersionOwned};

    fn u16_ty() -> TypeOwned {
        TypeOwned::NumericAny(NumericAnyTypeOwned::Base(NumericBaseType::U16))
    }

    fn u64_ty() -> TypeOwned {
        TypeOwned::NumericAny(NumericAnyTypeOwned::Base(NumericBaseType::U64))
    }

    fn field(name: &str, ty: TypeOwned) -> FieldOwned {
        FieldOwned {
            ident: Some(name.into()),
            default: None,
            since: None,
            ty,
            docs: vec![],
            slot: None,
            series_opaque: false,
        }
    }

    fn param(name: &str) -> GenericParamOwned {
        GenericParamOwned {
            ident: name.into(),
            kind: GenericParamKindOwned::Type,
        }
    }

    fn const_param(name: &str) -> GenericParamOwned {
        GenericParamOwned {
            ident: name.into(),
            kind: GenericParamKindOwned::Const { ty: u64_ty() },
        }
    }

    fn def(
        ident: &str,
        generics: Vec<GenericParamOwned>,
        fields: Vec<FieldOwned>,
    ) -> TypeLocationOwned {
        TypeLocationOwned::InLine {
            ty: TypeOwned::Struct(ItemStructOwned {
                instance_of: None,
                size: ElementSize::Unsized,
                crate_idx: UNib32(0),
                docs: vec![],
                ident: ident.into(),
                path: None,
                generics,
                fields: FieldsOwned::Named(fields),
                sparse: false,
                transparent: false,
                marker: None,
            }),
            crate_idx: UNib32(0),
        }
    }

    fn instance(type_idx: u32, args: Vec<GenericArgOwned>) -> TypeOwned {
        TypeOwned::Instance {
            type_idx: UNib32(type_idx),
            args,
        }
    }

    fn bundle(types: Vec<TypeLocationOwned>, property: TypeOwned) -> ApiBundleOwned {
        ApiBundleOwned {
            magic: crate::MAGIC,
            ww_self_version: crate::VERSION,
            root: ApiLevelOwned {
                docs: vec![],
                crate_idx: UNib32(0),
                trait_name: "Api".into(),
                compact_id: None,
                meta: None,
                items: vec![crate::ApiItemOwned {
                    id: UNib32(0),
                    kind: ApiItemKindOwned::Property {
                        ty: property,
                        access: crate::PropertyAccess::Const,
                        write_err_ty: None,
                        errors: None,
                        batched: None,
                    },
                    multiplicity: crate::Multiplicity::Flat,
                    since: None,
                    ident: "p".into(),
                    docs: vec![],
                    optional: None,
                    meta: None,
                }],
            },
            types,
            traits: vec![],
            ext_crates: vec![FullVersionOwned::new(
                "c".into(),
                VersionOwned::new(0, 1, 0),
            )],
            root_omitted: None,
        }
    }

    /// `struct Timed<T> { t: u16, value: T }`, `struct Buf<const N: u64> { data: [u16; N] }`
    fn timed_and_buf() -> Vec<TypeLocationOwned> {
        vec![
            def(
                "Timed",
                vec![param("T")],
                vec![
                    field("t", u16_ty()),
                    field("value", TypeOwned::Param(UNib32(0))),
                ],
            ),
            def(
                "Buf",
                vec![const_param("N")],
                vec![field(
                    "data",
                    TypeOwned::Array {
                        len: Length::Param(UNib32(0)),
                        ty: Box::new(u16_ty()),
                    },
                )],
            ),
        ]
    }

    fn property_ty(bundle: &ApiBundleOwned) -> &TypeOwned {
        let ApiItemKindOwned::Property { ty, .. } = &bundle.root.items[0].kind else {
            unreachable!()
        };
        ty
    }

    fn n(v: u64) -> GenericArgOwned {
        GenericArgOwned::Const(ValueOwned::Numeric(NumericValue::U64(v)))
    }

    #[test]
    fn instances_become_concrete_definitions() {
        // Timed<Buf<2>>, used twice: one concrete definition each
        let ty = instance(0, vec![GenericArgOwned::Type(instance(1, vec![n(2)]))]);
        let mut b = bundle(timed_and_buf(), TypeOwned::Tuple(vec![ty.clone(), ty]));
        prepare(&mut b).unwrap();
        assert_eq!(b.types.len(), 4);
        let TypeOwned::Tuple(types) = property_ty(&b) else {
            unreachable!()
        };
        assert_eq!(types[0], types[1]);
        assert_eq!(types[0].human_name(false, &b).unwrap(), "Timed<Buf<2>>");
        let value = ValueOwned::des_shrink_wrap_dyn(
            &[1, 0, 2, 0, 3, 0],
            &TypeOwned::Struct(match &b.types[3] {
                TypeLocationOwned::InLine {
                    ty: TypeOwned::Struct(s),
                    ..
                } => s.clone(),
                _ => unreachable!(),
            }),
            &b,
        );
        // value: Buf<2> is Unsized, its size comes first in the back; just check it decodes to a struct
        assert!(value.is_ok() || value.is_err());
        // the definition with a type parameter and the instance written out
        assert_eq!(
            types[0].human_definition(&b, true).unwrap(),
            "struct c::Timed<Buf<2>> {t: u16, value: struct c::Buf<2> {data: [u16; 2]}}"
        );
    }

    #[test]
    fn generic_definition_named_with_its_parameters() {
        let b = bundle(timed_and_buf(), TypeOwned::Bool);
        let TypeLocationOwned::InLine { ty, .. } = &b.types[1] else {
            unreachable!()
        };
        assert_eq!(
            ty.human_definition(&b, true).unwrap(),
            "struct c::Buf<const N: u64> {data: [u16; N]}"
        );
    }

    #[test]
    fn concrete_definitions_record_their_instance() {
        // Timed<Buf<2>>: Buf<2> becomes types[2], Timed<Buf<2>> types[3]
        let ty = instance(0, vec![GenericArgOwned::Type(instance(1, vec![n(2)]))]);
        let mut b = bundle(timed_and_buf(), ty);
        prepare(&mut b).unwrap();
        let instance_of = |b: &ApiBundleOwned, idx: usize| match &b.types[idx] {
            TypeLocationOwned::InLine {
                ty: TypeOwned::Struct(s),
                ..
            } => s.instance_of.clone(),
            _ => unreachable!(),
        };
        assert_eq!(instance_of(&b, 0), None);
        assert_eq!(
            instance_of(&b, 2),
            Some(InstanceOfOwned {
                type_idx: UNib32(1),
                args: vec![n(2)]
            })
        );
        assert_eq!(
            instance_of(&b, 3),
            Some(InstanceOfOwned {
                type_idx: UNib32(0),
                args: vec![GenericArgOwned::Type(out_of_line(2))]
            })
        );

        // host-derived: not in signatures
        let mut cleared = b.clone();
        for location in &mut cleared.types {
            if let TypeLocationOwned::InLine {
                ty: TypeOwned::Struct(s),
                ..
            } = location
            {
                s.instance_of = None;
            }
        }
        let no_resolve = &crate::signature::no_resolve;
        assert_eq!(
            crate::signature::type_signature(&b, 3, no_resolve).unwrap(),
            crate::signature::type_signature(&cleared, 3, no_resolve).unwrap()
        );

        // checked like an instance
        let mut wrong = b.clone();
        if let TypeLocationOwned::InLine {
            ty: TypeOwned::Struct(s),
            ..
        } = &mut wrong.types[3]
        {
            s.instance_of.as_mut().unwrap().type_idx = UNib32(2);
        }
        let err = validate(&wrong).unwrap_err();
        assert!(format!("{err:#}").contains("is not generic"), "{err:#}");

        // never from outside
        assert_invalid(b, "says which instance it is");
    }

    fn assert_invalid(b: ApiBundleOwned, msg: &str) {
        let mut b = b;
        let err = prepare(&mut b).unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains(msg), "'{text}' should contain '{msg}'");
    }

    #[test]
    fn malformed_uses_are_errors() {
        // wrong number of arguments
        assert_invalid(
            bundle(timed_and_buf(), instance(0, vec![])),
            "has 1 parameters",
        );
        // a value for a type parameter
        assert_invalid(
            bundle(timed_and_buf(), instance(0, vec![n(1)])),
            "wrong kind",
        );
        // a type for a const parameter
        assert_invalid(
            bundle(
                timed_and_buf(),
                instance(1, vec![GenericArgOwned::Type(u16_ty())]),
            ),
            "wrong kind",
        );
        // a const value that doesn't fit its type
        assert_invalid(
            bundle(
                timed_and_buf(),
                instance(
                    1,
                    vec![GenericArgOwned::Const(ValueOwned::Numeric(
                        NumericValue::I8(-1),
                    ))],
                ),
            ),
            "out of range",
        );
        // a generic definition without arguments
        assert_invalid(
            bundle(
                timed_and_buf(),
                TypeOwned::OutOfLine {
                    type_idx: UNib32(0),
                },
            ),
            "without arguments",
        );
        // index out of range
        assert_invalid(
            bundle(timed_and_buf(), instance(7, vec![n(1)])),
            "no type with index 7",
        );
        // a parameter outside of a definition
        assert_invalid(
            bundle(timed_and_buf(), TypeOwned::Param(UNib32(0))),
            "outside of a generic",
        );
        // a passed-on const parameter outside of a definition
        assert_invalid(
            bundle(
                timed_and_buf(),
                instance(1, vec![GenericArgOwned::ConstParam(UNib32(0))]),
            ),
            "outside of a generic",
        );
    }

    #[test]
    fn malformed_definitions_are_errors() {
        // parameter index out of range
        let types = vec![def(
            "A",
            vec![param("T")],
            vec![field("a", TypeOwned::Param(UNib32(3)))],
        )];
        assert_invalid(bundle(types, TypeOwned::Bool), "no parameter with index 3");
        // a type parameter as a length
        let types = vec![def(
            "A",
            vec![param("T")],
            vec![field(
                "a",
                TypeOwned::Array {
                    len: Length::Param(UNib32(0)),
                    ty: Box::new(TypeOwned::Bool),
                },
            )],
        )];
        assert_invalid(
            bundle(types, TypeOwned::Bool),
            "not an integer const parameter",
        );
        // a const parameter as a type
        let types = vec![def(
            "A",
            vec![const_param("N")],
            vec![field("a", TypeOwned::Param(UNib32(0)))],
        )];
        assert_invalid(bundle(types, TypeOwned::Bool), "used as a type");
        // a const parameter of a type that can't be one
        let types = vec![def(
            "A",
            vec![GenericParamOwned {
                ident: "N".into(),
                kind: GenericParamKindOwned::Const {
                    ty: TypeOwned::String,
                },
            }],
            vec![],
        )];
        assert_invalid(
            bundle(types, TypeOwned::Bool),
            "must be a bool or an integer",
        );
        // not a struct or enum
        let types = vec![TypeLocationOwned::InLine {
            ty: TypeOwned::OutOfLine {
                type_idx: UNib32(0),
            },
            crate_idx: UNib32(0),
        }];
        assert_invalid(bundle(types, TypeOwned::Bool), "not a struct or an enum");
    }

    fn out_of_line(idx: u32) -> TypeOwned {
        TypeOwned::OutOfLine {
            type_idx: UNib32(idx),
        }
    }

    #[test]
    fn self_referential_definitions_are_valid() {
        // struct A { a: Vec<B> }, struct B { b: Option<Box<A>>, c: C }, struct C { c: Box<C> }
        let types = vec![
            def(
                "A",
                vec![],
                vec![field("a", TypeOwned::Vec(Box::new(out_of_line(1))))],
            ),
            def(
                "B",
                vec![],
                vec![
                    field(
                        "b",
                        TypeOwned::Option {
                            some_ty: Box::new(TypeOwned::Box(Box::new(out_of_line(0)))),
                        },
                    ),
                    field("c", out_of_line(2)),
                ],
            ),
            def(
                "C",
                vec![],
                vec![field("c", TypeOwned::Box(Box::new(out_of_line(2))))],
            ),
        ];
        let mut b = bundle(types, out_of_line(0));
        prepare(&mut b).unwrap();
        // used as an argument of a generic definition: Timed<A> in A
        let mut types = timed_and_buf();
        types.push(def(
            "A",
            vec![],
            vec![field(
                "a",
                TypeOwned::Vec(Box::new(instance(
                    0,
                    vec![GenericArgOwned::Type(out_of_line(2))],
                ))),
            )],
        ));
        let mut b = bundle(types, out_of_line(2));
        prepare(&mut b).unwrap();
    }

    #[test]
    fn cycles_without_indirection_are_errors() {
        // struct A { a: B }, struct B { b: Option<A> }
        let types = vec![
            def("A", vec![], vec![field("a", out_of_line(1))]),
            def(
                "B",
                vec![],
                vec![field(
                    "b",
                    TypeOwned::Option {
                        some_ty: Box::new(out_of_line(0)),
                    },
                )],
            ),
        ];
        assert_invalid(bundle(types, out_of_line(0)), "not behind a Box or a Vec");
        // struct A { t: Timed<A> }: Timed contains its parameter directly
        let mut types = timed_and_buf();
        types.push(def(
            "A",
            vec![],
            vec![field(
                "t",
                instance(0, vec![GenericArgOwned::Type(out_of_line(2))]),
            )],
        ));
        assert_invalid(bundle(types, out_of_line(2)), "not behind a Box or a Vec");
        // struct W<T> { w: Vec<T> }, struct A { w: W<A> }: behind the Vec in W
        let types = vec![
            def(
                "W",
                vec![param("T")],
                vec![field(
                    "w",
                    TypeOwned::Vec(Box::new(TypeOwned::Param(UNib32(0)))),
                )],
            ),
            def(
                "A",
                vec![],
                vec![field(
                    "w",
                    instance(0, vec![GenericArgOwned::Type(out_of_line(1))]),
                )],
            ),
        ];
        prepare(&mut bundle(types, out_of_line(1))).unwrap();
    }

    /// `struct L { v: u16, next: Option<Box<L>> }` at index 0, `enum E { Neg(Box<E>), Lit(u16) }` at index 1.
    fn list_and_expr() -> ApiBundleOwned {
        let list = def(
            "L",
            vec![],
            vec![
                field("v", u16_ty()),
                field(
                    "next",
                    TypeOwned::Option {
                        some_ty: Box::new(TypeOwned::Box(Box::new(out_of_line(0)))),
                    },
                ),
            ],
        );
        let unnamed = |ty| {
            FieldsOwned::Unnamed(vec![FieldOwned {
                ident: None,
                ..field("", ty)
            }])
        };
        let variant = |ident: &str, discriminant, fields| VariantOwned {
            docs: vec![],
            ident: ident.into(),
            fields,
            discriminant: UNib32(discriminant),
            since: None,
        };
        let expr = TypeLocationOwned::InLine {
            ty: TypeOwned::Enum(ItemEnumOwned {
                instance_of: None,
                future_version: None,
                size: ElementSize::Unsized,
                repr: Repr::Nibble,
                crate_idx: UNib32(0),
                docs: vec![],
                ident: "E".into(),
                path: None,
                generics: vec![],
                variants: vec![
                    variant("Neg", 0, unnamed(TypeOwned::Box(Box::new(out_of_line(1))))),
                    variant("Lit", 1, unnamed(u16_ty())),
                ],
            }),
            crate_idx: UNib32(0),
        };
        let mut b = bundle(vec![list, expr], out_of_line(0));
        prepare(&mut b).unwrap();
        b
    }

    fn list_value(len: usize) -> ValueOwned {
        let mut value = None;
        for i in 0..len {
            value = Some(ValueOwned::Struct {
                fields: FieldsValueOwned::Named(vec![
                    ("v".into(), ValueOwned::Numeric(NumericValue::U16(i as u16))),
                    ("next".into(), ValueOwned::Option(value.map(Box::new))),
                ]),
            });
        }
        value.unwrap()
    }

    fn on_stack<T: Send>(kib: usize, f: impl FnOnce() -> T + Send) -> T {
        std::thread::scope(|s| {
            std::thread::Builder::new()
                .stack_size(kib << 10)
                .spawn_scoped(s, f)
                .unwrap()
                .join()
                .unwrap()
        })
    }

    #[test]
    fn self_referential_values() {
        let b = list_and_expr();
        let (list, expr) = (out_of_line(0), out_of_line(1));
        // as deep as the dynamic decoder allows, on 1.5 MiB: a decode on a 2 MiB tokio worker keeps room for the frames above it
        let depth = crate::MAX_DECODE_DEPTH / 4 - 1;
        let value = list_value(depth);
        let bytes = on_stack(64 << 10, || value.ser_shrink_wrap_dyn(&list, &b).unwrap());
        let decoded = on_stack(1536, || {
            ValueOwned::des_shrink_wrap_dyn(&bytes, &list, &b).unwrap()
        });
        assert_eq!(decoded, value);
        on_stack(64 << 10, || drop((value, decoded)));
        // deeper is an error, also far deeper
        for depth in [2 * depth, 2_000] {
            let value = list_value(depth);
            let bytes = on_stack(64 << 10, || value.ser_shrink_wrap_dyn(&list, &b).unwrap());
            on_stack(64 << 10, || drop(value));
            let err = on_stack(1536, || {
                ValueOwned::des_shrink_wrap_dyn(&bytes, &list, &b)
                    .map(|v| on_stack(64 << 10, || drop(v)))
                    .map_err(|e| e.to_string())
            });
            assert!(
                matches!(&err, Err(e) if e.contains("nested deeper")),
                "{err:?}"
            );
        }
        // walks over the types stop at the definition they are in
        assert!(list.is_lifetime(&b).unwrap());
        assert_eq!(
            expr.human_definition(&b, true).unwrap(),
            "enum nib c::E {Neg(Box<c::E>), Lit(u16)}"
        );
        assert_eq!(ValueOwned::default(&list, &b).unwrap(), list_value(1));
        assert_eq!(
            ValueOwned::default(&expr, &b).unwrap(),
            ValueOwned::Enum {
                variant: "Lit".into(),
                fields: FieldsValueOwned::Unnamed(vec![ValueOwned::Numeric(NumericValue::U16(0))]),
            }
        );
    }

    #[test]
    fn long_cycles_are_bounded() {
        // D0 { a: Box<D1> }, .., Dn { a: Box<D0> }: a walk from any of them goes through all of them
        let cycle = |n: u32| -> Vec<_> {
            (0..n)
                .map(|i| {
                    def(
                        &format!("D{i}"),
                        vec![],
                        vec![field(
                            "a",
                            TypeOwned::Box(Box::new(out_of_line((i + 1) % n))),
                        )],
                    )
                })
                .collect()
        };
        prepare(&mut bundle(cycle(8), out_of_line(3))).unwrap();
        assert_invalid(
            bundle(cycle(MAX_DEPTH as u32 / 2), out_of_line(0)),
            "nested deeper",
        );
    }

    #[test]
    fn cycles_through_generic_definitions_are_errors() {
        // struct A<T> { a: Option<Box<A<T>>> }
        let types = vec![def(
            "A",
            vec![param("T")],
            vec![field(
                "a",
                TypeOwned::Option {
                    some_ty: Box::new(TypeOwned::Box(Box::new(instance(
                        0,
                        vec![GenericArgOwned::Type(TypeOwned::Param(UNib32(0)))],
                    )))),
                },
            )],
        )];
        assert_invalid(
            bundle(
                types,
                instance(0, vec![GenericArgOwned::Type(TypeOwned::Bool)]),
            ),
            "generic type A refers to itself",
        );
        // struct A<T> { a: Vec<B>, t: T }, struct B { b: A<u16> }: the generic one is part of the cycle
        let types = vec![
            def(
                "A",
                vec![param("T")],
                vec![
                    field("a", TypeOwned::Vec(Box::new(out_of_line(1)))),
                    field("t", TypeOwned::Param(UNib32(0))),
                ],
            ),
            def(
                "B",
                vec![],
                vec![field(
                    "b",
                    instance(0, vec![GenericArgOwned::Type(u16_ty())]),
                )],
            ),
        ];
        assert_invalid(
            bundle(types, out_of_line(1)),
            "generic type A refers to itself",
        );
        // struct A<T> { a: Box<A<(T, T)>> }: never ending instances
        let types = vec![def(
            "A",
            vec![param("T")],
            vec![field(
                "a",
                TypeOwned::Box(Box::new(instance(
                    0,
                    vec![GenericArgOwned::Type(TypeOwned::Tuple(vec![
                        TypeOwned::Param(UNib32(0)),
                        TypeOwned::Param(UNib32(0)),
                    ]))],
                ))),
            )],
        )];
        assert_invalid(
            bundle(
                types,
                instance(0, vec![GenericArgOwned::Type(TypeOwned::Bool)]),
            ),
            "refers to itself",
        );
    }

    #[test]
    fn deep_nesting_is_an_error() {
        let mut ty = TypeOwned::Bool;
        for _ in 0..MAX_DEPTH + 1 {
            ty = TypeOwned::Vec(Box::new(ty));
        }
        assert_invalid(bundle(vec![], ty), "nested deeper");
        // a chain of definitions, each shallow
        let mut types = vec![def("D0", vec![], vec![field("a", TypeOwned::Bool)])];
        for i in 1..MAX_DEPTH as u32 {
            types.push(def(
                &format!("D{i}"),
                vec![],
                vec![field(
                    "a",
                    TypeOwned::OutOfLine {
                        type_idx: UNib32(i - 1),
                    },
                )],
            ));
        }
        let last = types.len() as u32 - 1;
        assert_invalid(
            bundle(
                types,
                TypeOwned::OutOfLine {
                    type_idx: UNib32(last),
                },
            ),
            "nested deeper",
        );
    }

    #[test]
    fn growing_instances_are_bounded() {
        // D0<T> { a: T, b: T }, Dk<T> { a: D(k-1)<(T, T)> }: each level doubles the arguments
        let mut types = vec![def(
            "D0",
            vec![param("T")],
            vec![
                field("a", TypeOwned::Param(UNib32(0))),
                field("b", TypeOwned::Param(UNib32(0))),
            ],
        )];
        for i in 1..40u32 {
            let pair = TypeOwned::Tuple(vec![
                TypeOwned::Param(UNib32(0)),
                TypeOwned::Param(UNib32(0)),
            ]);
            types.push(def(
                &format!("D{i}"),
                vec![param("T")],
                vec![field(
                    "a",
                    instance(i - 1, vec![GenericArgOwned::Type(pair)]),
                )],
            ));
        }
        let last = types.len() as u32 - 1;
        let mut b = bundle(
            types,
            instance(last, vec![GenericArgOwned::Type(TypeOwned::Bool)]),
        );
        let err = prepare(&mut b).unwrap_err();
        let text = format!("{err:#}");
        assert!(
            text.contains("nested deeper") || text.contains("type nodes"),
            "{text}"
        );
    }

    #[test]
    fn large_values_are_bounded() {
        // [(); 4_000_000_000] decodes from no bytes at all
        let ty = TypeOwned::Array {
            len: Length::Fixed(UNib32(4_000_000_000)),
            ty: Box::new(TypeOwned::Tuple(vec![])),
        };
        let b = bundle(vec![], ty.clone());
        let err = ValueOwned::des_shrink_wrap_dyn(&[], &ty, &b).unwrap_err();
        assert!(format!("{err}").contains("too large"), "{err}");
        assert!(ValueOwned::default(&ty, &b).is_err());
    }

    #[test]
    fn instance_of_skipped_definition_stays() {
        let types = vec![TypeLocationOwned::SkippedFullVersion {
            crate_idx: UNib32(0),
            type_name: "Timed".into(),
            signature: vec![],
        }];
        let mut b = bundle(types, instance(0, vec![GenericArgOwned::Type(u16_ty())]));
        prepare(&mut b).unwrap();
        let ty = property_ty(&b).clone();
        assert!(matches!(ty, TypeOwned::Instance { .. }));
        assert_eq!(ty.human_name(true, &b).unwrap(), "c::Timed<u16>");
        assert!(ValueOwned::des_shrink_wrap_dyn(&[0], &ty, &b).is_err());
    }

    #[test]
    fn enums_and_const_params_passed_on() {
        // enum E<const N: u64> { A(Buf<N>) }
        let mut types = timed_and_buf();
        types.push(TypeLocationOwned::InLine {
            ty: TypeOwned::Enum(ItemEnumOwned {
                instance_of: None,
                future_version: None,
                size: ElementSize::Unsized,
                repr: Repr::Nibble,
                crate_idx: UNib32(0),
                docs: vec![],
                ident: "E".into(),
                path: None,
                generics: vec![const_param("N")],
                variants: vec![VariantOwned {
                    docs: vec![],
                    ident: "A".into(),
                    fields: FieldsOwned::Unnamed(vec![FieldOwned {
                        ident: None,
                        ..field(
                            "",
                            instance(1, vec![GenericArgOwned::ConstParam(UNib32(0))]),
                        )
                    }]),
                    discriminant: UNib32(0),
                    since: None,
                }],
            }),
            crate_idx: UNib32(0),
        });
        let mut b = bundle(types, instance(2, vec![n(3)]));
        prepare(&mut b).unwrap();
        let ty = property_ty(&b).clone();
        assert_eq!(ty.human_name(false, &b).unwrap(), "E<3>");
        assert_eq!(
            ty.human_definition(&b, true).unwrap(),
            "enum nib c::E<3> {A(struct c::Buf<3> {data: [u16; 3]})}"
        );
        let value = ValueOwned::default(&ty, &b).unwrap();
        let bytes = value.ser_shrink_wrap_dyn(&ty, &b).unwrap();
        assert_eq!(
            ValueOwned::des_shrink_wrap_dyn(&bytes, &ty, &b).unwrap(),
            value
        );
    }

    #[test]
    fn definition_referring_to_itself_is_an_error_everywhere() {
        // types[0] = OutOfLine(0): no bytes are read while following it, only the definition check stops it
        let self_ref = TypeOwned::OutOfLine {
            type_idx: UNib32(0),
        };
        let mut b = bundle(
            vec![TypeLocationOwned::InLine {
                ty: self_ref.clone(),
                crate_idx: UNib32(0),
            }],
            self_ref.clone(),
        );
        assert!(self_ref.is_unsized(&b).is_err());
        assert!(ValueOwned::des_shrink_wrap_dyn(&[0; 8], &self_ref, &b).is_err());
        assert!(ValueOwned::default(&self_ref, &b).is_err());
        assert!(
            ValueOwned::Bool(true)
                .ser_shrink_wrap_dyn(&self_ref, &b)
                .is_err()
        );
        assert!(prepare(&mut b).is_err());
    }

    #[test]
    fn growing_instance_names_are_bounded() {
        // W<T> { w: T }, D0<T> { a: T }, Dk<T> { a: D(k-1)<W<(T, .. 16 times)>> }: few type nodes per instance, but
        // each level repeats the name of the previous one 16 times
        let mut types = vec![
            def(
                "W",
                vec![param("T")],
                vec![field("w", TypeOwned::Param(UNib32(0)))],
            ),
            def(
                "D0",
                vec![param("T")],
                vec![field("a", TypeOwned::Param(UNib32(0)))],
            ),
        ];
        for i in 1..12u32 {
            let many = TypeOwned::Tuple(vec![TypeOwned::Param(UNib32(0)); 16]);
            let w = instance(0, vec![GenericArgOwned::Type(many)]);
            types.push(def(
                &format!("D{i}"),
                vec![param("T")],
                vec![field("a", instance(i, vec![GenericArgOwned::Type(w)]))],
            ));
        }
        let last = types.len() as u32 - 1;
        let mut b = bundle(
            types,
            instance(last, vec![GenericArgOwned::Type(TypeOwned::Bool)]),
        );
        let err = prepare(&mut b).unwrap_err();
        assert!(format!("{err:#}").contains("longer than"), "{err:#}");
    }
    #[test]
    fn decode_work_is_bounded() {
        // Vec<S>, S { <100 KB name>: () } as final structure: zero bits per element, each value has the name
        let long = "n".repeat(100_000);
        let s = TypeOwned::Struct(ItemStructOwned {
            instance_of: None,
            size: ElementSize::UnsizedFinalStructure,
            crate_idx: UNib32(0),
            docs: vec![],
            ident: "S".into(),
            path: None,
            generics: vec![],
            fields: FieldsOwned::Named(vec![field(&long, TypeOwned::Tuple(vec![]))]),
            sparse: false,
            transparent: false,
            marker: None,
        });
        let ty = TypeOwned::Vec(Box::new(s));
        let b = bundle(vec![], ty.clone());
        // 30 000 elements
        let mut buf = [0u8; 16];
        let mut wr = shrink_wrap::BufWriter::new(&mut buf);
        wr.write_rev_len(30_000).unwrap();
        let bytes = wr.finish().unwrap().to_vec();
        let err = ValueOwned::des_shrink_wrap_dyn(&bytes, &ty, &b).unwrap_err();
        assert!(format!("{err}").contains("too large"), "{err}");
        // Vec<E>, E with 100 000 variants and a zero-bit discriminant: each element looks through all of them
        let e = TypeOwned::Enum(ItemEnumOwned {
            instance_of: None,
            future_version: None,
            size: ElementSize::UnsizedFinalStructure,
            repr: Repr::BitAligned(0),
            crate_idx: UNib32(0),
            docs: vec![],
            ident: "E".into(),
            path: None,
            generics: vec![],
            variants: (0..100_000u32)
                .map(|i| VariantOwned {
                    docs: vec![],
                    ident: format!("V{i}"),
                    fields: FieldsOwned::Unit,
                    discriminant: UNib32(100_000 - i - 1),
                    since: None,
                })
                .collect(),
        });
        let ty = TypeOwned::Vec(Box::new(e));
        let b = bundle(vec![], ty.clone());
        let err = ValueOwned::des_shrink_wrap_dyn(&bytes, &ty, &b).unwrap_err();
        assert!(format!("{err}").contains("too large"), "{err}");
    }
    #[test]
    fn long_definitions_are_bounded() {
        // L with a 100 KB name, T { f0..f1999: L }: 200 MB of text
        let mut types = vec![def(&"L".repeat(100_000), vec![], vec![])];
        let fields = (0..2000)
            .map(|i| {
                field(
                    &format!("f{i}"),
                    TypeOwned::OutOfLine {
                        type_idx: UNib32(0),
                    },
                )
            })
            .collect();
        types.push(def("T", vec![], fields));
        let ty = TypeOwned::OutOfLine {
            type_idx: UNib32(1),
        };
        let b = bundle(types, ty.clone());
        let err = ty.human_definition(&b, true).unwrap_err();
        assert!(format!("{err}").contains("bytes"), "{err}");
    }
}
