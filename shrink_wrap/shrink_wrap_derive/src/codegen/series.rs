//! `#[derive_shrink_wrap(.., series)]` (SW-60): the `SeriesColumns` impl of a struct or enum, made of the pieces
//! in `shrink_wrap::series` the way the hand-written impls of its tests are. A struct's group is its fields' groups
//! in declaration order (behind a field count or group length when it is evolvable), each field restarting where an
//! enum-typed field before it changed; an enum's group is its tag run, then each data variant's fields as such a
//! group over the items in the variant, in discriminant order.

use crate::ast::generics::Generics;
use crate::ast::item_enum::{Fields, Variant};
use crate::ast::item_struct::Field;
use crate::ast::ty::Type;
use crate::codegen::util::{CratePath, maybe_quote};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

/// What both impls need to know about the type.
pub(crate) struct SeriesSeed<'a> {
    pub(crate) ident: &'a Ident,
    /// The type's parameters and where clause (DER-14): each type parameter needs `SeriesColumns<'i>`.
    pub(crate) generics: &'a Generics,
    /// The type has a `<'i>` of its own.
    pub(crate) has_lifetime: bool,
    /// Borrowed side: opaque fields go through `SerializeShrinkWrap` / `DeserializeShrinkWrap<'i>`, else the owned
    /// traits.
    pub(crate) is_ref: bool,
    pub(crate) cfg: Option<&'a TokenStream>,
    pub(crate) cp: &'a CratePath,
    /// Also implement `SeriesStore` (SW-62), unless a field is opaque: off with `series(no_store)`.
    pub(crate) store: bool,
    /// Also implement `SeriesCells` (API-50) next to the store: `series(cells)`.
    pub(crate) cells: bool,
}

/// A field as a `Series` sees it: flags left out, they are part of their `Option`.
struct SeriesField<'a> {
    /// `self.#member` (or the binding in a variant pattern).
    member: TokenStream,
    ty: TokenStream,
    opaque: bool,
    /// Can have a `series_tag`: a user type or an `Option`, which an earlier-enum restart looks at.
    tagged: bool,
    /// Has `SeriesCells`: not opaque, no string or byte list in it.
    cells: bool,
    default: Option<&'a syn::Expr>,
}

/// Whether a field type can have `SeriesCells`: strings and byte lists can't be kept in atomics.
fn has_cells(ty: &Type) -> bool {
    match ty {
        Type::String | Type::Vec(_) | Type::MaxLen(_, _) => false,
        Type::Array(_, ty) | Type::Option(_, ty) => has_cells(ty),
        Type::Tuple(types) => types.iter().all(has_cells),
        _ => true,
    }
}

/// Fields of the v1 subset (design `series.md`, "Which `T` in v1"): a compile error names the field otherwise.
fn check_ty(ty: &Type, owner: &str, field: &str) -> Result<(), String> {
    let not_v1 = |what: &str, hint: &str| {
        Err(format!(
            "series: field `{field}` of `{owner}`: {what} not in Series v1{hint}"
        ))
    };
    const OPAQUE: &str =
        ", mark the field #[series(opaque)] to write it as an opaque run of its bytes";
    match ty {
        Type::RefBox(_) => not_v1(
            "a RefBox (a recursive type, its column set would be unbounded) is",
            OPAQUE,
        ),
        Type::Result(_, _) => not_v1("Result is", OPAQUE),
        Type::Vec(inner) | Type::MaxLen(_, inner) if **inner != Type::U8 => {
            not_v1("a list (other than of u8) is", OPAQUE)
        }
        Type::MaxLen(_, _) => not_v1("MaxLen is", OPAQUE),
        Type::Range(_) | Type::RangeInclusive(_) => not_v1("a range is", OPAQUE),
        Type::TailSize(_) => not_v1("TailSize (the size of a single value) is", ""),
        Type::Array(_, ty) | Type::Option(_, ty) => check_ty(ty, owner, field),
        Type::Tuple(types) => types.iter().try_for_each(|ty| check_ty(ty, owner, field)),
        _ => Ok(()),
    }
}

