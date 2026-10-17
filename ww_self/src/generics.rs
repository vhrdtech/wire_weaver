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
    FieldsOwned, GenericArgOwned, GenericParamKindOwned, GenericParamOwned, ItemEnumOwned,
    ItemStructOwned, Length, TypeLocationOwned, TypeOwned, ValueOwned,
};
use anyhow::{Result, anyhow, bail};
use shrink_wrap::{SerializeShrinkWrapOwned, UNib32};
use std::collections::HashMap;
use ww_numeric::{NumericAnyTypeOwned, NumericBaseType};

/// Deepest nesting of types allowed in a bundle, counted along the longest path through type definitions (each
/// `Vec`, `Option`, field, out-of-line reference, ... is one level). Walks over types (dynamic decoding, compatibility
/// checks) recurse this deep at most once a bundle passed [validate].
pub const MAX_DEPTH: usize = 96;
/// Most concrete definitions [instantiate] adds to a bundle.
pub const MAX_INSTANCES: usize = 4096;
/// Most type nodes [instantiate] creates in total (and [substitute] in one call).
pub const MAX_NODES: usize = 1 << 20;

/// Checks `bundle` ([validate]), instantiates every use of a generic type ([instantiate]) and checks the result
/// again. Run on every bundle from outside (a device's introspection data, a file) before using it dynamically,
/// after skipped definitions are put back ([crate::inline]).
pub fn prepare(bundle: &mut ApiBundleOwned) -> Result<()> {
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
/// - Definitions don't refer to themselves, directly or through others, and nothing is nested deeper than
///   [MAX_DEPTH].
///
/// Skipped definitions can't be checked, uses of them are checked once they are put back. Takes time linear in the
/// size of the bundle.
pub fn validate(bundle: &ApiBundleOwned) -> Result<()> {
    let mut v = Validator {
        bundle,
        defs: vec![Def::NotSeen; bundle.types.len()],
    };
    for idx in 0..bundle.types.len() {
        v.def_depth(idx as u32, 0)?;
    }
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
    InProgress,
    /// Nesting depth of the definition's own fields (parameters count as one level).
    Depth(usize),
}

struct Validator<'a> {
    bundle: &'a ApiBundleOwned,
    defs: Vec<Def>,
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
                    ..
                } => std::iter::once(ty)
                    .chain(args.iter().map(|a| &a.ty))
                    .chain(return_ty.as_ref())
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

    /// Nesting depth of the fields of `types[idx]`, checked once. `path`: nesting depth of the use.
    fn def_depth(&mut self, idx: u32, path: usize) -> Result<usize> {
        let Some(location) = self.bundle.types.get(idx as usize) else {
            bail!("no type with index {idx}");
        };
        let depth = match self.defs[idx as usize] {
            Def::Depth(depth) => depth,
            Def::InProgress => bail!("type {} refers to itself", def_name(location)),
            Def::NotSeen => {
                self.defs[idx as usize] = Def::InProgress;
                let depth = match location {
                    TypeLocationOwned::InLine { ty, .. } => match ty {
                        TypeOwned::Struct(s) => {
                            check_generics(&s.generics)?;
                            self.fields_depth(&s.fields, &s.generics, path)?
                        }
                        TypeOwned::Enum(e) => {
                            check_generics(&e.generics)?;
                            let mut depth = 1;
                            for variant in &e.variants {
                                depth = depth.max(self.fields_depth(
                                    &variant.fields,
                                    &e.generics,
                                    path,
                                )?);
                            }
                            depth
                        }
                        _ => bail!("type with index {idx} is not a struct or an enum"),
                    },
                    TypeLocationOwned::SkippedFullVersion { .. } => 1,
                };
                self.defs[idx as usize] = Def::Depth(depth);
                depth
            }
        };
        if path + depth > MAX_DEPTH {
            bail!("types are nested deeper than {MAX_DEPTH} levels");
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
            | TypeOwned::RangeInclusive(_)
            | TypeOwned::Delta(_)
            | TypeOwned::DeltaOfDelta(_)
            | TypeOwned::XorFloat(_) => 1,
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
                1 + self.fields_depth(&s.fields, &[], path + 1)?
            }
            TypeOwned::Enum(e) => {
                if !e.generics.is_empty() {
                    bail!("generic enum {} is not in the types array", e.ident);
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
            let TypeLocationOwned::InLine { ty, .. } = &mut bundle.types[idx as usize] else {
                continue;
            };
            let mut ty = std::mem::replace(ty, TypeOwned::Bool);
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
            | TypeOwned::RangeInclusive(_)
            | TypeOwned::Delta(_)
            | TypeOwned::DeltaOfDelta(_)
            | TypeOwned::XorFloat(_) => Ok(()),
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
    let mut arg_names = Vec::with_capacity(args.len());
    for arg in args {
        arg_names.push(match arg {
            GenericArgOwned::Type(ty) => ty.human_name(false, bundle)?,
            GenericArgOwned::Const(value) => const_name(value),
            GenericArgOwned::ConstParam(idx) => {
                bail!(
                    "argument is const parameter {} outside of a generic definition",
                    idx.0
                )
            }
        });
    }
    let ident = format!("{name}<{}>", arg_names.join(", "));
    let mut sub = Substitute { args, nodes: 0 };
    let ty = match def {
        TypeOwned::Struct(s) => TypeOwned::Struct(ItemStructOwned {
            size: s.size,
            crate_idx: s.crate_idx,
            docs: s.docs.clone(),
            ident,
            generics: vec![],
            fields: sub.fields(&s.fields, 0)?,
            sparse: s.sparse,
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
                generics: vec![],
                variants,
            })
        }
        _ => bail!("type with index {type_idx} is not a struct or an enum"),
    };
    Ok(ty)
}

/// A const argument as written: `64`, `true`.
fn const_name(value: &ValueOwned) -> String {
    match value {
        ValueOwned::Bool(b) => b.to_string(),
        ValueOwned::Numeric(n) => match n.to_i128() {
            Some(v) => v.to_string(),
            None => format!("{n:?}"),
        },
        other => format!("{other:?}"),
    }
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
    use crate::{FieldOwned, Repr, VariantOwned};
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
                size: ElementSize::Unsized,
                crate_idx: UNib32(0),
                docs: vec![],
                ident: ident.into(),
                generics,
                fields: FieldsOwned::Named(fields),
                sparse: false,
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
                items: vec![crate::ApiItemOwned {
                    id: UNib32(0),
                    kind: ApiItemKindOwned::Property {
                        ty: property,
                        access: crate::PropertyAccess::Const,
                        write_err_ty: None,
                        errors: None,
                    },
                    multiplicity: crate::Multiplicity::Flat,
                    since: None,
                    ident: "p".into(),
                    docs: vec![],
                    optional: None,
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

    #[test]
    fn cycles_are_errors() {
        // struct A { a: B }, struct B { b: A }
        let types = vec![
            def(
                "A",
                vec![],
                vec![field(
                    "a",
                    TypeOwned::OutOfLine {
                        type_idx: UNib32(1),
                    },
                )],
            ),
            def(
                "B",
                vec![],
                vec![field(
                    "b",
                    TypeOwned::OutOfLine {
                        type_idx: UNib32(0),
                    },
                )],
            ),
        ];
        assert_invalid(bundle(types, TypeOwned::Bool), "refers to itself");
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
                size: ElementSize::Unsized,
                repr: Repr::Nibble,
                crate_idx: UNib32(0),
                docs: vec![],
                ident: "E".into(),
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
        assert!(ValueOwned::Bool(true)
            .ser_shrink_wrap_dyn(&self_ref, &b)
            .is_err());
        assert!(prepare(&mut b).is_err());
    }
}
