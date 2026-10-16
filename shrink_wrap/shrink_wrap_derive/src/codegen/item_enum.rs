use crate::ast::docs::Docs;
use crate::ast::item_enum::{Fields, FutureVersion, Variant};
use crate::ast::object_size::ObjectSize;
use crate::ast::repr::Repr;
use crate::ast::ty::Type;
use crate::codegen::item_struct::write_fields;
use crate::codegen::ty::FieldPath;
use crate::codegen::util::{
    CratePath, assert_sized_before_tail_size, maybe_quote, rewrite_positional_placeholders,
    serdes_scaffold,
};
use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, format_ident, quote};
use std::collections::HashSet;
use syn::{Lit, LitInt, LitStr, Path};

#[derive(Clone, Debug)]
pub(crate) struct CGItemEnum<'i> {
    pub(crate) docs: &'i Docs,
    pub(crate) repr: Repr,
    pub(crate) ident: &'i Ident,
    pub(crate) variants: &'i [Variant],
    pub(crate) future_version: Option<&'i FutureVersion>,
    pub(crate) cfg: Option<&'i TokenStream>,
    pub(crate) cfg_attr: &'i [TokenStream],
    pub(crate) derive: &'i [Path],
    pub(crate) size_assumption: Option<ObjectSize>,
    /// Set for a plain, lifetime-less type generated only because a `borrowed`/`owned` directive
    /// was used to disambiguate it (see `TyKind::Ambiguous`). Such a type never gets a `<'i>`
    /// generic, even on its "borrowed" (`is_ref: true`) side.
    pub(crate) ambiguous: bool,
    pub(crate) crate_path: &'i CratePath,
}

impl<'i> CGItemEnum<'i> {
    pub(crate) fn native_repr(&self) -> Ident {
        match self.repr {
            Repr::U(bits) => {
                let ty = if bits <= 8 {
                    "u8"
                } else if bits <= 16 {
                    "u16"
                } else if bits <= 32 {
                    "u32"
                } else {
                    panic!("only up to u32 enum discriminants are currently supported");
                };
                Ident::new(ty, Span::call_site())
            }
            Repr::UNib32 | Repr::U32 => Ident::new("u32", Span::call_site()),
            Repr::U8 => Ident::new("u8", Span::call_site()),
            Repr::U16 => Ident::new("u16", Span::call_site()),
            Repr::Nibble => Ident::new("u8", Span::call_site()),
        }
    }

