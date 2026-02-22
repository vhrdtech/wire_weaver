use crate::ast::item_enum::{Fields, Variant};
use crate::ast::object_size::ObjectSize;
use crate::ast::repr::Repr;
use crate::ast::{ItemEnum, Type};
use crate::codegen::ty::FieldPath;
use crate::codegen::util::{serdes_scaffold, strings_to_derive};
use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, quote};
use syn::{Lit, LitInt};

impl ItemEnum {
    pub fn def_rust(&self, no_alloc: bool) -> TokenStream {
        let enum_name = &self.ident;
        let variants = CGEnumFieldsDef {
            variants: &self.variants,
            no_alloc,
        };
        let lifetime = enum_lifetime(self, no_alloc);
        let derive = strings_to_derive(&self.derive);
        let docs = &self.docs;
        let cfg = &self.cfg;
        let cfg_attr_defmt = &self.defmt;
        let assert_size = if let Some(size) = &self.size_assumption {
            size.assert_element_size(&self.ident, &self.cfg)
        } else {
            quote! {}
        };
        // let base_ty = ww_discriminant_type(self);
        let native_repr = self.native_repr();
        let enum_discriminant = enum_discriminant(self, lifetime.clone());
        let ts = quote! {
            #cfg
            #docs
            #derive
            #cfg_attr_defmt
            #[repr(#native_repr)]
            // #[ww_repr(#base_ty)]
            pub enum #enum_name #lifetime { #variants }

            #assert_size

            #cfg
            #enum_discriminant
        };
        ts
    }

    pub fn serdes_rust(&self, no_alloc: bool, skip_owned: bool) -> TokenStream {
        let enum_name = &self.ident;
        let enum_ser = CGEnumSer {
            item_enum: self,
            no_alloc,
        };
        let enum_des = CGEnumDes {
            item_enum: self,
            no_alloc,
            owned: false,
        };
        let (lifetime, enum_des_owned) = if no_alloc && self.potential_lifetimes() {
            (quote!(<'i>), None)
        } else if skip_owned {
            (quote!(), None)
        } else {
            let enum_des_owned = CGEnumDes {
                item_enum: self,
                no_alloc,
                owned: true,
            };
            (quote!(), Some(enum_des_owned))
        };

        let mut unknown_unsized = vec![];
        let mut sum = self.size_assumption.unwrap_or(ObjectSize::Unsized); // enum is Unsized by default.
        // No need to check if it's already Unsized.
        if !matches!(sum, ObjectSize::Unsized) {
            if matches!(self.repr, Repr::UNib32) {
                sum = sum.add(ObjectSize::SelfDescribing);
            }
            // NOTE: make sure to not accidentally bump Unsized to UFS here if any of the fields is UFS.
            // See ElementSize docs and comments on sum method.
            for v in &self.variants {
                match &v.fields {
                    Fields::Named(named) => {
                        for f in named {
                            if let Some(size) = f.ty.element_size() {
                                sum = sum.add(size);
                            }
                            if let Type::External(path, _) = &f.ty
                                && let Some(ident) = path.segments.last()
                            {
                                unknown_unsized.push(ident.clone());
                            }
                        }
                    }
                    Fields::Unnamed(unnamed) => {
                        for ty in unnamed {
                            if let Some(size) = ty.element_size() {
                                sum = sum.add(size);
                            }
                            if let Type::External(path, _) = ty
                                && let Some(ident) = path.segments.last()
                            {
                                unknown_unsized.push(ident.clone());
                            }
                        }
                    }
                    Fields::Unit => {}
                }
            }
        }
        let implicitly_unsized = sum.is_unsized() && unknown_unsized.is_empty();
        let element_size = if implicitly_unsized {
            let r#unsized = ObjectSize::Unsized;
            quote! { #r#unsized }
        } else {
            sum.sum_recursively(unknown_unsized)
        };
        serdes_scaffold(
            enum_name,
            enum_ser,
            enum_des,
            enum_des_owned,
            lifetime,
            &self.cfg,
            element_size,
        )
    }
}

pub fn enum_lifetime(item_enum: &ItemEnum, no_alloc: bool) -> TokenStream {
    if no_alloc && item_enum.potential_lifetimes() {
        quote!(<'i>)
    } else {
        quote!()
    }
}

// fn ww_discriminant_type(item_enum: &ItemEnum) -> Ident {
//     let ty = format!("u{}", item_enum.repr.required_bits());
//     Ident::new(ty.as_str(), Span::call_site())
// }

pub fn enum_discriminant(item_enum: &ItemEnum, lifetime: TokenStream) -> TokenStream {
    let enum_name = &item_enum.ident;
    let native_repr = item_enum.native_repr();
    quote! {
        impl #lifetime #enum_name #lifetime {
            pub fn discriminant(&self) -> #native_repr {
                unsafe { *<*const _>::from(self).cast::<#native_repr>() }
            }
        }
    }
}

struct CGEnumFieldsDef<'a> {
    variants: &'a [Variant],
    no_alloc: bool,
}

impl ToTokens for CGEnumFieldsDef<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for variant in self.variants {
            let ident = &variant.ident;
            let discriminant = variant.discriminant_lit();
            let variant_docs = &variant.docs;
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
                        .map(|f| f.ty.def(self.no_alloc))
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
                        .map(|ty| ty.def(self.no_alloc))
                        .collect();
                    quote!(#variant_docs #ident ( #(#field_types),* ) = #discriminant,)
                }
                Fields::Unit => quote!(#variant_docs #ident = #discriminant,),
            };
            tokens.append_all(variant);
        }
    }
}