impl SeriesSeed<'_> {
    fn sc(&self, name: TokenStream) -> TokenStream {
        self.cp.item(quote! { series::#name })
    }

    /// `<'i, T>` after the type's name.
    fn lifetime(&self) -> TokenStream {
        self.generics.ty(self.has_lifetime)
    }

    /// `T: SeriesColumns<'i>, const N: usize, ..` of the impl.
    fn impl_params(&self) -> TokenStream {
        let trait_ = self.sc(quote! { SeriesColumns });
        self.generics.impl_params(Some(quote! { #trait_<'i> }))
    }

    /// A block summing the `MAX_ITEM_BITS` or `MAX_FIXED_BITS` (`which`) of `fields`, starting from `start`; an
    /// opaque field is unbounded.
    fn bound_sum(
        &self,
        fields: &[SeriesField],
        which: TokenStream,
        start: TokenStream,
    ) -> TokenStream {
        let trait_ = self.sc(quote! { SeriesColumns });
        let add = self.sc(quote! { bound::add });
        let terms = fields.iter().map(|f| {
            if f.opaque {
                quote! { None }
            } else {
                let ty = &f.ty;
                quote! { <#ty as #trait_<'i>>::#which }
            }
        });
        quote! {
            {
                let __b: Option<usize> = #start;
                #(let __b = #add(__b, #terms);)*
                __b
            }
        }
    }

    /// `<'i, T, N, __N>`: the store type's arguments.
    fn store_args(&self) -> TokenStream {
        let lifetime = maybe_quote(self.has_lifetime, || quote! { 'i, });
        let idents = self.generics.params.iter().map(|p| match p {
            syn::GenericParam::Type(ty) => &ty.ident,
            syn::GenericParam::Const(c) => &c.ident,
            syn::GenericParam::Lifetime(_) => unreachable!("lifetimes are not kept"),
        });
        quote! { <#lifetime #(#idents,)* __N> }
    }

    /// The store type's definition, named `#store_ident`, with `fields` (`name: type,` each, `#[cfg]`s included),
    /// and its `impl SeriesStore` with the bodies given; `init` writes every field of the store at `__at` in place
    /// (`empty_store` goes through it).
    #[allow(clippy::too_many_arguments)]
    fn store_impl(
        &self,
        store_ident: &Ident,
        fields: TokenStream,
        init: TokenStream,
        set: TokenStream,
        get: TokenStream,
        columnar: TokenStream,
        write: TokenStream,
    ) -> TokenStream {
        let cfg = self.cfg.map(|cond| quote! { #[cfg(#cond)] });
        let ident = self.ident;
        let lifetime = self.lifetime();
        let store_trait = self.sc(quote! { SeriesStore });
        let params = self.generics.impl_params(Some(store_trait.clone()));
        let where_clause = self.generics.where_clause(&[]);
        let def_lifetime = maybe_quote(self.has_lifetime, || quote! { 'i, });
        let marker_lifetime = maybe_quote(self.has_lifetime, || quote! { &'i (), });
        let type_params: Vec<_> = self.generics.type_params().map(|p| &p.ident).collect();
        let args = self.store_args();
        let bit_sink = self.sc(quote! { BitSink });
        let series_policy = self.sc(quote! { SeriesPolicy });
        let level = self.sc(quote! { Level });
        let error = self.cp.error();
        let empty_store_from_init = self.sc(quote! { empty_store_from_init });
        let doc = format!("The `SeriesStore` of [{ident}]: an array per leaf field.");
        quote! {
            #cfg
            #[doc = #doc]
            #[doc(hidden)]
            #[allow(non_camel_case_types)]
            pub struct #store_ident<#def_lifetime #params const __N: usize> #where_clause {
                #fields
                __params: ::core::marker::PhantomData<fn() -> (#marker_lifetime #(#type_params,)*)>,
            }

            #cfg
            impl<#def_lifetime #params> #store_trait for #ident #lifetime #where_clause {
                type Store<const __N: usize> = #store_ident #args;

                fn empty_store<const __N: usize>() -> Self::Store<__N> {
                    #empty_store_from_init::<Self, __N>()
                }

                fn init_store<const __N: usize>(
                    slot: &mut ::core::mem::MaybeUninit<Self::Store<__N>>,
                ) -> &mut Self::Store<__N> {
                    let __at = slot.as_mut_ptr();
                    // SAFETY: every field of the store written in place, each through its own pointer
                    unsafe {
                        #init
                        ::core::ptr::addr_of_mut!((*__at).__params).write(::core::marker::PhantomData);
                        slot.assume_init_mut()
                    }
                }

                #[allow(unused_variables)]
                fn set<const __N: usize>(store: &mut Self::Store<__N>, i: usize, item: &Self) {
                    #set
                }

                #[allow(unused_variables)]
                fn get<const __N: usize>(store: &Self::Store<__N>, i: usize) -> Self {
                    #get
                }

                #[allow(unused_variables)]
                fn columnar<const __N: usize>(
                    store: &Self::Store<__N>,
                    rows: ::core::ops::Range<usize>,
                ) -> bool {
                    #columnar
                }

                #[allow(unused_variables)]
                fn write_store<__S: #bit_sink, __P: #series_policy + ?Sized, const __N: usize>(
                    store: &Self::Store<__N>,
                    rows: ::core::ops::Range<usize>,
                    sink: &mut __S,
                    policy: &__P,
                    level: #level,
                ) -> Result<(), #error> {
                    #write
                }
            }
        }
    }

    /// `impl SeriesStore` of a struct: a store per field; columnar when every field is (an earlier field then
    /// restarts no later one), written field by field after the head of an evolvable struct.
    fn series_store_struct(&self, fields: &[SeriesField], evolvable: bool) -> TokenStream {
        if !self.store || fields.iter().any(|f| f.opaque) {
            return quote! {};
        }
        let store_trait = self.sc(quote! { SeriesStore });
        let store_ident = format_ident!("__{}SeriesStore", self.ident);
        let members: Vec<_> = fields.iter().map(|f| &f.member).collect();
        let tys: Vec<_> = fields.iter().map(|f| &f.ty).collect();
        let level = self.sc(quote! { Level });
        let init_store_at = self.sc(quote! { init_store_at });
        let n = fields.len() as u32;
        let head = maybe_quote(evolvable, || {
            let write_struct_head = self.sc(quote! { write_struct_head });
            quote! {
                #write_struct_head(sink, level, true, #n, |counter| {
                    <Self as #store_trait>::write_store::<_, __P, __N>(
                        store, rows.clone(), counter, policy, #level::Fields,
                    )
                })?;
            }
        });
        self.store_impl(
            &store_ident,
            quote! { #(#members: <#tys as #store_trait>::Store<__N>,)* },
            quote! {
                #(#init_store_at::<#tys, __N>(::core::ptr::addr_of_mut!((*__at).#members));)*
            },
            quote! { #(<#tys as #store_trait>::set::<__N>(&mut store.#members, i, &item.#members);)* },
            quote! { Self { #(#members: <#tys as #store_trait>::get::<__N>(&store.#members, i)),* } },
            quote! { true #(&& <#tys as #store_trait>::columnar::<__N>(&store.#members, rows.clone()))* },
            quote! {
                #head
                #(<#tys as #store_trait>::write_store::<__S, __P, __N>(
                    &store.#members, rows.clone(), sink, policy, #level::Nested,
                )?;)*
                Ok(())
            },
        )
    }

    /// `impl SeriesStore` of an enum: an array of discriminants (all of a `#[future_version]` one's `u32`, else the
    /// smallest unsigned type that holds the largest), and a store per field of each data variant, written only for
    /// the items in that variant.
    fn series_store_enum(
        &self,
        variants: &[Variant],
        data: &[(&Variant, Vec<SeriesField>)],
        future_version: Option<&Ident>,
        max_tag: u32,
    ) -> TokenStream {
        if !self.store
            || data
                .iter()
                .any(|(_, fields)| fields.iter().any(|f| f.opaque))
        {
            return quote! {};
        }
        let store_trait = self.sc(quote! { SeriesStore });
        let store_ident = format_ident!("__{}SeriesStore", self.ident);
        let init_store_at = self.sc(quote! { init_store_at });
        let fill_array = self.sc(quote! { fill_array });
        let tag_ty = match (future_version, max_tag) {
            (Some(_), _) => quote! { u32 },
            (None, 0..=0xFF) => quote! { u8 },
            (None, 0x100..=0xFFFF) => quote! { u16 },
            (None, _) => quote! { u32 },
        };
        let d = |v: &Variant| proc_macro2::Literal::u32_unsuffixed(v.discriminant);
        let slot = |v: &Variant, f: &SeriesField| {
            format_ident!("__v{}_{}", v.discriminant, f.member.to_string())
        };
        let bind = |f: &SeriesField| format_ident!("__x_{}", f.member.to_string());
        // `Self::V { a: __x_a, .. }` / `Self::V(__x__0, ..)`
        let pattern = |v: &Variant, fields: &[SeriesField]| -> TokenStream {
            let vi = &v.ident;
            let binds = fields.iter().map(bind);
            match &v.fields {
                Fields::Named(_) => {
                    let members = fields.iter().map(|f| &f.member);
                    quote! { Self::#vi { #(#members: #binds),* } }
                }
                _ => quote! { Self::#vi(#(#binds),*) },
            }
        };

        let mut store_fields = vec![];
        let mut empties = vec![];
        let mut set_arms = vec![];
        for (v, fields) in data {
            let on = v.cfg_attrs();
            let slots: Vec<_> = fields.iter().map(|f| slot(v, f)).collect();
            let tys: Vec<_> = fields.iter().map(|f| &f.ty).collect();
            let binds: Vec<_> = fields.iter().map(bind).collect();
            for (s, ty) in slots.iter().zip(&tys) {
                store_fields.push(quote! { #on #s: <#ty as #store_trait>::Store<__N>, });
                empties.push(quote! {
                    #on #init_store_at::<#ty, __N>(::core::ptr::addr_of_mut!((*__at).#s));
                });
            }
            let pat = pattern(v, fields);
            set_arms.push(quote! {
                #on
                #pat => { #(<#tys as #store_trait>::set::<__N>(&mut store.#slots, i, #binds);)* }
            });
        }
        let get_arms = variants.iter().map(|v| {
            let on = v.cfg_attrs();
            let dv = d(v);
            let vi = &v.ident;
            let value = match data.iter().find(|(dv, _)| dv.ident == v.ident) {
                Some((_, fields)) => {
                    let gets = fields.iter().map(|f| {
                        let ty = &f.ty;
                        let s = slot(v, f);
                        quote! { <#ty as #store_trait>::get::<__N>(&store.#s, i) }
                    });
                    match &v.fields {
                        Fields::Named(_) => {
                            let members = fields.iter().map(|f| &f.member);
                            quote! { Self::#vi { #(#members: #gets),* } }
                        }
                        _ => quote! { Self::#vi(#(#gets),*) },
                    }
                }
                None => match &v.fields {
                    Fields::Unit => quote! { Self::#vi },
                    Fields::Named(_) => quote! { Self::#vi {} },
                    Fields::Unnamed(_) => quote! { Self::#vi() },
                },
            };
            quote! { #on #dv => #value, }
        });
        // a fresh store's tag: the first variant compiled in
        let fill_steps = variants.iter().map(|v| {
            let on = v.cfg_attrs();
            let dv = d(v);
            quote! { #on { if __d.is_none() { __d = Some(#dv); } } }
        });
        // columnar: one variant over the rows (a future one isn't: the item walk rejects it), its fields columnar
        let level = self.sc(quote! { Level });
        let write_enum_tags = self.sc(quote! { write_enum_tags });
        let known: Vec<&Variant> = variants
            .iter()
            .filter(|v| Some(&v.ident) != future_version)
            .collect();
        let columnar_arms = known.iter().map(|v| {
            let on = v.cfg_attrs();
            let dv = d(v);
            let fields = data
                .iter()
                .find(|(dv, _)| dv.ident == v.ident)
                .map(|(_, f)| f.as_slice())
                .unwrap_or_default();
            let slots = fields.iter().map(|f| slot(v, f));
            let tys = fields.iter().map(|f| &f.ty);
            quote! {
                #on #dv => true #(&& <#tys as #store_trait>::columnar::<__N>(&store.#slots, rows.clone()))*,
            }
        });
        let write_arms = data.iter().map(|(v, fields)| {
            let on = v.cfg_attrs();
            let dv = d(v);
            let slots = fields.iter().map(|f| slot(v, f));
            let tys = fields.iter().map(|f| &f.ty);
            quote! {
                #on #dv => {
                    #(<#tys as #store_trait>::write_store::<__S, __P, __N>(
                        &store.#slots, rows.clone(), sink, policy, #level::Nested,
                    )?;)*
                }
            }
        });
        let columnar = quote! {
            let __tags = &store.__tag[rows.clone()];
            let __t = __tags[0];
            if !__tags.iter().all(|t| *t == __t) {
                return false;
            }
            #[allow(unreachable_patterns)]
            match __t as u32 {
                #(#columnar_arms)*
                _ => false,
            }
        };
        let write = quote! {
            let __t = store.__tag[rows.start] as u32;
            #write_enum_tags(sink, ::core::iter::repeat_n(__t, rows.len()))?;
            #[allow(unreachable_patterns, clippy::match_single_binding)]
            match __t {
                #(#write_arms)*
                _ => {}
            }
            Ok(())
        };
        let (set_tag, other) = match future_version {
            Some(fv) => (
                quote! {
                    store.__tag[i] = match item {
                        Self::#fv(__d) => *__d,
                        _ => item.discriminant() as #tag_ty,
                    };
                },
                quote! { __d => Self::#fv(__d), },
            ),
            None => (
                quote! { store.__tag[i] = item.discriminant() as #tag_ty; },
                quote! { _ => unreachable!("an enum without variants has no items"), },
            ),
        };
        self.store_impl(
            &store_ident,
            quote! {
                __tag: [#tag_ty; __N],
                #(#store_fields)*
            },
            quote! {
                #[allow(unused_mut)]
                const __FILL: #tag_ty = {
                    let mut __d: Option<#tag_ty> = None;
                    #(#fill_steps)*
                    match __d {
                        Some(__d) => __d,
                        None => 0,
                    }
                };
                #fill_array(
                    &mut *::core::ptr::addr_of_mut!((*__at).__tag)
                        .cast::<::core::mem::MaybeUninit<[#tag_ty; __N]>>(),
                    __FILL,
                );
                #(#empties)*
            },
            quote! {
                #set_tag
                #[allow(unreachable_patterns)]
                match item {
                    #(#set_arms)*
                    _ => {}
                }
            },
            quote! {
                #[allow(clippy::match_single_binding, unreachable_patterns)]
                match store.__tag[i] {
                    #(#get_arms)*
                    #other
                }
            },
            columnar,
            write,
        )
    }

    /// The cells type's definition, named `#cells_ident`, with `fields`, and its `impl SeriesCells` with the
    /// bodies given (API-50, observe cells).
    fn cells_impl(
        &self,
        cells_ident: &Ident,
        fields: TokenStream,
        put: TokenStream,
        copy: TokenStream,
    ) -> TokenStream {
        let cfg = self.cfg.map(|cond| quote! { #[cfg(#cond)] });
        let ident = self.ident;
        let lifetime = self.lifetime();
        let cells_trait = self.sc(quote! { SeriesCells });
        let params = self.generics.impl_params(Some(cells_trait.clone()));
        let where_clause = self.generics.where_clause(&[]);
        let def_lifetime = maybe_quote(self.has_lifetime, || quote! { 'i, });
        let marker_lifetime = maybe_quote(self.has_lifetime, || quote! { &'i (), });
        let type_params: Vec<_> = self.generics.type_params().map(|p| &p.ident).collect();
        let lifetime_arg = maybe_quote(self.has_lifetime, || quote! { 'i, });
        let idents = self.generics.params.iter().map(|p| match p {
            syn::GenericParam::Type(ty) => &ty.ident,
            syn::GenericParam::Const(c) => &c.ident,
            syn::GenericParam::Lifetime(_) => unreachable!("lifetimes are not kept"),
        });
        let args = quote! { <#lifetime_arg #(#idents,)* __R> };
        let doc = format!("The `SeriesCells` of [{ident}]: an array of atomics per leaf field.");
        quote! {
            #cfg
            #[doc = #doc]
            #[doc(hidden)]
            #[allow(non_camel_case_types)]
            pub struct #cells_ident<#def_lifetime #params const __R: usize> #where_clause {
                #fields
                __params: ::core::marker::PhantomData<fn() -> (#marker_lifetime #(#type_params,)*)>,
            }

            #cfg
            // SAFETY: the cells are the fields' cells and a `PhantomData`, all valid all zero
            unsafe impl<#def_lifetime #params> #cells_trait for #ident #lifetime #where_clause {
                type Cells<const __R: usize> = #cells_ident #args;

                #[inline(always)]
                #[allow(unused_variables)]
                fn put<const __R: usize>(cells: &Self::Cells<__R>, i: usize, item: &Self) {
                    #put
                }

                #[allow(unused_variables)]
                fn copy_rows<const __R: usize, const __N: usize>(
                    cells: &Self::Cells<__R>,
                    from: usize,
                    store: &mut Self::Store<__N>,
                    to: usize,
                    n: usize,
                ) {
                    #copy
                }
            }
        }
    }

    /// `impl SeriesCells` of a struct: cells per field, copied column by column.
    fn series_cells_struct(&self, fields: &[SeriesField]) -> TokenStream {
        if !self.store || !self.cells || fields.iter().any(|f| !f.cells) {
            return quote! {};
        }
        let cells_trait = self.sc(quote! { SeriesCells });
        let cells_ident = format_ident!("__{}SeriesCells", self.ident);
        let members: Vec<_> = fields.iter().map(|f| &f.member).collect();
        let tys: Vec<_> = fields.iter().map(|f| &f.ty).collect();
        self.cells_impl(
            &cells_ident,
            quote! { #(#members: <#tys as #cells_trait>::Cells<__R>,)* },
            quote! { #(<#tys as #cells_trait>::put::<__R>(&cells.#members, i, &item.#members);)* },
            quote! {
                #(<#tys as #cells_trait>::copy_rows::<__R, __N>(&cells.#members, from, &mut store.#members, to, n);)*
            },
        )
    }

    /// `impl SeriesCells` of an enum: the discriminants, and cells per field of each data variant, written and
    /// copied out only for the items in that variant (as `SeriesStore::set` does).
    fn series_cells_enum(
        &self,
        data: &[(&Variant, Vec<SeriesField>)],
        future_version: Option<&Ident>,
        max_tag: u32,
    ) -> TokenStream {
        if !self.store
            || !self.cells
            || data
                .iter()
                .any(|(_, fields)| fields.iter().any(|f| !f.cells))
        {
            return quote! {};
        }
        let cells_trait = self.sc(quote! { SeriesCells });
        let word = self.sc(quote! { CellWord });
        let cells_ident = format_ident!("__{}SeriesCells", self.ident);
        let tag_ty = match (future_version, max_tag) {
            (Some(_), _) => quote! { u32 },
            (None, 0..=0xFF) => quote! { u8 },
            (None, 0x100..=0xFFFF) => quote! { u16 },
            (None, _) => quote! { u32 },
        };
        let d = |v: &Variant| proc_macro2::Literal::u32_unsuffixed(v.discriminant);
        let slot = |v: &Variant, f: &SeriesField| {
            format_ident!("__v{}_{}", v.discriminant, f.member.to_string())
        };
        let bind = |f: &SeriesField| format_ident!("__x_{}", f.member.to_string());
        let pattern = |v: &Variant, fields: &[SeriesField]| -> TokenStream {
            let vi = &v.ident;
            let binds = fields.iter().map(bind);
            match &v.fields {
                Fields::Named(_) => {
                    let members = fields.iter().map(|f| &f.member);
                    quote! { Self::#vi { #(#members: #binds),* } }
                }
                _ => quote! { Self::#vi(#(#binds),*) },
            }
        };
        let mut cells_fields = vec![];
        let mut put_arms = vec![];
        let mut copy_arms = vec![];
        for (v, fields) in data {
            let on = v.cfg_attrs();
            let dv = d(v);
            let slots: Vec<_> = fields.iter().map(|f| slot(v, f)).collect();
            let tys: Vec<_> = fields.iter().map(|f| &f.ty).collect();
            let binds: Vec<_> = fields.iter().map(bind).collect();
            for (s, ty) in slots.iter().zip(&tys) {
                cells_fields.push(quote! { #on #s: <#ty as #cells_trait>::Cells<__R>, });
            }
            let pat = pattern(v, fields);
            put_arms.push(quote! {
                #on
                #pat => { #(<#tys as #cells_trait>::put::<__R>(&cells.#slots, i, #binds);)* }
            });
            copy_arms.push(quote! {
                #on
                #dv => { #(<#tys as #cells_trait>::copy_rows::<__R, __N>(&cells.#slots, from + k, &mut store.#slots, to + k, 1);)* }
            });
        }
        let tag = match future_version {
            Some(fv) => quote! {
                match item {
                    Self::#fv(__d) => *__d,
                    _ => item.discriminant() as #tag_ty,
                }
            },
            None => quote! { item.discriminant() as #tag_ty },
        };
        let copy = if data.is_empty() {
            quote! {
                for k in 0..n {
                    store.__tag[to + k] = <#tag_ty as #word>::load::<__R>(&cells.__tag, from + k);
                }
            }
        } else {
            quote! {
                for k in 0..n {
                    let __t = <#tag_ty as #word>::load::<__R>(&cells.__tag, from + k);
                    store.__tag[to + k] = __t;
                    #[allow(unreachable_patterns, clippy::match_single_binding)]
                    match __t {
                        #(#copy_arms)*
                        _ => {}
                    }
                }
            }
        };
        self.cells_impl(
            &cells_ident,
            quote! {
                __tag: <#tag_ty as #word>::Cells<__R>,
                #(#cells_fields)*
            },
            quote! {
                <#tag_ty as #word>::put::<__R>(&cells.__tag, i, #tag);
                #[allow(unreachable_patterns)]
                match item {
                    #(#put_arms)*
                    _ => {}
                }
            },
            copy,
        )
    }

    fn fields<'f>(
        &self,
        fields: impl Iterator<Item = (TokenStream, &'f Type, bool, Option<&'f syn::Expr>)>,
        owner: &str,
    ) -> Result<Vec<SeriesField<'f>>, String> {
        let mut out = vec![];
        for (member, ty, opaque, default) in fields {
            if matches!(ty, Type::IsSome(_) | Type::IsOk(_)) {
                continue;
            }
            if !opaque {
                check_ty(ty, owner, &member.to_string())?;
            }
            out.push(SeriesField {
                member,
                ty: ty.def(self.is_ref, self.cp),
                opaque,
                tagged: !opaque && matches!(ty, Type::External(_, _) | Type::Option(_, _)),
                cells: !opaque && has_cells(ty),
                default,
            });
        }
        Ok(out)
    }

    fn reader_ty(&self, f: &SeriesField) -> TokenStream {
        let ty = &f.ty;
        if f.opaque {
            self.sc(quote! { OpaqueReader<'i> })
        } else {
            let trait_ = self.sc(quote! { SeriesColumns });
            quote! { <#ty as #trait_<'i>>::Reader }
        }
    }

    /// `[Option<u32>; N]` of the fields' tags, from `#source` (an item, or a binding per field).
    fn tags_array(
        &self,
        fields: &[SeriesField],
        value: impl Fn(&SeriesField) -> TokenStream,
    ) -> TokenStream {
        let trait_ = self.sc(quote! { SeriesColumns });
        let tags = fields.iter().map(|f| {
            if f.tagged {
                let ty = &f.ty;
                let v = value(f);
                // a field that can't have a tag is a constant `None`, compared with nothing
                quote! {
                    if <#ty as #trait_<'i>>::TAGGED { <#ty as #trait_<'i>>::series_tag(#v) } else { None }
                }
            } else {
                quote! { None }
            }
        });
        quote! { [#(#tags),*] }
    }

    /// Writes a field's group: `items` the parent's items, `pick` the closure from an item to `Option<&field>`,
    /// `earlier` the fields declared before it, `__tags` the closure from an item to the tags of its fields (when
    /// one of them may have a tag).
    fn write_field(
        &self,
        f: &SeriesField,
        earlier: &[&SeriesField],
        pick: TokenStream,
    ) -> TokenStream {
        let ty = &f.ty;
        let trait_ = self.sc(quote! { SeriesColumns });
        let field_items = self.sc(quote! { FieldItems });
        let level = self.sc(quote! { Level });
        if f.opaque {
            let opaque = self.sc(quote! { OpaqueField });
            let write = if self.is_ref {
                quote! { write_group }
            } else {
                quote! { write_group_owned }
            };
            // runs restart nothing and don't care for restarts
            return quote! {
                {
                    let __pick = #pick;
                    #opaque::<#ty>::#write(items.clone().filter_map(|(s, r)| __pick(s).map(|v| (v, r))), sink)?;
                }
            };
        }
        let tagged: Vec<syn::Index> = earlier
            .iter()
            .enumerate()
            .filter(|(_, f)| f.tagged)
            .map(|(k, _)| syn::Index::from(k))
            .collect();
        let earlier = if !tagged.is_empty() {
            // unrolled over the earlier fields that may have a tag: those whose type can't fold away
            let tag_changed = self.sc(quote! { tag_changed });
            quote! {
                move |p: &'a Self, c: &'a Self| {
                    let (__p, __c) = (__tags(p), __tags(c));
                    false #(|| #tag_changed(__p[#tagged], __c[#tagged]))*
                }
            }
        } else {
            let no_earlier = self.sc(quote! { no_earlier });
            quote! { #no_earlier::<Self> }
        };
        quote! {
            {
                let __field = #field_items::new(items.clone(), #pick, #earlier);
                <#ty as #trait_<'i>>::write_group(__field, sink, policy, #level::Nested)?;
            }
        }
    }

    /// Skims field `j` (as the expression of its reader), `ctx` the parent's context, `__runs` the tag runs of the
    /// fields before it.
    fn skim_field(&self, f: &SeriesField, j: usize, ctx: TokenStream) -> TokenStream {
        let ty = &f.ty;
        let sibling_ctx = self.sc(quote! { SiblingCtx });
        if f.opaque {
            let opaque = self.sc(quote! { OpaqueField });
            let skim = if self.is_ref {
                quote! { skim }
            } else {
                quote! { skim_owned }
            };
            return quote! { #opaque::<#ty>::#skim(rd, &#sibling_ctx::new(#ctx, __runs))? };
        }
        let trait_ = self.sc(quote! { SeriesColumns });
        let level = self.sc(quote! { Level });
        let record = maybe_quote(f.tagged, || {
            quote! { __runs[#j] = <#ty as #trait_<'i>>::tag_run(&__r); }
        });
        quote! {
            {
                let __r = <#ty as #trait_<'i>>::skim(rd, #sibling_ctx::new(#ctx, __runs), #level::Nested)?;
                #record
                __r
            }
        }
    }

    /// The next value of field `j` from reader `#r` (a `&mut`), `__sib` the `SiblingTags` of the item.
    fn next_field(
        &self,
        f: &SeriesField,
        j: usize,
        r: TokenStream,
        restart: TokenStream,
    ) -> TokenStream {
        let ty = &f.ty;
        if f.opaque {
            let opaque = self.sc(quote! { OpaqueField });
            let next = if self.is_ref {
                quote! { next }
            } else {
                quote! { next_owned }
            };
            return quote! { #opaque::<#ty>::#next(#r)? };
        }
        let trait_ = self.sc(quote! { SeriesColumns });
        let record = maybe_quote(f.tagged, || {
            quote! { __sib.record(#j, <#ty as #trait_<'i>>::series_tag(&__v)); }
        });
        quote! {
            {
                let __v = <#ty as #trait_<'i>>::next(#r, __sib.restart(#restart))?;
                #record
                __v
            }
        }
    }

    /// `impl SeriesColumns` of a struct with named fields (or none), `evolvable` when it is `Unsized`.
    pub(crate) fn series_struct(
        &self,
        fields: &[Field],
        evolvable: bool,
    ) -> Result<TokenStream, String> {
        let owner = self.ident.to_string();
        let fields = self.fields(
            fields.iter().map(|f| {
                let ident = &f.ident;
                (
                    quote! { #ident },
                    &f.ty,
                    f.series_opaque,
                    f.default.as_ref(),
                )
            }),
            &owner,
        )?;
        let n = fields.len();
        let n_u32 = n as u32;
        let cfg = self.cfg.map(|cond| quote! { #[cfg(#cond)] });
        let ident = self.ident;
        let lifetime = self.lifetime();
        let trait_ = self.sc(quote! { SeriesColumns });
        let series_policy = self.sc(quote! { SeriesPolicy });
        let level = self.sc(quote! { Level });
        let bit_sink = self.sc(quote! { BitSink });
        let ctx_trait = self.sc(quote! { SeriesCtx });
        let sibling_tags = self.sc(quote! { SiblingTags });
        let tag_run = self.sc(quote! { TagRun });
        let write_struct_head = self.sc(quote! { write_struct_head });
        let skim_struct_head = self.sc(quote! { skim_struct_head });
        let error = self.cp.error();
        let buf_reader = self.cp.item(quote! { BufReader });

        let readers = fields.iter().map(|f| self.reader_ty(f));
        let any_tagged = fields.iter().any(|f| f.tagged);
        let tags_fn = maybe_quote(any_tagged, || {
            let tags = self.tags_array(&fields, |f| {
                let m = &f.member;
                quote! { &s.#m }
            });
            quote! { let __tags = |s: &'a Self| -> [Option<u32>; #n] { #tags }; }
        });
        let writes = fields.iter().enumerate().map(|(j, f)| {
            let m = &f.member;
            let earlier: Vec<&SeriesField> = fields[..j].iter().collect();
            self.write_field(f, &earlier, quote! { |s: &'a Self| Some(&s.#m) })
        });
        let skims = fields.iter().enumerate().map(|(j, f)| {
            let skim = self.skim_field(f, j, quote! { ctx.clone() });
            // an older writer's struct without the field: its default, or malformed when it has none
            let missing = match f.default {
                Some(_) => quote! { None },
                None => quote! { return Err(#error::MalformedSeries) },
            };
            quote! {
                if __head.has_field(rd, #j) { Some(#skim) } else { #missing },
            }
        });
        let locals: Vec<Ident> = fields
            .iter()
            .map(|f| format_ident!("__f_{}", f.member.to_string()))
            .collect();
        let nexts = fields.iter().enumerate().map(|(j, f)| {
            let idx = syn::Index::from(j);
            let local = &locals[j];
            let next = self.next_field(f, j, quote! { __r }, quote! { restart });
            let missing = match f.default {
                Some(default) => quote! { #default },
                // rejected by the skim
                None => quote! { return Err(#error::MalformedSeries) },
            };
            quote! {
                let #local = match &mut __readers.#idx {
                    Some(__r) => #next,
                    None => #missing,
                };
            }
        });
        let members = fields.iter().map(|f| &f.member);

        let max_item = self.bound_sum(&fields, quote! { MAX_ITEM_BITS }, quote! { Some(0) });
        let head = self.sc(quote! { bound::STRUCT_HEAD_BITS });
        let max_fixed = self.bound_sum(
            &fields,
            quote! { MAX_FIXED_BITS },
            if evolvable {
                quote! { Some(#head) }
            } else {
                quote! { Some(0) }
            },
        );
        let store = self.series_store_struct(&fields, evolvable);
        let cells = self.series_cells_struct(&fields);
        // only an evolvable struct has a head: the measuring pass behind it would instantiate every column writer
        // once more for the counter
        let head = maybe_quote(evolvable, || {
            quote! {
                #write_struct_head(sink, level, true, #n_u32, |counter| {
                    <Self as #trait_<'i>>::write_group(items.clone(), counter, policy, #level::Fields)
                })?;
            }
        });

        let params = self.impl_params();
        let where_clause = self.generics.where_clause(&[]);
        Ok(quote! {
            #store
            #cells

            #cfg
            impl<'i, #params> #trait_<'i> for #ident #lifetime #where_clause {
                type Reader = (#sibling_tags<#n>, (#(Option<#readers>,)*));
                const MAX_ITEM_BITS: Option<usize> = #max_item;
                const MAX_FIXED_BITS: Option<usize> = #max_fixed;
                const TAGGED: bool = false;

                fn write_group<'a, I, S, P>(
                    items: I,
                    sink: &mut S,
                    policy: &P,
                    level: #level,
                ) -> Result<(), #error>
                where
                    I: Iterator<Item = (&'a Self, bool)> + Clone,
                    S: #bit_sink,
                    P: #series_policy + ?Sized,
                    Self: 'a,
                {
                    if items.clone().next().is_none() {
                        return Ok(());
                    }
                    #head
                    #tags_fn
                    #(#writes)*
                    Ok(())
                }

                #[allow(unused_assignments, unused_mut)]
                fn skim<C: #ctx_trait>(
                    rd: &mut #buf_reader<'i>,
                    ctx: C,
                    level: #level,
                ) -> Result<Self::Reader, #error> {
                    let __head = #skim_struct_head(rd, &ctx, level, #evolvable)?;
                    let mut __runs: [Option<#tag_run<'i>>; #n] = [None; #n];
                    let __readers = (#(#skims)*);
                    __head.finish(rd, #n)?;
                    Ok((#sibling_tags::new(), __readers))
                }

                #[allow(unused_variables)]
                fn next(reader: &mut Self::Reader, restart: bool) -> Result<Self, #error> {
                    let (__sib, __readers) = reader;
                    __sib.begin();
                    #(#nexts)*
                    Ok(Self { #(#members: #locals),* })
                }
            }
        })
    }

    /// `impl SeriesColumns` of an enum, `future_version` the name of its `#[future_version]` variant: such an item
    /// can't be written (its fields are unknown), an unknown tag is rejected on reading.
    pub(crate) fn series_enum(
        &self,
        variants: &[Variant],
        future_version: Option<&Ident>,
    ) -> Result<TokenStream, String> {
        let owner = self.ident.to_string();
        let cfg = self.cfg.map(|cond| quote! { #[cfg(#cond)] });
        let ident = self.ident;
        let lifetime = self.lifetime();
        let trait_ = self.sc(quote! { SeriesColumns });
        let series_policy = self.sc(quote! { SeriesPolicy });
        let level = self.sc(quote! { Level });
        let bit_sink = self.sc(quote! { BitSink });
        let ctx_trait = self.sc(quote! { SeriesCtx });
        let sibling_tags = self.sc(quote! { SiblingTags });
        let tag_run = self.sc(quote! { TagRun });
        let enum_tags = self.sc(quote! { EnumTags });
        let variant_ctx = self.sc(quote! { VariantCtx });
        let write_enum_tags = self.sc(quote! { write_enum_tags });
        let error = self.cp.error();
        let buf_reader = self.cp.item(quote! { BufReader });

        // data variants in discriminant order, with their fields as a Series sees them
        let mut data: Vec<(&Variant, Vec<SeriesField>)> = vec![];
        for v in variants {
            let vowner = format!("{owner}::{}", v.ident);
            let fields = match &v.fields {
                Fields::Unit => continue,
                Fields::Named(named) => self.fields(
                    named.iter().map(|f| {
                        let i = &f.ident;
                        (quote! { #i }, &f.ty, f.series_opaque, None)
                    }),
                    &vowner,
                )?,
                Fields::Unnamed(types) => self.fields(
                    types
                        .iter()
                        .filter(|ty| !matches!(ty, Type::IsSome(_) | Type::IsOk(_)))
                        .enumerate()
                        .map(|(k, ty)| {
                            let i = format_ident!("_{k}");
                            (quote! { #i }, ty, false, None)
                        }),
                    &vowner,
                )?,
            };
            if !fields.is_empty() {
                data.push((v, fields));
            }
        }
        data.sort_by_key(|(v, _)| v.discriminant);
        let reader_ident = format_ident!("__{}SeriesReader", ident);

        // `Self::V { a: __x_a, .. }` / `Self::V(__x__0, ..)`: a pattern binding every field
        let pattern = |v: &Variant, fields: &[SeriesField]| -> TokenStream {
            let vi = &v.ident;
            let binds = fields
                .iter()
                .map(|f| format_ident!("__x_{}", f.member.to_string()));
            match &v.fields {
                Fields::Named(_) => {
                    let members = fields.iter().map(|f| &f.member);
                    quote! { Self::#vi { #(#members: #binds),* } }
                }
                _ => quote! { Self::#vi(#(#binds),*) },
            }
        };
        let bind = |f: &SeriesField| {
            let b = format_ident!("__x_{}", f.member.to_string());
            quote! { #b }
        };

        // the tag run, checked for every value
        let discriminant = |v: &Variant| proc_macro2::Literal::u32_unsuffixed(v.discriminant);
        let tag_arms = variants.iter().map(|v| {
            let d = discriminant(v);
            let on = v.cfg_attrs();
            let off = v.not_cfg_attr().map(|off| {
                quote! { #off #d => Err(#error::EnumVariantDisabled), }
            });
            quote! { #on #d => Ok(()), #off }
        });
        let future_check = future_version.map(|fv| {
            quote! {
                if items.clone().any(|(e, _)| matches!(e, Self::#fv(_))) {
                    return Err(#error::EnumFutureVersionOrMalformedData);
                }
            }
        });

        let mut writes = vec![];
        let mut skims = vec![];
        let mut reader_fields = vec![];
        let mut reader_inits = vec![];
        let mut reader_clones = vec![];
        let mut data_arms = vec![];
        for (v, fields) in &data {
            let d = discriminant(v);
            let on = v.cfg_attrs();
            let k = fields.len();
            let pat = pattern(v, fields);
            let vfield = format_ident!("__v{}", v.discriminant);
            let any_tagged = fields.iter().any(|f| f.tagged);
            let tags_fn = maybe_quote(any_tagged, || {
                let tags = self.tags_array(fields, |f| bind(f));
                quote! {
                    #[allow(unreachable_patterns, unused_variables)]
                    let __tags = |e: &'a Self| -> [Option<u32>; #k] {
                        match e {
                            #pat => #tags,
                            _ => [None; #k],
                        }
                    };
                }
            });
            let field_writes = fields.iter().enumerate().map(|(j, f)| {
                let b = bind(f);
                let earlier: Vec<&SeriesField> = fields[..j].iter().collect();
                let pick = quote! {
                    #[allow(unreachable_patterns, unused_variables)]
                    |e: &'a Self| match e {
                        #pat => Some(#b),
                        _ => None,
                    }
                };
                self.write_field(f, &earlier, pick)
            });
            // discriminants from 63 up share the last bit: such a variant is written when any of them is seen
            let seen_bit = v.discriminant.min(63);
            writes.push(quote! {
                #on
                if __seen & (1u64 << #seen_bit) != 0 {
                    #tags_fn
                    #(#field_writes)*
                }
            });

            let readers = fields.iter().map(|f| self.reader_ty(f));
            reader_fields.push(quote! { #on #vfield: (#sibling_tags<#k>, (#(#readers,)*)), });
            let field_skims = fields
                .iter()
                .enumerate()
                .map(|(j, f)| self.skim_field(f, j, quote! { __vctx.clone() }));
            skims.push(quote! {
                #on
                #[allow(unused_assignments, unused_mut)]
                let #vfield = {
                    let __vctx = #variant_ctx::new(ctx.clone(), __tags.tag_run(), #d);
                    let mut __runs: [Option<#tag_run<'i>>; #k] = [None; #k];
                    let __rs = (#(#field_skims,)*);
                    (#sibling_tags::new(), __rs)
                };
            });
            reader_inits.push(quote! { #on #vfield, });
            reader_clones.push(quote! { #on #vfield: self.#vfield.clone(), });

            let field_nexts = fields.iter().enumerate().map(|(j, f)| {
                let idx = syn::Index::from(j);
                let b = bind(f);
                let next = self.next_field(f, j, quote! { &mut __rs.#idx }, quote! { __restart });
                quote! { let #b = #next; }
            });
            data_arms.push(quote! {
                #on
                #d => {
                    let (__sib, __rs) = &mut reader.#vfield;
                    let __restart = restart || __entered;
                    __sib.begin();
                    #(#field_nexts)*
                    Ok(#pat)
                }
            });
        }
        let unit_arms = variants
            .iter()
            .filter(|v| matches!(v.fields, Fields::Unit))
            .map(|v| {
                let d = discriminant(v);
                let on = v.cfg_attrs();
                let vi = &v.ident;
                quote! { #on #d => Ok(Self::#vi), }
            });
        // `V {}` and `V()`: no columns, like unit variants
        let empty_arms = variants
            .iter()
            .filter(|v| {
                !matches!(v.fields, Fields::Unit) && !data.iter().any(|(dv, _)| dv.ident == v.ident)
            })
            .map(|v| {
                let d = discriminant(v);
                let on = v.cfg_attrs();
                let vi = &v.ident;
                match &v.fields {
                    Fields::Named(_) => quote! { #on #d => Ok(Self::#vi {}), },
                    _ => quote! { #on #d => Ok(Self::#vi()), },
                }
            })
            .collect::<Vec<_>>();

        // the tag run, the widest data variant per item, the headers of every data variant
        let max_tag = variants.iter().map(|v| v.discriminant).max().unwrap_or(0);
        let tag_item = self.sc(quote! { bound::tag_run_item_bits });
        let tag_fixed = self.sc(quote! { bound::tag_run_fixed_bits });
        let max = self.sc(quote! { bound::max });
        let add = self.sc(quote! { bound::add });
        let variant_items = data.iter().map(|(v, fields)| {
            let on = v.cfg_attrs();
            let sum = self.bound_sum(fields, quote! { MAX_ITEM_BITS }, quote! { Some(0) });
            quote! { #on { __widest = #max(__widest, #sum); } }
        });
        let variant_fixed = data.iter().map(|(v, fields)| {
            let on = v.cfg_attrs();
            let sum = self.bound_sum(fields, quote! { MAX_FIXED_BITS }, quote! { Some(0) });
            quote! { #on { __fixed = #add(__fixed, #sum); } }
        });
        let max_item = quote! {
            {
                let mut __widest: Option<usize> = Some(0);
                #(#variant_items)*
                #add(Some(#tag_item(#max_tag)), __widest)
            }
        };
        let max_fixed = quote! {
            {
                let mut __fixed: Option<usize> = Some(#tag_fixed(#max_tag));
                #(#variant_fixed)*
                __fixed
            }
        };
        let store = self.series_store_enum(variants, &data, future_version, max_tag);
        let cells = self.series_cells_enum(&data, future_version, max_tag);
        let store = quote! { #store #cells };

        let params = self.impl_params();
        let where_clause = self.generics.where_clause(&[]);
        let reader_generics = self.generics.ty(true);
        let type_params: Vec<_> = self.generics.type_params().map(|p| &p.ident).collect();
        let (reader_def, reader_ty, tags_of, finish_skim) = if data.is_empty() {
            (
                quote! {},
                quote! { #enum_tags<'i> },
                quote! { reader },
                quote! { Ok(__tags) },
            )
        } else {
            let vis_doc = format!("Column cursors of [{ident}] in a `Series`.");
            (
                quote! {
                    #cfg
                    #[doc = #vis_doc]
                    #[doc(hidden)]
                    #[allow(non_camel_case_types)]
                    pub struct #reader_ident<'i, #params> #where_clause {
                        __tags: #enum_tags<'i>,
                        #(#reader_fields)*
                        // a type parameter only in opaque fields (or none) is not in the readers
                        __params: ::core::marker::PhantomData<fn() -> (#(#type_params,)*)>,
                    }

                    // not derived: that would want the parameters to be `Clone`, the readers are
                    #cfg
                    impl<'i, #params> ::core::clone::Clone for #reader_ident #reader_generics #where_clause {
                        fn clone(&self) -> Self {
                            Self {
                                __tags: self.__tags.clone(),
                                #(#reader_clones)*
                                __params: ::core::marker::PhantomData,
                            }
                        }
                    }
                },
                quote! { #reader_ident #reader_generics },
                quote! { reader.__tags },
                quote! {
                    Ok(#reader_ident { __tags, #(#reader_inits)* __params: ::core::marker::PhantomData })
                },
            )
        };

        Ok(quote! {
            #reader_def

            #store

            #cfg
            impl<'i, #params> #trait_<'i> for #ident #lifetime #where_clause {
                type Reader = #reader_ty;
                #[allow(unused_mut)]
                const MAX_ITEM_BITS: Option<usize> = #max_item;
                #[allow(unused_mut)]
                const MAX_FIXED_BITS: Option<usize> = #max_fixed;

                fn write_group<'a, I, S, P>(
                    items: I,
                    sink: &mut S,
                    policy: &P,
                    level: #level,
                ) -> Result<(), #error>
                where
                    I: Iterator<Item = (&'a Self, bool)> + Clone,
                    S: #bit_sink,
                    P: #series_policy + ?Sized,
                    Self: 'a,
                {
                    let _ = (policy, level);
                    #future_check
                    // the variants seen while the tag run is written: a variant without items writes nothing, and
                    // is not walked over all items to find that out
                    #[allow(unused_mut)]
                    let mut __seen = 0u64;
                    #write_enum_tags(sink, items.clone().map(|(e, _)| {
                        let __d = e.discriminant() as u32;
                        __seen |= 1u64 << __d.min(63);
                        __d
                    }))?;
                    let _ = __seen;
                    #(#writes)*
                    Ok(())
                }

                #[allow(clippy::manual_range_patterns)]
                fn skim<C: #ctx_trait>(
                    rd: &mut #buf_reader<'i>,
                    ctx: C,
                    _level: #level,
                ) -> Result<Self::Reader, #error> {
                    let __tags = #enum_tags::skim(rd, &ctx, |tag| match tag {
                        #(#tag_arms)*
                        _ => Err(#error::EnumFutureVersionOrMalformedData),
                    })?;
                    #(#skims)*
                    #finish_skim
                }

                #[allow(unused_variables)]
                fn next(reader: &mut Self::Reader, restart: bool) -> Result<Self, #error> {
                    let (__tag, __entered) = #tags_of.next_tag()?;
                    match __tag {
                        #(#unit_arms)*
                        #(#empty_arms)*
                        #(#data_arms)*
                        _ => Err(#error::EnumFutureVersionOrMalformedData),
                    }
                }

                fn series_tag(&self) -> Option<u32> {
                    Some(self.discriminant() as u32)
                }

                fn tag_run(reader: &Self::Reader) -> Option<#tag_run<'i>> {
                    #tags_of.tag_run()
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::shrink_wrap::shrink_wrap_attr;
    use proc_macro2::TokenStream;
    use quote::quote;

    /// The compile error the derive gives, `None` when it generates code.
    fn error_of(args: TokenStream, item: TokenStream) -> Option<String> {
        let s = shrink_wrap_attr(args, item).to_string();
        s.contains("compile_error").then_some(s)
    }

    fn struct_error(field: TokenStream) -> Option<String> {
        error_of(
            quote! { borrowed, owned, series },
            quote! { struct S<'i> { a: u8, #field, z: &'i str } },
        )
    }

    #[test]
    fn v1_exclusions_name_the_field() {
        let cases = [
            (quote! { tree: RefBox<'i, u8> }, "`tree` of `S`: a RefBox"),
            (
                quote! { r: Result<u8, u16> },
                "`r` of `S`: Result is not in Series v1",
            ),
            (quote! { list: RefVec<'i, u16> }, "`list` of `S`: a list"),
            (
                quote! { deep: Option<[RefBox<'i, u8>; 2]> },
                "`deep` of `S`: a RefBox",
            ),
            (quote! { span: Range<u8> }, "`span` of `S`: a range"),
        ];
        for (field, message) in cases {
            let err = struct_error(field.clone()).unwrap_or_else(|| panic!("{field} compiled"));
            assert!(err.contains(message), "{err}");
            assert!(err.contains("#[series(opaque)]"), "{err}");
        }
        let err = struct_error(quote! { size: TailSize }).unwrap();
        assert!(err.contains("`size` of `S`: TailSize"), "{err}");
    }

    #[test]
    fn opaque_fields_and_byte_lists_compile() {
        for field in [
            quote! { #[series(opaque)] tree: RefBox<'i, u8> },
            quote! { #[series(opaque)] r: Result<u8, u16> },
            quote! { #[series(opaque)] list: RefVec<'i, u16> },
            quote! { bytes: RefVec<'i, u8> },
            quote! { pair: (u8, Option<[i16; 2]>) },
        ] {
            assert_eq!(struct_error(field.clone()), None, "{field}");
        }
    }

    #[test]
    fn exclusions_in_enum_variants() {
        let err = error_of(
            quote! { borrowed, owned, series, ww_repr = u4 },
            quote! { enum E<'i> { A, B { r: Result<u8, u8> }, C(&'i str) } },
        )
        .unwrap();
        assert!(err.contains("`r` of `E::B`: Result"), "{err}");
        let err = error_of(
            quote! { borrowed, owned, series, ww_repr = u4 },
            quote! { enum E { A(#[series(opaque)] Option<u8>) } },
        )
        .unwrap();
        assert!(err.contains("tuple variant"), "{err}");
    }

    #[test]
    fn directives_series_does_not_go_with() {
        let err = error_of(
            quote! { owned, series, sparse },
            quote! { struct SOwned { a: Option<u8> } },
        )
        .unwrap();
        assert!(err.contains("sparse structs are not in Series v1"), "{err}");
        let err = error_of(
            quote! { borrowed, owned, series },
            quote! { struct T(u8, u16); },
        )
        .unwrap();
        assert!(err.contains("`T` is a tuple struct"), "{err}");
        let err = error_of(
            quote! { borrowed, owned },
            quote! { struct S { #[series(opaque)] a: u8 } },
        )
        .unwrap();
        assert!(err.contains("needs the `series` directive"), "{err}");
        let err = error_of(
            quote! { borrowed, owned, series },
            quote! { struct S { #[series(opaq)] a: u8 } },
        )
        .unwrap();
        assert!(
            err.contains("field `a`: expected #[series(opaque)]"),
            "{err}"
        );
    }

    #[test]
    fn no_store() {
        let has_store = |args: TokenStream, item: TokenStream| {
            let s = shrink_wrap_attr(args, item).to_string();
            assert!(!s.contains("compile_error"), "{s}");
            s.contains("SeriesStore for")
        };
        assert!(has_store(
            quote! { borrowed, owned, series },
            quote! { struct S { a: u8 } }
        ));
        assert!(!has_store(
            quote! { borrowed, owned, series(no_store) },
            quote! { struct S { a: u8 } }
        ));
        // an opaque field has no store, so neither has the type
        assert!(!has_store(
            quote! { owned, series },
            quote! { struct SOwned { #[series(opaque)] a: Vec<String> } }
        ));
        let err = error_of(
            quote! { borrowed, series(no_stor) },
            quote! { struct S { a: u8 } },
        )
        .unwrap();
        assert!(
            err.contains("expected series, series(no_store) or series(cells)"),
            "{err}"
        );
    }

    #[test]
    fn type_parameters() {
        let compiles = [
            (
                quote! { borrowed, owned, series, final_structure },
                quote! { struct Timed<T: Copy> { t: u32, value: T } },
            ),
            (
                quote! { borrowed, owned, series, ww_repr = u4 },
                quote! { enum E<T> { A(T), B } },
            ),
            (
                quote! { borrowed, owned, display },
                quote! { /// msg
                struct S<T> { a: T } },
            ),
            (
                quote! { borrowed, owned, series },
                quote! { struct S<T, const N: usize> where T: Copy { a: [T; N] } },
            ),
        ];
        for (args, item) in compiles {
            assert_eq!(error_of(args, item), None);
        }
        for directive in [
            quote! { fixed_layout },
            quote! { head_tail },
            quote! { sparse },
        ] {
            let err = error_of(
                quote! { borrowed, #directive },
                quote! { struct Buf<const N: usize> { a: Option<[u8; N]> } },
            )
            .unwrap();
            assert!(
                err.contains("`Buf`") && err.contains(&format!("don't go with `{directive}`")),
                "{err}"
            );
        }
    }
}
