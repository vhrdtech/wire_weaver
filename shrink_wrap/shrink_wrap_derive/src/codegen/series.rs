//! `#[derive_shrink_wrap(.., series)]` (SW-60): the `SeriesColumns` impl of a struct or enum, made of the pieces
//! in `shrink_wrap::series` the way the hand-written impls of its tests are. A struct's group is its fields' groups
//! in declaration order (behind a field count or group length when it is evolvable), each field restarting where an
//! enum-typed field before it changed; an enum's group is its tag run, then each data variant's fields as such a
//! group over the items in the variant, in discriminant order.

use crate::ast::item_enum::{Fields, Variant};
use crate::ast::item_struct::Field;
use crate::ast::ty::Type;
use crate::codegen::util::{CratePath, impl_params, maybe_quote, ty_generics};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

/// What both impls need to know about the type.
pub(crate) struct SeriesSeed<'a> {
    pub(crate) ident: &'a Ident,
    /// A struct's type parameters (DER-14): each needs `SeriesColumns<'i>`.
    pub(crate) type_params: &'a [syn::TypeParam],
    /// The type has a `<'i>` of its own.
    pub(crate) has_lifetime: bool,
    /// Borrowed side: opaque fields go through `SerializeShrinkWrap` / `DeserializeShrinkWrap<'i>`, else the owned
    /// traits.
    pub(crate) is_ref: bool,
    pub(crate) cfg: Option<&'a TokenStream>,
    pub(crate) cp: &'a CratePath,
}

/// A field as a `Series` sees it: flags left out, they are part of their `Option`.
struct SeriesField<'a> {
    /// `self.#member` (or the binding in a variant pattern).
    member: TokenStream,
    ty: TokenStream,
    opaque: bool,
    /// Can have a `series_tag`: a user type or an `Option`, which an earlier-enum restart looks at.
    tagged: bool,
    default: Option<&'a syn::Expr>,
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
        ty_generics(self.has_lifetime, self.type_params)
    }

    /// `T: SeriesColumns<'i>, ..` of the impl.
    fn impl_params(&self) -> TokenStream {
        let trait_ = self.sc(quote! { SeriesColumns });
        impl_params(self.type_params, quote! { #trait_<'i> })
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
                quote! { <#ty as #trait_<'i>>::series_tag(#v) }
            } else {
                quote! { None }
            }
        });
        quote! { [#(#tags),*] }
    }

    /// Writes field `j`'s group: `items` the parent's items, `pick` the closure from an item to `Option<&field>`,
    /// `tags` the closure from an item to the tags of its fields (when an enum field comes before `j`).
    fn write_field(
        &self,
        f: &SeriesField,
        j: usize,
        earlier_tagged: bool,
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
        let earlier = if earlier_tagged {
            let changed_before = self.sc(quote! { changed_before });
            quote! { move |p: &'a Self, c: &'a Self| #changed_before(&__tags(p), &__tags(c), #j) }
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
        let codec_policy = self.sc(quote! { CodecPolicy });
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
            let earlier_tagged = fields[..j].iter().any(|f| f.tagged);
            self.write_field(f, j, earlier_tagged, quote! { |s: &'a Self| Some(&s.#m) })
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

        let params = self.impl_params();
        Ok(quote! {
            #cfg
            impl<'i, #params> #trait_<'i> for #ident #lifetime {
                type Reader = (#sibling_tags<#n>, (#(Option<#readers>,)*));

                fn write_group<'a, I, S>(
                    items: I,
                    sink: &mut S,
                    policy: &#codec_policy,
                    level: #level,
                ) -> Result<(), #error>
                where
                    I: Iterator<Item = (&'a Self, bool)> + Clone,
                    S: #bit_sink,
                    Self: 'a,
                {
                    if items.clone().next().is_none() {
                        return Ok(());
                    }
                    #write_struct_head(sink, level, #evolvable, #n_u32, |counter| {
                        <Self as #trait_<'i>>::write_group(items.clone(), counter, policy, #level::Fields)
                    })?;
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
        let codec_policy = self.sc(quote! { CodecPolicy });
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
                let earlier_tagged = fields[..j].iter().any(|f| f.tagged);
                let pick = quote! {
                    #[allow(unreachable_patterns, unused_variables)]
                    |e: &'a Self| match e {
                        #pat => Some(#b),
                        _ => None,
                    }
                };
                self.write_field(f, j, earlier_tagged, pick)
            });
            writes.push(quote! {
                #on
                {
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
                    #[derive(Clone)]
                    #[allow(non_camel_case_types)]
                    pub struct #reader_ident<'i> {
                        __tags: #enum_tags<'i>,
                        #(#reader_fields)*
                    }
                },
                quote! { #reader_ident<'i> },
                quote! { reader.__tags },
                quote! { Ok(#reader_ident { __tags, #(#reader_inits)* }) },
            )
        };

        Ok(quote! {
            #reader_def

            #cfg
            impl<'i> #trait_<'i> for #ident #lifetime {
                type Reader = #reader_ty;

                fn write_group<'a, I, S>(
                    items: I,
                    sink: &mut S,
                    policy: &#codec_policy,
                    level: #level,
                ) -> Result<(), #error>
                where
                    I: Iterator<Item = (&'a Self, bool)> + Clone,
                    S: #bit_sink,
                    Self: 'a,
                {
                    let _ = (policy, level);
                    #future_check
                    #write_enum_tags(sink, items.clone().map(|(e, _)| e.discriminant() as u32))?;
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
            (
                quote! { span: Range<u8> },
                "`span` of `S`: a range",
            ),
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
    fn type_parameters() {
        assert_eq!(
            error_of(
                quote! { borrowed, owned, series, final_structure },
                quote! { struct Timed<T: Copy> { t: u32, value: T } },
            ),
            None
        );
        let err = error_of(
            quote! { borrowed, owned, ww_repr = u4 },
            quote! { enum E<T> { A(T) } },
        )
        .unwrap();
        assert!(err.contains("only supported on structs"), "{err}");
        let err = error_of(
            quote! { borrowed, owned, display },
            quote! { /// msg
            struct S<T> { a: T } },
        )
        .unwrap();
        assert!(err.contains("don't go with `display`"), "{err}");
        let err = error_of(
            quote! { borrowed, owned },
            quote! { struct S<T> where T: Copy { a: T } },
        )
        .unwrap();
        assert!(err.contains("where clause"), "{err}");
    }
}
