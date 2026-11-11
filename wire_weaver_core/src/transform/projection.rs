//! Associated types of a trait impl, `<V as RawValue>::Raw` and the shorthand `V::Raw` (API-65): exact_units'
//! `Niched<V, N, T = <V as RawValue>::Raw>` takes its storage from the value type. The walker finds the
//! `impl RawValue for Fixed<U, P, T, B>` the value type has, binds the impl's parameters by matching its self type
//! against the converted value type, and converts the impl's `type Raw = T` with that binding.
//!
//! The impl is searched for in the trait's crate and in the self type's crate, the only two the orphan rule allows,
//! in any module of them ([CrateContext::trait_impls]). A projection of a parameter of the generic definition being
//! converted has no type to look up yet and is an error, as is one no impl answers.

use super::{
    crate_walker::{CrateContext, Scratch, TraitImpl},
    ty::{ConstValue, alias_arg, const_expr_value, convert_ty, find_user_item, scope_param},
};
use anyhow::{Result, anyhow};
use quote::ToTokens;
use std::{collections::HashSet, rc::Rc};
use syn::{GenericArgument, GenericParam, Item, PathArguments, Type, TypePath, WherePredicate};
use ww_self::{GenericArgOwned, TypeLocationOwned, TypeOwned, ValueOwned};

/// A projection as written: `<qself as trait_path>::assoc`, or `param::assoc` with the trait left to `param`'s
/// bounds.
pub(super) struct Projection<'t> {
    written: &'t TypePath,
    self_ty: Type,
    /// `None` for the shorthand `V::Assoc`.
    trait_path: Option<syn::Path>,
    assoc: &'t syn::Ident,
}