struct CGEnumSer<'a> {
    item_enum: &'a ItemEnum,
    no_alloc: bool,
}

struct CGEnumDes<'a> {
    item_enum: &'a ItemEnum,
    no_alloc: bool,
    owned: bool,
}

fn write_discriminant(repr: Repr, tokens: &mut TokenStream) {
    let (write_fn, bits) = match repr {
        Repr::U(1) => {
            tokens.append_all(quote! { wr.write_bool(self.discriminant() != 0)?; });
            return;
        }
        Repr::U(4) => ("write_u4", None),
        Repr::U(8) => ("write_u8", None),
        Repr::U(16) => ("write_u16", None),
        Repr::U(32) => ("write_u32", None),
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
        write_discriminant(self.item_enum.repr, tokens);

        let enum_name = &self.item_enum.ident;
        let mut ser_data_variants = quote! {};
        for variant in &self.item_enum.variants {
            match &variant.fields {
                Fields::Named(fields_named) => {
                    let mut fields_names = vec![];
                    let mut ser = quote!();
                    for field in fields_named {
                        let field_name = &field.ident;
                        let field_path = if matches!(field.ty, Type::IsSome(_) | Type::IsOk(_)) {
                            FieldPath::Ref(quote! {}) // empty path, because IsSome and IsOk already carry field name
                        } else {
                            fields_names.push(field_name.clone()); // do not create a match arm with a flag, because it's not a part of an enum
                            FieldPath::Ref(quote!(#field_name))
                        };
                        // tokens.append_all(trace_extended_key_val(
                        //     "Serialize named field",
                        //     field.ident.to_string().as_str(),
                        // ));
                        field
                            .ty
                            .buf_write(field_path, self.no_alloc, quote! { ? }, &mut ser);
                    }
                    let variant_name = &variant.ident;
                    ser_data_variants.append_all(
                        quote!(#enum_name::#variant_name { #(#fields_names),* } => { #ser }),
                    );
                }
                Fields::Unnamed(fields_unnamed) => {
                    let mut fields_numbers = vec![];
                    let mut ser = quote!();
                    for (idx, ty) in fields_unnamed.iter().enumerate() {
                        let field_name = Ident::new(format!("_{idx}").as_str(), Span::call_site());
                        let field_path = if matches!(ty, Type::IsSome(_) | Type::IsOk(_)) {
                            FieldPath::Ref(quote! {}) // empty path, because IsSome and IsOk already carry field name
                        } else {
                            fields_numbers.push(field_name.clone()); // do not create a match arm with a flag, because it's not a part of an enum
                            FieldPath::Ref(quote!(#field_name))
                        };
                        // tokens.append_all(trace_extended_key_val(
                        //     "Serialize unnamed field",
                        //     field_name.to_string().as_str(),
                        // ));
                        ty.buf_write(field_path, self.no_alloc, quote! { ? }, &mut ser);
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
            tokens.append_all(quote!(Ok(())));
        } else {
            tokens.append_all(quote! {
                match &self {
                    #ser_data_variants,
                    _ => {}
                }
                Ok(())
            });
        }
    }
}

fn read_discriminant(repr: Repr) -> TokenStream {
    let (write_fn, bits) = match repr {
        Repr::U(1) => return quote! { read_bool()? as u8 },
        Repr::U(4) => ("read_u4", None),
        Repr::U(8) => ("read_u8", None),
        Repr::U(16) => ("read_u16", None),
        Repr::U(32) => ("read_u32", None),
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
            no_alloc: self.no_alloc,
            owned: self.owned,
        };
        // tokens.append_all(trace_extended_key_val(
        //     "Deserialize enum",
        //     self.item_enum.ident.to_string().as_str(),
        // ));
        let read_discriminant = read_discriminant(self.item_enum.repr);
        tokens.append_all(quote! {
            let discriminant = rd.#read_discriminant;
            Ok(match discriminant {
                #known_variants
                _ => { return Err(ShrinkWrapError::EnumFutureVersionOrMalformedData); }
            })
        });
    }
}

struct CGEnumVariantsDes<'a> {
    item_enum: &'a ItemEnum,
    no_alloc: bool,
    owned: bool,
}

impl Variant {
    pub(crate) fn discriminant_lit(&self) -> syn::Lit {
        Lit::Int(LitInt::new(
            format!("{}", self.discriminant).as_str(),
            Span::call_site(),
        ))
    }

    pub fn is_unit(&self) -> bool {
        matches!(self.fields, Fields::Unit)
    }
}

impl ToTokens for CGEnumVariantsDes<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for variant in &self.item_enum.variants {
            let discriminant = variant.discriminant_lit();
            let enum_name = &self.item_enum.ident;
            let variant_name = &variant.ident;
            match &variant.fields {
                Fields::Named(fields_named) => {
                    let mut field_names = vec![];
                    let mut des_fields = TokenStream::new();
                    for field in fields_named {
                        let field_name = &field.ident;
                        if !matches!(field.ty, Type::IsSome(_) | Type::IsOk(_)) {
                            field_names.push(field_name.clone());
                        }
                        let handle_eob = field.handle_eob();
                        // let x = rd.read_()?; or let x = rd.read_().unwrap_or(default);
                        // tokens.append_all(trace_extended_key_val(
                        //     "Deserialize named field",
                        //     field.ident.to_string().as_str(),
                        // ));
                        field.ty.buf_read(
                            field_name,
                            self.no_alloc,
                            self.owned,
                            handle_eob,
                            &quote! { _ },
                            &mut des_fields,
                        );
                    }
                    tokens.append_all(quote!(#discriminant => { #des_fields #enum_name::#variant_name{ #(#field_names),* } }))
                }
                Fields::Unnamed(fields_unnamed) => {
                    let mut field_names = vec![];
                    let mut des_fields = TokenStream::new();
                    for (idx, ty) in fields_unnamed.iter().enumerate() {
                        let field_name = Ident::new(format!("_{idx}").as_str(), Span::call_site());
                        if !matches!(ty, Type::IsSome(_) | Type::IsOk(_)) {
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
                            self.no_alloc,
                            self.owned,
                            handle_eob,
                            &quote! { _ },
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