    pub(crate) fn def_rust(&self, is_ref: bool) -> TokenStream {
        let cfg = self.cfg.map(|cond| quote! { #[cfg(#cond)] });
        let docs = &self.docs;
        let derive = maybe_quote(!self.derive.is_empty(), || {
            let d = self.derive.iter();
            quote! { #[derive( #(#d),* )] }
        });
        let cfg_attr = self.cfg_attr.iter();
        let enum_name = &self.ident;
        let variants = CGEnumFieldsDef {
            variants: self.variants,
            is_ref,
            cp: self.crate_path,
        };
        let lifetime = maybe_quote(is_ref && !self.ambiguous, || quote! { <'i> });
        let assert_size = if let Some(size) = &self.size_assumption {
            let is_sized = matches!(size, ObjectSize::Sized { .. });
            let before = maybe_quote(is_sized, || self.assert_variants_same_size(is_ref));
            let hint = match (is_sized, self.repr) {
                (true, Repr::UNib32) => ": a unib32 discriminant is SelfDescribing",
                (true, _) => ": a variant has a field that is not Sized",
                _ => "",
            };
            size.assert_element_size(self.ident, self.cfg, is_ref, before, hint, self.crate_path)
        } else {
            quote! {}
        };
        let assert_tail_size = self.variants.iter().map(|v| {
            let types = v.fields.types();
            let assert = assert_sized_before_tail_size(
                &types,
                self.ident,
                self.cfg,
                is_ref,
                self.crate_path,
            );
            if v.cfg.is_empty() || assert.is_empty() {
                assert
            } else {
                let cfg = v.cfg_attrs();
                quote! { #cfg const _: () = { #assert }; }
            }
        });
        let native_repr = self.native_repr();
        // its in-memory tag is the largest one, the wire discriminant is in its field
        let future_version = self.future_version.map(|fv| {
            let docs = &fv.docs;
            let ident = &fv.ident;
            quote! { #docs #ident(u32) = #native_repr::MAX, }
        });

        let ts = quote! {
            #cfg
            #docs
            #derive
            #(#[cfg_attr(#cfg_attr)])*
            #[repr(#native_repr)]
            pub enum #enum_name #lifetime { #variants #future_version }

            #assert_size
            #(#assert_tail_size)*
        };
        ts
    }

    pub(crate) fn impl_discriminant(&self, is_ref: bool) -> TokenStream {
        let cfg = self.cfg.map(|cond| quote! { #[cfg(#cond)] });
        let enum_name = &self.ident;
        let native_repr = self.native_repr();
        let lifetime = maybe_quote(is_ref && !self.ambiguous, || quote! { <'i> });
        let tag = quote! { unsafe { *<*const _>::from(self).cast::<#native_repr>() } };
        let body = match self.future_version {
            Some(fv) => {
                let ident = &fv.ident;
                quote! {
                    match self {
                        Self::#ident(discriminant) => *discriminant as #native_repr,
                        _ => #tag,
                    }
                }
            }
            None => tag,
        };
        quote! {
            #cfg
            impl #lifetime #enum_name #lifetime {
                /// Discriminant on the wire.
                pub fn discriminant(&self) -> #native_repr {
                    #body
                }
            }
        }
    }

    /// Size of each variant's fields as an `ElementSize` const expression, discriminant not included.
    fn variant_sizes(&self, is_ref: bool) -> Vec<TokenStream> {
        self.variants
            .iter()
            .map(|v| ObjectSize::sized(0).sum_fields(v.fields.types(), is_ref, self.crate_path))
            .collect()
    }

    /// `ELEMENT_SIZE` const expression: only one variant is on the wire, so the variants are folded with `one_of`
    /// (same size or SelfDescribing) instead of being summed, then the discriminant is added.
    fn element_size(&self, is_ref: bool) -> TokenStream {
        let size = match &self.size_assumption {
            // enum is Unsized by default, no need to look at the variants
            None | Some(ObjectSize::Unsized) => return ObjectSize::Unsized.tokens(self.crate_path),
            // nothing added to it changes it
            Some(ObjectSize::UnsizedFinalStructure) => {
                return ObjectSize::UnsizedFinalStructure.tokens(self.crate_path);
            }
            Some(size) => size,
        };
        let discriminant = match self.repr {
            Repr::U(bits) => ObjectSize::sized(bits as usize),
            Repr::Nibble => ObjectSize::sized(4),
            Repr::U8 => ObjectSize::sized(8),
            Repr::U16 => ObjectSize::sized(16),
            Repr::U32 => ObjectSize::sized(32),
            Repr::UNib32 => ObjectSize::SelfDescribing,
        };
        // `self_describing` stays SelfDescribing also when every variant happens to be of one size
        let start = size.add(discriminant).tokens(self.crate_path);
        // identical expressions are the same size: keeps the expression short for unit variants
        let mut seen = HashSet::new();
        let sizes = self.variant_sizes(is_ref);
        let mut always = vec![];
        let mut conditional = vec![];
        for (v, size) in self.variants.iter().zip(sizes) {
            if !v.cfg.is_empty() {
                conditional.push((v.cfg_attrs(), size));
            } else if seen.insert(size.to_string()) {
                always.push(size);
            }
        }
        if conditional.is_empty() {
            let mut variants = always.into_iter();
            return match variants.next() {
                Some(first) => quote! { #start.add(#first #(.one_of(#variants))*) },
                None => start,
            };
        }
        // a variant behind #[cfg] is folded in only where it exists: its field types might not exist elsewhere
        let element_size = self.crate_path.item(quote! { ElementSize });
        let (cfg, conditional): (Vec<_>, Vec<_>) = conditional.into_iter().unzip();
        quote! {{
            let payload: Option<#element_size> = None;
            #(
                let payload = Some(match payload {
                    Some(payload) => payload.one_of(#always),
                    None => #always,
                });
            )*
            #(
                #cfg
                let payload = Some(match payload {
                    Some(payload) => payload.one_of(#conditional),
                    None => #conditional,
                });
            )*
            match payload {
                Some(payload) => #start.add(payload),
                None => #start,
            }
        }}
    }

    /// `MAX_SIZE` const expression: the discriminant, then the largest variant, a variant behind `#[cfg]` only
    /// where it exists.
    fn max_size(&self) -> TokenStream {
        let cp = self.crate_path;
        let max_size = cp.item(quote! { MaxSize });
        let discriminant = match self.repr {
            Repr::U(bits) => quote! { bits(#bits as usize) },
            Repr::Nibble => quote! { nibbles(1) },
            Repr::U8 => quote! { bytes(1) },
            Repr::U16 => quote! { bytes(2) },
            Repr::U32 => quote! { bytes(4) },
            Repr::UNib32 => {
                let max = self
                    .variants
                    .iter()
                    .map(|v| v.discriminant)
                    .max()
                    .unwrap_or(0);
                let nibbles = shrink_wrap_len_nibbles(max);
                quote! { nibbles(#nibbles) }
            }
        };
        let variants = self.variants.iter().map(|v| {
            let cfg = v.cfg_attrs();
            let size = Type::seq_max_size(v.fields.types().into_iter(), cp);
            quote! {
                #cfg
                let payload = match payload {
                    Some(payload) => Some(#max_size::one_of(payload, #size)),
                    None => Some(#size),
                };
            }
        });
        quote! {{
            let payload: Option<Option<#max_size>> = None;
            #(#variants)*
            match payload {
                Some(payload) => #max_size::seq(Some(#max_size::#discriminant), payload),
                None => Some(#max_size::#discriminant),
            }
        }}
    }

    /// `PUSHES_LEN` const expression: any field of any variant, a variant behind `#[cfg]` only where it exists.
    fn pushes_len(&self) -> Option<TokenStream> {
        let cp = self.crate_path;
        let always = self.variants.iter().filter(|v| v.cfg.is_empty());
        let always = Type::any_pushes_len(always.flat_map(|v| v.fields.types()), cp);
        let conditional: Vec<_> = self
            .variants
            .iter()
            .filter(|v| !v.cfg.is_empty())
            .filter_map(|v| {
                let expr = Type::any_pushes_len(v.fields.types().into_iter(), cp)?;
                Some((v.cfg_attrs(), expr))
            })
            .collect();
        if conditional.is_empty() {
            return always;
        }
        let always = always.unwrap_or_else(|| quote! { false });
        let (cfg, conditional): (Vec<_>, Vec<_>) = conditional.into_iter().unzip();
        Some(quote! {{
            let pushes_len = #always;
            #(
                #cfg
                let pushes_len = pushes_len || #conditional;
            )*
            pushes_len
        }})
    }

    /// Part of the size assert of a `sized` enum: variants of different sizes make it SelfDescribing, say which.
    fn assert_variants_same_size(&self, is_ref: bool) -> TokenStream {
        let element_size = self.crate_path.item(quote! { ElementSize });
        let variants = self.variant_sizes(is_ref);
        let variants_cfg = self.variants.iter().map(|v| v.cfg_attrs());
        let sizes = self
            .variants
            .iter()
            .map(|v| {
                let mut known = ObjectSize::sized(0);
                let mut user_types = String::new();
                for ty in v.fields.types() {
                    match ty.element_size() {
                        Some(size) => known = known.add(size),
                        None => {
                            let ty = ty.def(is_ref, self.crate_path).to_string();
                            user_types += &format!(" + {}", ty.replace(' ', ""));
                        }
                    }
                }
                format!("{}: {}{user_types}", v.ident, known.describe())
            })
            .collect::<Vec<_>>()
            .join(", ");
        let err_msg = format!(
            "sized enum {}: variants differ in size ({sizes}); use self_describing instead of sized, \
            or pad the variants to one size",
            self.ident
        );
        quote! {
            assert!(!#element_size::variants_differ(&[#(#variants_cfg #variants),*]), #err_msg);
        }
    }

    pub(crate) fn serdes_rust(&self, is_ref: bool) -> TokenStream {
        let enum_name = &self.ident;
        let enum_ser = CGEnumSer {
            item_enum: self,
            is_ref,
        };
        let enum_des = CGEnumDes {
            item_enum: self,
            is_ref,
        };

        let element_size = self.element_size(is_ref);
        serdes_scaffold(
            enum_name,
            enum_ser,
            enum_des,
            self.cfg,
            element_size,
            // the discriminant never pushes a length, any variant's field can
            self.pushes_len(),
            self.max_size(),
            is_ref,
            is_ref && !self.ambiguous,
            self.crate_path,
        )
    }

    /// DER-12: `core::fmt::Display` (from each variant's `display` message) and `core::error::Error`.
    pub(crate) fn display_and_error_rust(&self, is_ref: bool) -> TokenStream {
        let cfg = self.cfg.map(|cond| quote! { #[cfg(#cond)] });
        let enum_name = &self.ident;
        let lifetime = maybe_quote(is_ref && !self.ambiguous, || quote! { <'i> });
        let arms = CGEnumDisplayArms { item_enum: self };
        quote! {
            #cfg
            impl #lifetime ::core::fmt::Display for #enum_name #lifetime {
                #[allow(unused_variables)]
                fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    match self {
                        #arms
                    }
                }
            }

            #cfg
            impl #lifetime ::core::error::Error for #enum_name #lifetime {}
        }
    }
}

/// DER-12: one `write!` match arm per variant, from its `display` message (checked non-empty before codegen),
/// plus the `#[future_version]` variant if any.
struct CGEnumDisplayArms<'i> {
    item_enum: &'i CGItemEnum<'i>,
}

impl ToTokens for CGEnumDisplayArms<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let enum_name = &self.item_enum.ident;
        for variant in self.item_enum.variants {
            let variant_name = &variant.ident;
            let message = variant
                .display_message()
                .expect("checked before codegen: every variant has a doc line or #[display(..)]");
            let lit = LitStr::new(
                &rewrite_positional_placeholders(&message),
                Span::call_site(),
            );
            tokens.append_all(variant.cfg_attrs());
            match &variant.fields {
                Fields::Named(named) => {
                    let names: Vec<&Ident> = named
                        .iter()
                        .filter(|f| !matches!(f.ty, Type::IsOk(_) | Type::IsSome(_)))
                        .map(|f| &f.ident)
                        .collect();
                    tokens.append_all(quote! {
                        #enum_name::#variant_name { #(#names),* } => write!(f, #lit),
                    });
                }
                Fields::Unnamed(unnamed) => {
                    let names: Vec<Ident> = unnamed
                        .iter()
                        .filter(|ty| !matches!(ty, Type::IsOk(_) | Type::IsSome(_)))
                        .enumerate()
                        .map(|(i, _)| format_ident!("_{i}"))
                        .collect();
                    tokens.append_all(quote! {
                        #enum_name::#variant_name( #(#names),* ) => write!(f, #lit),
                    });
                }
                Fields::Unit => {
                    tokens.append_all(quote! {
                        #enum_name::#variant_name => write!(f, #lit),
                    });
                }
            }
        }
        if let Some(fv) = self.item_enum.future_version {
            let ident = &fv.ident;
            let message = fv.docs.message_line().unwrap_or_default();
            let lit = LitStr::new(&message, Span::call_site());
            tokens.append_all(quote! {
                #enum_name::#ident(_) => write!(f, #lit),
            });
        }
    }
}

struct CGEnumFieldsDef<'a> {
    variants: &'a [Variant],
    is_ref: bool,
    cp: &'a CratePath,
}

impl ToTokens for CGEnumFieldsDef<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for variant in self.variants {
            let ident = &variant.ident;
            let discriminant = variant.discriminant_lit();
            let variant_docs = &variant.docs;
            tokens.append_all(variant.cfg_attrs());
            let variant = match &variant.fields {
                Fields::Named(fields_named) => {
                    let fields_named = fields_named
                        .iter()
                        .filter(|f| !matches!(f.ty, Type::IsOk(_) | Type::IsSome(_)))
                        .collect::<Vec<_>>();
                    let fields_docs = fields_named.iter().map(|f| &f.docs).collect::<Vec<_>>();
                    let field_names: Vec<Ident> =
                        fields_named.iter().map(|f| f.ident.clone()).collect();
                    let field_types: Vec<TokenStream> = fields_named
                        .iter()
                        .map(|f| f.ty.def(self.is_ref, self.cp))
                        .collect();
                    quote!(#variant_docs #ident { #(#fields_docs #field_names: #field_types),* } = #discriminant,)
                }
                Fields::Unnamed(fields_unnamed) => {
                    let fields_unnamed = fields_unnamed
                        .iter()
                        .filter(|ty| !matches!(ty, Type::IsOk(_) | Type::IsSome(_)))
                        .collect::<Vec<_>>();
                    let field_types: Vec<TokenStream> = fields_unnamed
                        .iter()
                        .map(|ty| ty.def(self.is_ref, self.cp))
                        .collect();
                    quote!(#variant_docs #ident ( #(#field_types),* ) = #discriminant,)
                }
                Fields::Unit => quote!(#variant_docs #ident = #discriminant,),
            };
            tokens.append_all(variant);
        }
    }
}

struct CGEnumSer<'i> {
    item_enum: &'i CGItemEnum<'i>,
    is_ref: bool,
}

struct CGEnumDes<'i> {
    item_enum: &'i CGItemEnum<'i>,
    is_ref: bool,
}

fn write_discriminant(repr: Repr, tokens: &mut TokenStream) {
    let (write_fn, bits) = match repr {
        Repr::U(1) => {
            tokens.append_all(quote! { wr.write_bool(self.discriminant() != 0)?; });
            return;
        }
        Repr::U8 | Repr::U16 | Repr::U32 => {
            let write_fn = match repr {
                Repr::U8 => quote!(write_u8_latch),
                Repr::U16 => quote!(write_u16_latch),
                _ => quote!(write_u32_latch),
            };
            tokens.append_all(quote! { wr.#write_fn(self.discriminant()); });
            return;
        }
        Repr::Nibble => ("write_nib_masked", None),
        Repr::UNib32 => ("write_unib32", None),
        Repr::U(bits) if bits < 8 => ("write_un8", Some(bits)),
        Repr::U(bits) if bits < 16 => ("write_un16", Some(bits)),
        Repr::U(bits) if bits < 32 => ("write_un32", Some(bits)),
        u => unimplemented!("discriminant_type {:?}", u),
    };
    let write_fn = Ident::new(write_fn, Span::call_site());
    if let Some(bits) = bits {
        let bit_count = Lit::Int(LitInt::new(format!("{bits}").as_str(), Span::call_site()));
        tokens.append_all(quote! { wr.#write_fn(#bit_count, self.discriminant())?; });
    } else {
        tokens.append_all(quote! { wr.#write_fn(self.discriminant())?; });
    }
}

impl ToTokens for CGEnumSer<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        // TODO: forbid empty enums or not?
        // tokens.append_all(trace_extended_key_val(
        //     "Serialize enum",
        //     self.item_enum.ident.to_string().as_str(),
        // ));
        if let Some(fv) = self.item_enum.future_version {
            // a known one would be read back as that variant, without its fields
            let ident = &fv.ident;
            let max = self.item_enum.repr.max_discriminant();
            let known = self.item_enum.variants.iter().map(|v| v.discriminant_lit());
            let error = self.item_enum.crate_path.error();
            tokens.append_all(quote! {
                if let Self::#ident(discriminant) = self {
                    if *discriminant > #max #(|| *discriminant == #known)* {
                        return Err(#error::SubtypeOutOfRange);
                    }
                }
            });
        }
        write_discriminant(self.item_enum.repr, tokens);

        let enum_name = &self.item_enum.ident;
        let mut ser_data_variants = quote! {};
        for variant in self.item_enum.variants {
            if !matches!(variant.fields, Fields::Unit) {
                ser_data_variants.append_all(variant.cfg_attrs());
            }
            match &variant.fields {
                Fields::Named(fields_named) => {
                    let mut fields_names = vec![];
                    let mut ser = quote!();
                    write_fields(
                        fields_named,
                        |f| {
                            let ident = &f.ident;
                            quote! { (*#ident) }
                        },
                        |field, ser| {
                            let field_name = &field.ident;
                            let field_path = if matches!(field.ty, Type::IsSome(_) | Type::IsOk(_))
                            {
                                FieldPath::Ref(quote! {}) // empty path, because IsSome and IsOk already carry field name
                            } else if matches!(field.ty, Type::TailSize(_)) {
                                fields_names.push(quote!(#field_name: _)); // its value is not written, don't bind it
                                FieldPath::Ref(quote! {})
                            } else {
                                fields_names.push(quote!(#field_name)); // do not create a match arm with a flag, because it's not a part of an enum
                                FieldPath::Ref(quote!(#field_name))
                            };
                            field
                                .ty
                                .buf_write(field_path, self.is_ref, quote! { ? }, ser);
                        },
                        &mut ser,
                    );
                    if fields_named
                        .iter()
                        .any(|f| matches!(f.ty, Type::TailSize(_)))
                    {
                        Type::backfill_tail_size(&mut ser);
                    }
                    let variant_name = &variant.ident;
                    ser_data_variants.append_all(
                        quote!(#enum_name::#variant_name { #(#fields_names),* } => { #ser }),
                    );
                }
                Fields::Unnamed(fields_unnamed) => {
                    let mut fields_numbers = vec![];
                    let mut ser = quote!();
                    let mut idx_skip_flags = 0;
                    for ty in fields_unnamed {
                        let field_name =
                            Ident::new(format!("_{idx_skip_flags}").as_str(), Span::call_site());
                        let field_path = if matches!(ty, Type::IsSome(_) | Type::IsOk(_)) {
                            FieldPath::Ref(quote! {}) // empty path, because IsSome and IsOk already carry field name
                        } else {
                            idx_skip_flags += 1;
                            fields_numbers.push(field_name.clone()); // do not create a match arm with a flag, because it's not a part of an enum
                            FieldPath::Ref(quote!(#field_name))
                        };
                        // tokens.append_all(trace_extended_key_val(
                        //     "Serialize unnamed field",
                        //     field_name.to_string().as_str(),
                        // ));
                        ty.buf_write(field_path, self.is_ref, quote! { ? }, &mut ser);
                    }
                    if fields_unnamed
                        .iter()
                        .any(|ty| matches!(ty, Type::TailSize(_)))
                    {
                        Type::backfill_tail_size(&mut ser);
                    }
                    let variant_name = &variant.ident;
                    ser_data_variants.append_all(
                        quote!(#enum_name::#variant_name ( #(#fields_numbers),* ) => { #ser }),
                    );
                }
                Fields::Unit => continue,
            }
        }

        if ser_data_variants.is_empty() {
            tokens.append_all(quote!(wr.latched()));
        } else {
            tokens.append_all(quote! {
                match &self {
                    #ser_data_variants,
                    _ => {}
                }
                wr.latched()
            });
        }
    }
}

fn read_discriminant(repr: Repr) -> TokenStream {
    let (write_fn, bits) = match repr {
        Repr::U(1) => return quote! { read_bool()? as u8 },
        // a failed read gives 0 and is reported by `rd.latched()?` after the variant's fields
        Repr::U8 => return quote! { read_u8_latch() },
        Repr::U16 => return quote! { read_u16_latch() },
        Repr::U32 => return quote! { read_u32_latch() },
        Repr::Nibble => ("read_nib_value", None),
        Repr::UNib32 => ("read_unib32", None),
        Repr::U(bits) if bits < 8 => ("read_un8", Some(bits)),
        Repr::U(bits) if bits < 16 => ("read_un16", Some(bits)),
        Repr::U(bits) if bits < 32 => ("read_un32", Some(bits)),
        u => unimplemented!("discriminant_type {:?}", u),
    };
    let read_fn = Ident::new(write_fn, Span::call_site());
    if let Some(bits) = bits {
        let bit_count = Lit::Int(LitInt::new(format!("{bits}").as_str(), Span::call_site()));
        quote! { #read_fn(#bit_count)? }
    } else {
        quote! { #read_fn()? }
    }
}

impl ToTokens for CGEnumDes<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let known_variants = CGEnumVariantsDes {
            item_enum: self.item_enum,
            is_ref: self.is_ref,
        };
        // tokens.append_all(trace_extended_key_val(
        //     "Deserialize enum",
        //     self.item_enum.ident.to_string().as_str(),
        // ));
        let read_discriminant = read_discriminant(self.item_enum.repr);
        let error = self.item_enum.crate_path.error();
        // A latched discriminant that failed to read is 0, so is one read after an earlier latched read failed:
        // without a variant 0 that is this arm, where the kept error comes first. Any other discriminant read
        // returns the kept error itself.
        let latched_discriminant = matches!(self.item_enum.repr, Repr::U8 | Repr::U16 | Repr::U32);
        let has_zero = self.item_enum.variants.iter().any(|v| v.discriminant == 0);
        let check_kept = maybe_quote(
            latched_discriminant && !has_zero,
            || quote! { rd.latched()?; },
        );
        let unknown = match self.item_enum.future_version {
            // An Unsized enum is bounded by its size slot, the newer variant's fields are skipped with it. Otherwise
            // nothing more is read: right for a new unit variant.
            Some(fv) => {
                let ident = &fv.ident;
                let enum_name = &self.item_enum.ident;
                quote! {
                    #check_kept
                    #enum_name::#ident(discriminant as u32)
                }
            }
            None => quote! {
                #check_kept
                return Err(#error::EnumFutureVersionOrMalformedData);
            },
        };
        tokens.append_all(quote! {
            let discriminant = rd.#read_discriminant;
            let value = match discriminant {
                #known_variants
                _ => { #unknown }
            };
            rd.latched()?;
            Ok(value)
        });
    }
}

struct CGEnumVariantsDes<'i> {
    item_enum: &'i CGItemEnum<'i>,
    is_ref: bool,
}

impl Variant {
    pub(crate) fn discriminant_lit(&self) -> syn::Lit {
        Lit::Int(LitInt::new(
            format!("{}", self.discriminant).as_str(),
            Span::call_site(),
        ))
    }
}

impl ToTokens for CGEnumVariantsDes<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for variant in self.item_enum.variants {
            let discriminant = variant.discriminant_lit();
            let enum_name = &self.item_enum.ident;
            let variant_name = &variant.ident;
            if let Some(not_cfg) = variant.not_cfg_attr() {
                // Compiled out here, but a known variant: say so instead of "future version or malformed".
                // A latched discriminant that failed to read is 0, the kept error comes first then.
                let error = self.item_enum.crate_path.error();
                let check_kept = maybe_quote(
                    matches!(self.item_enum.repr, Repr::U8 | Repr::U16 | Repr::U32),
                    || quote! { rd.latched()?; },
                );
                tokens.append_all(quote! {
                    #not_cfg
                    #discriminant => {
                        #check_kept
                        return Err(#error::EnumVariantDisabled);
                    }
                });
                tokens.append_all(variant.cfg_attrs());
            }
            match &variant.fields {
                Fields::Named(fields_named) => {
                    let mut field_names = vec![];
                    let mut des_fields = TokenStream::new();
                    for field in fields_named {
                        let field_name = &field.ident;
                        if !matches!(field.ty, Type::IsSome(_) | Type::IsOk(_)) {
                            field_names.push(field_name.clone());
                        }
                        field.buf_read(!self.is_ref, self.item_enum.crate_path, &mut des_fields);
                    }
                    tokens.append_all(quote!(#discriminant => { #des_fields #enum_name::#variant_name{ #(#field_names),* } }))
                }
                Fields::Unnamed(fields_unnamed) => {
                    let mut field_names = vec![];
                    let mut des_fields = TokenStream::new();
                    let mut idx_skip_flags = 0;
                    for ty in fields_unnamed {
                        let field_name =
                            Ident::new(format!("_{idx_skip_flags}").as_str(), Span::call_site());
                        if !matches!(ty, Type::IsSome(_) | Type::IsOk(_)) {
                            idx_skip_flags += 1;
                            field_names.push(field_name.clone());
                        }
                        let handle_eob = quote! { ? };
                        // let x = rd.read_()?; or let x = rd.read_().unwrap_or(default);
                        // tokens.append_all(trace_extended_key_val(
                        //     "Deserialize unnamed field",
                        //     field_name.to_string().as_str(),
                        // ));
                        ty.buf_read(
                            &field_name,
                            !self.is_ref,
                            handle_eob,
                            true,
                            self.item_enum.crate_path,
                            &mut des_fields,
                        );
                    }
                    tokens.append_all(quote!(#discriminant => { #des_fields #enum_name::#variant_name( #(#field_names),* ) }))
                }
                Fields::Unit => {
                    tokens.append_all(quote!(#discriminant => #enum_name::#variant_name,));
                }
            }
        }
    }
}

/// Nibbles of `UNib32(value)`, as in shrink_wrap.
fn shrink_wrap_len_nibbles(value: u32) -> usize {
    let mut value = value >> 3;
    let mut nibbles = 1;
    while value != 0 {
        value >>= 3;
        nibbles += 1;
    }
    nibbles
}