impl<'t> Projection<'t> {
    /// `ty_path` as a projection: `<X as Trait>::Assoc`, or `V::Assoc` where `V` is a parameter in scope (of the
    /// alias, the default or the impl being converted, or of the generic definition).
    pub(super) fn of(ty_path: &'t TypePath, scratch: &Scratch) -> Option<Self> {
        let segments = &ty_path.path.segments;
        if let Some(qself) = &ty_path.qself {
            let assoc = segments.last()?;
            let trait_path = syn::Path {
                leading_colon: ty_path.path.leading_colon,
                segments: segments.iter().take(qself.position).cloned().collect(),
            };
            return Some(Self {
                written: ty_path,
                self_ty: (*qself.ty).clone(),
                trait_path: (qself.position > 0).then_some(trait_path),
                assoc: &assoc.ident,
            });
        }
        let [param, assoc] = segments.iter().collect::<Vec<_>>()[..] else {
            return None;
        };
        let name = param.ident.to_string();
        let in_scope = alias_arg(scratch, &name).is_some() || scope_param(scratch, &name).is_some();
        (in_scope && param.arguments.is_none() && assoc.arguments.is_none()).then(|| Self {
            written: ty_path,
            self_ty: {
                let ident = &param.ident;
                syn::parse_quote!(#ident)
            },
            trait_path: None,
            assoc: &assoc.ident,
        })
    }

    /// The type the projection names, for the self type converted in `current_crate`.
    pub(super) fn resolve(
        &self,
        current_crate: &CrateContext,
        scratch: &mut Scratch,
    ) -> Result<TypeOwned> {
        let written = self.written.to_token_stream().to_string();
        let err = |msg: String| {
            anyhow!("projection {written}: {msg}").context(current_crate.err_context())
        };
        let self_ty = convert_ty(&self.self_ty, current_crate, scratch)
            .map_err(|e| e.context(format!("projection {written}")))?;
        if let TypeOwned::Param(_) = self_ty {
            return Err(err(format!(
                "{} is a parameter of the generic definition being converted, so the impl is not known until it is \
                 used; write the type out instead",
                self.self_ty.to_token_stream()
            )));
        }
        let traits = match &self.trait_path {
            Some(path) => vec![path.clone()],
            None => {
                let param = self.self_ty.to_token_stream().to_string();
                scratch
                    .trait_bounds
                    .iter()
                    .find(|(name, _)| *name == param)
                    .map(|(_, traits)| traits.clone())
                    .unwrap_or_default()
            }
        };
        let mut searched = vec![];
        for trait_path in &traits {
            // a bound that isn't a trait the walker can find (`Copy`) can't declare the type
            let Ok((trait_cx, item_trait)) = find_trait(trait_path, current_crate, scratch) else {
                if self.trait_path.is_some() {
                    find_trait(trait_path, current_crate, scratch)
                        .map_err(|e| e.context(format!("projection {written}")))?;
                }
                continue;
            };
            if !declares_assoc(&item_trait, self.assoc) {
                if self.trait_path.is_some() {
                    return Err(err(format!(
                        "trait {} has no associated type {}",
                        item_trait.ident, self.assoc
                    )));
                }
                continue;
            }
            let trait_key = (
                trait_cx.version().clone(),
                trait_cx.qualified_name(&item_trait.ident.to_string()),
            );
            for krate in candidate_crates(&trait_cx, &self_ty, scratch)? {
                searched.push(krate.crate_name().to_string());
                for imp in krate.trait_impls(scratch)?.iter() {
                    if let Some(ty) = assoc_of_impl(imp, &trait_key, &self_ty, self.assoc, scratch)
                        .map_err(|e| e.context(format!("projection {written}")))?
                    {
                        return Ok(ty);
                    }
                }
            }
        }
        if traits.is_empty() || searched.is_empty() {
            return Err(err(format!(
                "no trait bound of {} declares {}",
                self.self_ty.to_token_stream(),
                self.assoc
            )));
        }
        searched.dedup();
        Err(err(format!(
            "no impl for {} found in {}",
            self_ty_name(&self_ty, scratch),
            searched.join(", ")
        )))
    }
}

/// Trait bounds of the type parameters of `generics`, in the list and in the where clause.
pub(super) fn param_bounds(generics: &syn::Generics) -> Vec<(String, Vec<syn::Path>)> {
    let mut bounds: Vec<(String, Vec<syn::Path>)> = vec![];
    let mut add = |name: String, of: &mut dyn Iterator<Item = &syn::TypeParamBound>| {
        let traits = of.filter_map(|b| match b {
            syn::TypeParamBound::Trait(t) => Some(t.path.clone()),
            _ => None,
        });
        match bounds.iter_mut().find(|(n, _)| *n == name) {
            Some((_, list)) => list.extend(traits),
            None => bounds.push((name, traits.collect())),
        }
    };
    for param in &generics.params {
        if let GenericParam::Type(type_param) = param {
            add(type_param.ident.to_string(), &mut type_param.bounds.iter());
        }
    }
    for predicate in generics.where_clause.iter().flat_map(|w| &w.predicates) {
        if let WherePredicate::Type(pred) = predicate
            && let Type::Path(p) = &pred.bounded_ty
            && let Some(ident) = p.path.get_ident()
        {
            add(ident.to_string(), &mut pred.bounds.iter());
        }
    }
    bounds
}

/// The trait `path` names in `current_crate`, with the module that defines it.
fn find_trait(
    path: &syn::Path,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<(CrateContext, syn::ItemTrait)> {
    let segments: Vec<_> = path.segments.iter().collect();
    let Some((last, leading)) = segments.split_last() else {
        return Err(anyhow!("empty trait path"));
    };
    let lookup_cx = if leading.is_empty() {
        Rc::new(current_crate.clone())
    } else {
        current_crate.resolve_path(leading.iter().map(|s| &s.ident), scratch)?
    };
    match find_user_item(&last.ident.to_string(), &lookup_cx, scratch)? {
        (cx, Item::Trait(item_trait)) => Ok((cx, item_trait)),
        _ => Err(anyhow!("{} is not a trait", last.ident)),
    }
}

fn declares_assoc(item_trait: &syn::ItemTrait, assoc: &syn::Ident) -> bool {
    item_trait
        .items
        .iter()
        .any(|i| matches!(i, syn::TraitItem::Type(t) if t.ident == *assoc))
}

/// The crates an impl of the trait of `trait_cx` for `self_ty` can be in: the trait's, and the self type's when it
/// is a user type of another crate.
fn candidate_crates(
    trait_cx: &CrateContext,
    self_ty: &TypeOwned,
    scratch: &mut Scratch,
) -> Result<Vec<Rc<CrateContext>>> {
    let mut crates = vec![Rc::new(trait_cx.clone())];
    if let Some((version, _, _)) = def_identity(self_ty, scratch)
        && version != *trait_cx.version()
        && let Some(krate) = scratch.loaded_crate(&version)
    {
        crates.push(krate);
    }
    Ok(crates)
}

/// Crate, path and name of the user definition `ty` is (an instance of).
fn def_identity(
    ty: &TypeOwned,
    scratch: &Scratch,
) -> Option<(ww_version::FullVersionOwned, Option<Vec<String>>, String)> {
    let (TypeOwned::OutOfLine { type_idx } | TypeOwned::Instance { type_idx, .. }) = ty else {
        return None;
    };
    let TypeLocationOwned::InLine { ty, .. } =
        scratch.root_bundle.types.get(type_idx.0 as usize)?
    else {
        return None;
    };
    let (crate_idx, path, ident) = match ty {
        TypeOwned::Struct(s) => (s.crate_idx, s.path.clone(), s.ident.clone()),
        TypeOwned::Enum(e) => (e.crate_idx, e.path.clone(), e.ident.clone()),
        _ => return None,
    };
    let version = scratch
        .root_bundle
        .ext_crates
        .get(crate_idx.0 as usize)?
        .clone();
    Some((version, path, ident))
}

fn self_ty_name(ty: &TypeOwned, scratch: &Scratch) -> String {
    match def_identity(ty, scratch) {
        Some((version, path, ident)) => {
            let mut name = version.crate_id.clone();
            for m in path.iter().flatten() {
                name.push_str("::");
                name.push_str(m);
            }
            format!("{name}::{ident}")
        }
        None => format!("{ty:?}"),
    }
}

/// `imp`'s associated type `assoc`, when `imp` is an impl of the trait `trait_key` whose self type matches
/// `self_ty`.
fn assoc_of_impl(
    imp: &TraitImpl,
    trait_key: &(ww_version::FullVersionOwned, String),
    self_ty: &TypeOwned,
    assoc: &syn::Ident,
    scratch: &mut Scratch,
) -> Result<Option<TypeOwned>> {
    let item = imp.item().clone();
    let Some((trait_path, _)) = &item.trait_ else {
        return Ok(None);
    };
    let Some(trait_name) = trait_path.segments.last() else {
        return Ok(None);
    };
    if trait_name.ident != trait_key.1.rsplit("::").next().unwrap_or_default() {
        return Ok(None);
    }
    // the same trait, not one of the same name elsewhere
    match find_trait(trait_path, &imp.cx, scratch) {
        Ok((cx, t))
            if (
                cx.version().clone(),
                cx.qualified_name(&t.ident.to_string()),
            ) == *trait_key => {}
        _ => return Ok(None),
    }
    let params: HashSet<String> = item
        .generics
        .params
        .iter()
        .filter_map(|p| match p {
            GenericParam::Type(t) => Some(t.ident.to_string()),
            GenericParam::Const(c) => Some(c.ident.to_string()),
            GenericParam::Lifetime(_) => None,
        })
        .collect();
    let mut binding = vec![];
    if !matches(
        &item.self_ty,
        self_ty,
        &imp.cx,
        &params,
        &mut binding,
        scratch,
    ) {
        return Ok(None);
    }
    let Some(assoc_ty) = item.items.iter().find_map(|i| match i {
        syn::ImplItem::Type(t) if t.ident == *assoc => Some(&t.ty),
        _ => None,
    }) else {
        return Err(anyhow!(
            "the impl of {} for {} has no type {assoc}",
            trait_name.ident,
            item.self_ty.to_token_stream()
        )
        .context(imp.cx.err_context()));
    };
    let ty = with_binding(scratch, binding, param_bounds(&item.generics), |scratch| {
        convert_ty(assoc_ty, &imp.cx, scratch)
    });
    ty.map(Some).map_err(|e| {
        e.context(format!(
            "type {assoc} of the impl of {} for {}",
            trait_name.ident,
            item.self_ty.to_token_stream()
        ))
    })
}

/// Runs `convert` with the impl's parameters standing for `binding` and nothing else in scope.
fn with_binding<T>(
    scratch: &mut Scratch,
    binding: Vec<(String, GenericArgOwned)>,
    bounds: Vec<(String, Vec<syn::Path>)>,
    convert: impl FnOnce(&mut Scratch) -> Result<T>,
) -> Result<T> {
    let saved_scope = std::mem::take(&mut scratch.generic_scope);
    let saved_alias_args = std::mem::replace(&mut scratch.alias_args, binding);
    let saved_bounds = std::mem::replace(&mut scratch.trait_bounds, bounds);
    let out = convert(scratch);
    scratch.trait_bounds = saved_bounds;
    scratch.alias_args = saved_alias_args;
    scratch.generic_scope = saved_scope;
    out
}

/// Whether the impl's self type `written` (in `cx`, with the impl's `params`) matches the converted `target`,
/// binding the parameters it meets on the way.
fn matches(
    written: &Type,
    target: &TypeOwned,
    cx: &CrateContext,
    params: &HashSet<String>,
    binding: &mut Vec<(String, GenericArgOwned)>,
    scratch: &mut Scratch,
) -> bool {
    match written {
        Type::Paren(p) => matches(&p.elem, target, cx, params, binding, scratch),
        Type::Group(g) => matches(&g.elem, target, cx, params, binding, scratch),
        Type::Reference(r) => matches(&r.elem, target, cx, params, binding, scratch),
        Type::Tuple(t) => match target {
            TypeOwned::Tuple(types) if types.len() == t.elems.len() => t
                .elems
                .iter()
                .zip(types)
                .all(|(w, ty)| matches(w, ty, cx, params, binding, scratch)),
            _ => false,
        },
        Type::Path(p) if p.qself.is_none() => {
            if let Some(ident) = p.path.get_ident()
                && params.contains(&ident.to_string())
            {
                return bind(
                    binding,
                    ident.to_string(),
                    GenericArgOwned::Type(target.clone()),
                );
            }
            match user_def(p, cx, scratch) {
                Some((def_cx, item)) => {
                    matches_def(p, &def_cx, &item, target, cx, params, binding, scratch)
                }
                None => converts_to(written, target, cx, binding, scratch),
            }
        }
        _ => converts_to(written, target, cx, binding, scratch),
    }
}

/// The struct or enum the path names, `None` for a built-in type, an alias or one the walker can't find.
fn user_def(
    p: &TypePath,
    cx: &CrateContext,
    scratch: &mut Scratch,
) -> Option<(CrateContext, Item)> {
    let segments: Vec<_> = p.path.segments.iter().collect();
    let (last, leading) = segments.split_last()?;
    let lookup_cx = if leading.is_empty() {
        Rc::new(cx.clone())
    } else {
        cx.resolve_path(leading.iter().map(|s| &s.ident), scratch)
            .ok()?
    };
    match find_user_item(&last.ident.to_string(), &lookup_cx, scratch).ok()? {
        found @ (_, Item::Struct(_) | Item::Enum(_)) => Some(found),
        _ => None,
    }
}

/// `p`, the struct or enum `item` of `def_cx`, against `target`: the same definition, then its arguments.
#[allow(clippy::too_many_arguments)]
fn matches_def(
    p: &TypePath,
    def_cx: &CrateContext,
    item: &Item,
    target: &TypeOwned,
    cx: &CrateContext,
    params: &HashSet<String>,
    binding: &mut Vec<(String, GenericArgOwned)>,
    scratch: &mut Scratch,
) -> bool {
    let ident = match item {
        Item::Struct(s) => s.ident.to_string(),
        Item::Enum(e) => e.ident.to_string(),
        _ => return false,
    };
    let Some((version, path, target_ident)) = def_identity(target, scratch) else {
        return false;
    };
    if version != *def_cx.version() || target_ident != ident {
        return false;
    }
    match def_cx.public_name(&ident, scratch) {
        Ok((def_path, _)) if def_path == path => {}
        _ => return false,
    }
    let target_args: &[GenericArgOwned] = match target {
        TypeOwned::Instance { args, .. } => args,
        _ => &[],
    };
    let written: Vec<&GenericArgument> = match &p.path.segments.last().expect("a path").arguments {
        PathArguments::AngleBracketed(args) => args
            .args
            .iter()
            .filter(|a| !matches!(a, GenericArgument::Lifetime(_)))
            .collect(),
        PathArguments::None => vec![],
        PathArguments::Parenthesized(_) => return false,
    };
    if written.len() > target_args.len() {
        return false;
    }
    // arguments left out are the definition's defaults, whatever they are
    written
        .iter()
        .zip(target_args)
        .all(|(w, arg)| match (w, arg) {
            (
                GenericArgument::Type(Type::Path(tp)),
                GenericArgOwned::Const(_) | GenericArgOwned::ConstParam(_),
            ) if tp
                .path
                .get_ident()
                .is_some_and(|i| params.contains(&i.to_string())) =>
            {
                let name = tp.path.get_ident().expect("an ident").to_string();
                bind(binding, name, arg.clone())
            }
            (GenericArgument::Type(ty), GenericArgOwned::Type(target)) => {
                matches(ty, target, cx, params, binding, scratch)
            }
            (GenericArgument::Const(expr), GenericArgOwned::Const(value)) => match expr {
                syn::Expr::Path(ep)
                    if ep
                        .path
                        .get_ident()
                        .is_some_and(|i| params.contains(&i.to_string())) =>
                {
                    let name = ep.path.get_ident().expect("an ident").to_string();
                    bind(binding, name, arg.clone())
                }
                _ => const_expr_value(expr, cx).is_some_and(|v| same_value(v, value)),
            },
            _ => false,
        })
}

fn same_value(v: ConstValue, value: &ValueOwned) -> bool {
    match (v, value) {
        (ConstValue::Bool(a), ValueOwned::Bool(b)) => a == *b,
        (ConstValue::Int(a), ValueOwned::Numeric(n)) => n.to_i128() == Some(a),
        _ => false,
    }
}

/// `written` converted with the parameters bound so far is `target`: a built-in type, or one with no parameter
/// of the impl left in it.
fn converts_to(
    written: &Type,
    target: &TypeOwned,
    cx: &CrateContext,
    binding: &[(String, GenericArgOwned)],
    scratch: &mut Scratch,
) -> bool {
    with_binding(scratch, binding.to_vec(), vec![], |scratch| {
        convert_ty(written, cx, scratch)
    })
    .is_ok_and(|ty| ty == *target)
}

/// Binds `name` to `arg`; false when it is bound to another already.
fn bind(binding: &mut Vec<(String, GenericArgOwned)>, name: String, arg: GenericArgOwned) -> bool {
    match binding.iter().find(|(n, _)| *n == name) {
        Some((_, bound)) => *bound == arg,
        None => {
            binding.push((name, arg));
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ty::recursive_tests::load;
    use ww_self::{ApiBundleOwned, ApiItemKindOwned};

    /// Arguments of the first method, as written out in full.
    fn args(bundle: &ApiBundleOwned) -> Vec<String> {
        let mut prepared = bundle.clone();
        ww_self::generics::prepare(&mut prepared).unwrap();
        let ApiItemKindOwned::Method { args, .. } = &bundle.root.items[0].kind else {
            panic!("method");
        };
        args.iter()
            .map(|a| a.ty.human_definition(&prepared, true).unwrap())
            .collect()
    }

    /// exact_units' shapes: the trait, the impls and the type with the default in three modules, the impls of
    /// `RawValue` nowhere near the types they are for.
    const TYPES: &str = "
        use core::marker::PhantomData;
        pub use fixed::{Fixed, Point, Unbounded};
        pub use niche::{Clipped, Niched, Null, RawValue};
        mod fixed {
            use core::marker::PhantomData;
            pub struct Fixed<U, P, T, B = Unbounded> { raw: T, _u: PhantomData<(U, P, B)> }
            pub struct Point<O, V> { value: V, _o: PhantomData<O> }
            pub struct Unbounded;
        }
        pub struct Ratio;
        pub struct One;
        pub struct Celsius;
        pub struct Kelvin;
        pub trait Origin { type Unit; }
        impl Origin for Celsius { type Unit = Kelvin; }
        pub mod niche {
            use core::marker::PhantomData;
            pub trait RawValue: Copy { type Raw: Copy; }
            pub trait Niches<T> {}
            pub struct Null<const CODE: i128>;
            impl<T, const CODE: i128> Niches<T> for Null<CODE> {}
            #[derive_shrink_wrap(transparent)]
            pub struct Niched<V: RawValue<Raw = T>, N: Niches<T>, T: Copy = <V as RawValue>::Raw> {
                raw: T, _v: PhantomData<V>, _n: PhantomData<N>,
            }
            pub struct Clipped<V, R> { value: V, _r: PhantomData<R> }
            /// a default through the shorthand
            pub struct Short<V: RawValue, T = V::Raw> { raw: T, _v: PhantomData<V> }
            pub use super::raw::*;
        }
        mod raw {
            use crate::fixed::{Fixed, Point};
            use crate::niche::{Clipped, RawValue};
            use crate::Origin;
            impl<U, P, T: Copy, B> RawValue for Fixed<U, P, T, B> { type Raw = T; }
            impl<O: Origin<Unit = U>, U, P, T: Copy, B> RawValue for Point<O, Fixed<U, P, T, B>> { type Raw = T; }
            impl<V: RawValue, R> RawValue for Clipped<V, R> { type Raw = V::Raw; }
            impl RawValue for bool { type Raw = u8; }
        }
        pub struct NotRaw;
        pub struct Wrap<V> { n: Niched<V, Null<0>> }
    ";

    #[test]
    fn default_through_a_projection() {
        let bundle = load(
            "projection",
            "fn a(a: Niched<Fixed<Ratio, One, u16>, Null<0xFFFF>>, b: Niched<Fixed<Ratio, One, u16>, Null<0xFFFF>, u16>, \
             c: Niched<Point<Celsius, Fixed<Kelvin, One, i8>>, Null<-128>>, \
             d: Niched<Clipped<Fixed<Ratio, One, u32>, One>, Null<0>>, e: niche::Short<Fixed<Ratio, One, u8>>, \
             f: Niched<bool, Null<0>>, g: <Fixed<Ratio, One, i64> as RawValue>::Raw);",
            TYPES,
        )
        .unwrap();
        let args = args(&bundle);
        // left out and written out are one type
        assert_eq!(args[0], args[1]);
        assert_eq!(
            args[0],
            "struct test_api::Niched<Fixed<Ratio, One, u16, Unbounded>, Null<65535>, u16> {raw: u16}"
        );
        assert_eq!(
            args[2],
            "struct test_api::Niched<Point<Celsius, Fixed<Kelvin, One, i8, Unbounded>>, Null<-128>, i8> {raw: i8}"
        );
        assert_eq!(
            args[3],
            "struct test_api::Niched<Clipped<Fixed<Ratio, One, u32, Unbounded>, One>, Null<0>, u32> {raw: u32}"
        );
        assert_eq!(
            args[4],
            "struct test_api::Short<Fixed<Ratio, One, u8, Unbounded>, u8> {raw: u8}"
        );
        assert_eq!(
            args[5],
            "struct test_api::Niched<bool, Null<0>, u8> {raw: u8}"
        );
        assert_eq!(args[6], "i64");
    }

    #[test]
    fn unresolved_projections_are_named() {
        let e = load(
            "projection_no_impl",
            "fn a(a: Niched<NotRaw, Null<0>>);",
            TYPES,
        )
        .unwrap_err();
        let e = format!("{e:#}");
        assert!(
            e.contains("projection < V as RawValue > :: Raw: no impl for test_api::NotRaw found in test_api"),
            "{e}"
        );
        assert!(e.contains("default of T of Niched"), "{e}");
        let e = load(
            "projection_param",
            "fn a(a: Wrap<Fixed<Ratio, One, u8>>);",
            TYPES,
        )
        .unwrap_err();
        let e = format!("{e:#}");
        assert!(
            e.contains(
                "projection < V as RawValue > :: Raw: V is a parameter of the generic definition"
            ),
            "{e}"
        );
        let e = load(
            "projection_no_assoc",
            "fn a(a: <Fixed<Ratio, One, u8> as RawValue>::Wire);",
            TYPES,
        )
        .unwrap_err();
        assert!(
            format!("{e:#}").contains("trait RawValue has no associated type Wire"),
            "{e:#}"
        );
    }
}
