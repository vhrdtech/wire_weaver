use crate::ast::docs::Docs;
use crate::ast::generics::Generics;
use crate::ast::item_struct::{Field, StructKind};
use crate::ast::object_size::ObjectSize;
use crate::ast::ty::Type;
use crate::codegen::ty::FieldPath;
use crate::codegen::util::{
    CratePath, SerdeDefaults, assert_sized_before_tail_size, maybe_attrs, maybe_quote,
    rewrite_positional_placeholders, serdes_scaffold,
};
use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, quote};
use syn::{LitStr, Path};

#[derive(Clone, Debug)]
pub(crate) struct CGItemStruct<'i> {
    pub(crate) docs: &'i Docs,
    pub(crate) ident: &'i Ident,
    pub(crate) fields: &'i [Field],
    /// Named, tuple or unit struct (SW-59)
    pub(crate) kind: StructKind,
    /// `sparse` (DER-13): the slot of each field
    pub(crate) sparse_slots: Option<&'i [u32]>,
    /// `transparent` (DER-18): the index of the one field that isn't zero bits, which the struct is on the wire
    pub(crate) transparent: Option<usize>,
    /// `validate = path`: called on the value just read
    pub(crate) validate: Option<&'i Path>,
    pub(crate) cfg: Option<&'i TokenStream>,
    pub(crate) cfg_attr: &'i [TokenStream],
    pub(crate) derive: &'i [Path],
    pub(crate) size_assumption: Option<ObjectSize>,
    /// Set for a plain, lifetime-less type generated only because a `borrowed`/`owned` directive
    /// was used to disambiguate it (see `TyKind::Ambiguous`). Such a type never gets a `<'i>`
    /// generic, even on its "borrowed" (`is_ref: true`) side.
    pub(crate) ambiguous: bool,
    pub(crate) crate_path: &'i CratePath,
    /// Type and const parameters and where clause of the struct (DER-14), as written.
    pub(crate) generics: &'i Generics,
    /// The type with the item's own name: its fields keep the attributes the derive doesn't read.
    pub(crate) base: bool,
    /// `#[repr(..)]` of the struct, as written.
    pub(crate) repr: &'i [syn::Attribute],
}

impl<'i> CGItemStruct<'i> {
    pub(crate) fn def_rust(&self, is_ref: bool) -> TokenStream {
        let cfg = self.cfg.map(|cond| quote! { #[cfg(#cond)] });
        let docs = &self.docs;
        let derive = maybe_quote(!self.derive.is_empty(), || {
            let d = self.derive.iter();
            quote! { #[derive( #(#d),* )] }
        });
        let cfg_attr = self.cfg_attr.iter();
        let repr = self.repr;
        let ident = &self.ident;
        let serde_defaults =
            SerdeDefaults::new(self.cfg_attr, self.derive, self.ident, self.generics)
                .filter(|_| self.kind == StructKind::Named);
        let fields = CGStructFieldsDef {
            fields: self.fields,
            is_ref,
            tuple: self.kind == StructKind::Tuple,
            cp: self.crate_path,
            serde_defaults: serde_defaults.as_ref(),
            attrs: self.base,
        };
        let where_clause = self.generics.where_clause(&[]);
        let body = match self.kind {
            StructKind::Named => quote! { #where_clause { #fields } },
            StructKind::Tuple => quote! { ( #fields ) #where_clause; },
            StructKind::Unit => quote! { #where_clause; },
        };
        let lifetime = self.generics.def(is_ref && !self.ambiguous);
        // a const assert outside an impl can't name a generic type: the size it checks is the parameters'
        let generic = !self.generics.is_empty();
        let assert_size = if generic {
            quote! {}
        } else if let Some(size) = &self.size_assumption {
            size.assert_element_size(self.ident, self.cfg, is_ref, quote! {}, "", self.crate_path)
        } else {
            quote! {}
        };
        let assert_tail_size = if generic {
            quote! {}
        } else {
            assert_sized_before_tail_size(
                &self.fields.iter().map(|f| &f.ty).collect::<Vec<_>>(),
                self.ident,
                self.cfg,
                is_ref,
                self.crate_path,
            )
        };
        let ts = quote! {
            #cfg
            #docs
            #derive
            #(#[cfg_attr(#cfg_attr)])*
            #(#repr)*
            pub struct #ident #lifetime #body
            #assert_size
            #assert_tail_size
        };
        let mut ts = ts;
        if serde_defaults.is_some() {
            let fns = self
                .fields
                .iter()
                .filter(|f| !matches!(f.ty, Type::IsOk(_) | Type::IsSome(_)))
                .filter_map(|f| {
                    let name = SerdeDefaults::fn_name(None, &f.ident);
                    let ty = f.ty.def(is_ref, self.crate_path);
                    Some(SerdeDefaults::fn_def(&name, ty, f.default.as_ref()?))
                })
                .collect();
            ts.append_all(SerdeDefaults::impl_block(
                fns,
                self.cfg,
                self.ident,
                self.generics,
                is_ref && !self.ambiguous,
            ));
        }
        ts
    }

    pub(crate) fn serdes_rust(&self, is_ref: bool) -> TokenStream {
        if let Some(slots) = self.sparse_slots {
            return self.sparse_serdes_rust(slots, is_ref);
        }
        if let Some(idx) = self.transparent {
            return self.transparent_serdes_rust(idx, is_ref);
        }
        let struct_name = &self.ident;
        let struct_ser = CGStructSer {
            item_struct: self,
            is_ref,
        };
        let struct_des = CGStructDes {
            item_struct: self,
            is_ref,
        };

        let element_size = match &self.size_assumption {
            // struct is Unsized by default, no need to look at the fields
            None | Some(ObjectSize::Unsized) => ObjectSize::Unsized.tokens(self.crate_path),
            Some(size) => {
                size.sum_fields(self.fields.iter().map(|f| &f.ty), is_ref, self.crate_path)
            }
        };
        serdes_scaffold(
            struct_name,
            self.generics,
            struct_ser,
            struct_des,
            self.cfg,
            element_size,
            Type::any_pushes_len(self.fields.iter().map(|f| &f.ty), self.crate_path),
            Type::seq_max_size(self.fields.iter().map(|f| &f.ty), self.crate_path),
            is_ref,
            is_ref && !self.ambiguous,
            None,
            self.crate_path,
        )
    }

    /// `sparse` (DER-13): an `OptionAnyVec` with the present fields in their slots, written in place.
    fn sparse_serdes_rust(&self, slots: &[u32], is_ref: bool) -> TokenStream {
        let cp = self.crate_path;
        let struct_name = &self.ident;
        let idents: Vec<_> = self.fields.iter().map(|f| &f.ident).collect();
        let members: Vec<_> = idents.iter().map(|i| self.kind.member(i)).collect();
        let inits = idents.iter().map(|i| self.kind.field_init(i));
        let count = slots.len();
        let builder = if is_ref {
            cp.item(quote! { OptionAnyVecBuilder })
        } else {
            cp.item(quote! { OptionAnyVecBuilderOwned })
        };
        let ser = quote! {
            let __present: [Option<u32>; #count] = [#(self.#members.as_ref().map(|_| #slots)),*];
            let mut __builder = #builder::new(wr, __present.iter().flatten().copied())?;
            #(
                if let Some(value) = &self.#members {
                    __builder.write(#slots, value, wr)?;
                }
            )*
            __builder.finish(wr)
        };
        let option_any_vec = cp.item(quote! { OptionAnyVec });
        let des_trait = cp.item(quote! { DeserializeShrinkWrap });
        let read_slot = if is_ref {
            quote! { read_slot }
        } else {
            quote! { read_slot_owned }
        };
        let des = quote! {
            let __vec = <#option_any_vec<'_> as #des_trait<'_>>::des_shrink_wrap(rd)?;
            let mut __slots = __vec.iter();
            #(let #idents = __slots.#read_slot(#slots)?;)*
            Ok(#struct_name { #(#inits),* })
        };
        // the header is never longer than the dense mode's: a mode nibble, `UNib32` slot count, a bit per slot
        let len = slots.last().map_or(0, |last| last + 1);
        let len_nibbles = (32 - len.leading_zeros()).div_ceil(3).max(1) as usize;
        let header_bits = 4 + 4 * len_nibbles + len as usize;
        let max_size_ty = cp.item(quote! { MaxSize });
        let fields_max_size = Type::seq_max_size(self.fields.iter().map(|f| &f.ty), cp);
        let max_size = quote! {
            #max_size_ty::seq(Some(#max_size_ty::bits(#header_bits)), #fields_max_size)
        };
        serdes_scaffold(
            struct_name,
            self.generics,
            ser,
            des,
            self.cfg,
            ObjectSize::Unsized.tokens(cp),
            Type::any_pushes_len(self.fields.iter().map(|f| &f.ty), cp),
            max_size,
            is_ref,
            is_ref && !self.ambiguous,
            None,
            cp,
        )
    }

    /// `transparent` (DER-18): the field's own serdes, `ELEMENT_SIZE`, `PUSHES_LEN` and `MAX_SIZE`, so the struct is
    /// the field on the wire in every position (a parent writes the size slot of an `Unsized` field once, for the
    /// field's type). The zero-bit fields are their `Default` when read.
    fn transparent_serdes_rust(&self, idx: usize, is_ref: bool) -> TokenStream {
        let cp = self.crate_path;
        let field = &self.fields[idx];
        let ty = field.ty.def(is_ref, cp);
        let member = self.kind.member(&field.ident);
        let (ser_trait, des_trait) = cp.serdes_traits(is_ref);
        let (ser, des) = if is_ref {
            (
                quote! { <#ty as #ser_trait>::ser_shrink_wrap(&self.#member, wr) },
                quote! { <#ty as #des_trait<'i>>::des_shrink_wrap(rd)? },
            )
        } else {
            (
                quote! { <#ty as #ser_trait>::ser_shrink_wrap_owned(&self.#member, wr) },
                quote! { <#ty as #des_trait>::des_shrink_wrap_owned(rd)? },
            )
        };
        let init = self.transparent_init(idx, quote! { __field });
        let validate = self.validate.map(|path| quote! { #path(&__value)?; });
        // as a field of a derived type, the struct is written and read exactly like its field, a latched one
        // included; a failed `validate` is kept for `rd.latched()?` like a failed read (an earlier error stays
        // first, and a value read after an error, 0, may fail it too)
        let validate_latch = self.validate.map(|path| {
            quote! {
                if let Err(e) = #path(&__value) {
                    rd.keep_error(e);
                }
            }
        });
        let latch = if is_ref {
            (
                quote! { <#ty as #ser_trait>::ser_shrink_wrap_latch(&self.#member, wr) },
                quote! {
                    let __field = <#ty as #des_trait<'i>>::des_shrink_wrap_latch(rd)?;
                    let __value = #init;
                    #validate_latch
                    Ok(__value)
                },
            )
        } else {
            (
                quote! { <#ty as #ser_trait>::ser_shrink_wrap_owned_latch(&self.#member, wr) },
                quote! {
                    let __field = <#ty as #des_trait>::des_shrink_wrap_owned_latch(rd)?;
                    let __value = #init;
                    #validate_latch
                    Ok(__value)
                },
            )
        };
        let des = quote! {
            let __field = #des;
            let __value = #init;
            #validate
            Ok(__value)
        };
        serdes_scaffold(
            self.ident,
            self.generics,
            ser,
            des,
            self.cfg,
            quote! { <#ty as #ser_trait>::ELEMENT_SIZE },
            Some(quote! { <#ty as #ser_trait>::PUSHES_LEN }),
            quote! { <#ty as #ser_trait>::MAX_SIZE },
            is_ref,
            is_ref && !self.ambiguous,
            Some(latch),
            cp,
        )
    }

    /// `Self { <field>: #value, <zero-bit fields>: Default::default() }` of a `transparent` struct.
    fn transparent_init(&self, idx: usize, value: TokenStream) -> TokenStream {
        transparent_init(self.fields, self.kind, idx, value)
    }

    /// DER-12: `core::fmt::Display` (from `message`, the struct's `display` message) and `core::error::Error`.
    pub(crate) fn display_and_error_rust(&self, is_ref: bool, message: &str) -> TokenStream {
        let cfg = self.cfg.map(|cond| quote! { #[cfg(#cond)] });
        let ident = &self.ident;
        let params = self.generics.impl_params(None);
        let ty_generics = self.generics.ty(is_ref && !self.ambiguous);
        let where_clause = self.generics.where_clause(&[]);
        let error_where = self.generics.error_where();
        let lit = LitStr::new(&rewrite_positional_placeholders(message), Span::call_site());
        let binds = self.fields.iter().filter_map(|f| {
            if matches!(f.ty, Type::IsOk(_) | Type::IsSome(_)) {
                return None;
            }
            let field = &f.ident;
            let member = self.kind.member(field);
            Some(quote! { let #field = &self.#member; })
        });
        quote! {
            #cfg
            impl<'i, #params> ::core::fmt::Display for #ident #ty_generics #where_clause {
                #[allow(unused_variables)]
                fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    #(#binds)*
                    write!(f, #lit)
                }
            }

            #cfg
            impl<'i, #params> ::core::error::Error for #ident #ty_generics #error_where {}
        }
    }
}

struct CGStructFieldsDef<'a> {
    fields: &'a [Field],
    is_ref: bool,
    /// `pub T,` instead of `pub name: T,`
    tuple: bool,
    cp: &'a CratePath,
    /// DER-17: when the type derives `serde::Deserialize`
    serde_defaults: Option<&'a SerdeDefaults>,
    /// Keep the fields' attributes the derive doesn't read.
    attrs: bool,
}

impl ToTokens for CGStructFieldsDef<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for struct_field in self.fields {
            if matches!(struct_field.ty, Type::IsOk(_) | Type::IsSome(_)) {
                continue;
            }
            let ident = &struct_field.ident;
            let ty = struct_field.ty.def(self.is_ref, self.cp);
            let docs = &struct_field.docs;
            let serde_default = match (self.serde_defaults, &struct_field.default) {
                (Some(serde), Some(_)) => serde.field_attr(&SerdeDefaults::fn_name(None, ident)),
                _ => quote! {},
            };
            let attrs = maybe_attrs(self.attrs, &struct_field.attrs);
            let docs = quote! { #docs #serde_default #(#attrs)* };
            if self.tuple {
                tokens.append_all(quote! {
                    #docs
                    pub #ty,
                });
            } else {
                tokens.append_all(quote! {
                    #docs
                    pub #ident: #ty,
                });
            }
        }
    }
}

struct CGStructSer<'a> {
    item_struct: &'a CGItemStruct<'a>,
    is_ref: bool,
}

struct CGStructDes<'a> {
    item_struct: &'a CGItemStruct<'a>,
    is_ref: bool,
}

impl ToTokens for CGStructSer<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let tuple = self.item_struct.kind == StructKind::Tuple;
        if tuple {
            // bound like the fields of a tuple variant, so that a flag finds its `Option` or `Result` by name
            for struct_field in self.item_struct.fields {
                if !matches!(struct_field.ty, Type::IsOk(_) | Type::IsSome(_)) {
                    let field_name = &struct_field.ident;
                    let member = self.item_struct.kind.member(field_name);
                    tokens.append_all(quote! { let #field_name = &self.#member; });
                }
            }
        }
        for struct_field in self.item_struct.fields {
            let field_name = &struct_field.ident;
            let is_flag = matches!(struct_field.ty, Type::IsOk(_) | Type::IsSome(_));
            let field_path = match (tuple, is_flag) {
                (true, true) => FieldPath::Ref(quote! {}),
                (true, false) => FieldPath::Ref(quote! {#field_name}),
                (false, true) => FieldPath::Value(quote! {self}),
                (false, false) => FieldPath::Value(quote! {self.#field_name}),
            };
            struct_field.ty.buf_write(
                field_path,
                self.is_ref,
                quote! { ? },
                self.item_struct.crate_path,
                tokens,
            );
        }
        if self
            .item_struct
            .fields
            .iter()
            .any(|f| matches!(f.ty, Type::TailSize(_)))
        {
            Type::backfill_tail_size(tokens);
        }
        tokens.append_all(quote! {
            wr.latched()
        });
    }
}

impl ToTokens for CGStructDes<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut field_names = vec![];
        for struct_field in self.item_struct.fields {
            let field_name = &struct_field.ident;
            if !matches!(struct_field.ty, Type::IsOk(_) | Type::IsSome(_)) {
                field_names.push(self.item_struct.kind.field_init(field_name));
            }
            struct_field.buf_read(!self.is_ref, self.item_struct.crate_path, tokens);
        }
        let struct_name = &self.item_struct.ident;
        let validate = self
            .item_struct
            .validate
            .map(|path| quote! { #path(&__value)?; });
        tokens.append_all(quote! {
            rd.latched()?;
            let __value = #struct_name {
                #(#field_names),*
            };
            #validate
            Ok(__value)
        });
    }
}

impl Field {
    /// `let <field> = ..;` reading the field. A field with `#[default = ..]` was added in a later version: it is its
    /// default when it starts at the end of the data (no whole byte left) and its read runs out of data, any other
    /// error is reported (SW-15). The flag of an evolved `Option` or `Result` notes that, the value then is the
    /// default.
    pub(crate) fn buf_read(&self, owned: bool, cp: &CratePath, tokens: &mut TokenStream) {
        let ident = &self.ident;
        let Some(default) = &self.default else {
            self.ty.buf_read(ident, owned, quote!(?), true, cp, tokens);
            return;
        };
        let at_end = quote! { let __at_end = rd.bytes_left() == 0; };
        match &self.ty {
            Type::IsSome(value) | Type::IsOk(value) => {
                let (absent, at_end) = evolved_flag_idents(value);
                tokens.append_all(quote! {
                    let #at_end = rd.bytes_left() == 0;
                    let mut #absent = false;
                });
                let fallback = quote! {
                    .or_else(|e| if #at_end && e.is_read_eob() { #absent = true; Ok(false) } else { Err(e) })?
                };
                self.ty.buf_read(ident, owned, fallback, false, cp, tokens);
            }
            Type::Option(_, _) => {
                // a missing flag reads as false, nothing more is read then
                let (absent, _) = evolved_flag_idents(ident);
                let mut read = TokenStream::new();
                self.ty
                    .buf_read(ident, owned, quote!(?), false, cp, &mut read);
                tokens.append_all(quote! {
                    let #ident = if #absent { #default } else { #read #ident };
                });
            }
            Type::Result(is_ok, _) => {
                // A flag that started at the end of the data can be read from padding bits, which are 0: then the
                // `Err` value is missing too. A flag that reads 1 is data, the `Ok` value must follow.
                let (absent, at_end) = evolved_flag_idents(ident);
                let error = cp.error();
                let mut read = TokenStream::new();
                self.ty
                    .buf_read(ident, owned, quote!(?), false, cp, &mut read);
                tokens.append_all(quote! {
                    let #ident = if #absent {
                        #default
                    } else {
                        match (|| -> Result<_, #error> { #read Ok(#ident) })() {
                            Ok(value) => value,
                            Err(e) if #at_end && !#is_ok && e.is_read_eob() => #default,
                            Err(e) => return Err(e),
                        }
                    };
                });
            }
            _ => {
                let fallback = quote! {
                    .or_else(|e| if __at_end && e.is_read_eob() { Ok(#default) } else { Err(e) })?
                };
                tokens.append_all(at_end);
                self.ty.buf_read(ident, owned, fallback, false, cp, tokens);
            }
        }
    }
}

/// Variables of the flag of an evolved `Option` / `Result` field `value`: set when the flag is missing from the data,
/// and whether the flag started at the end of the data.
fn evolved_flag_idents(value: &Ident) -> (Ident, Ident) {
    (
        quote::format_ident!("__absent_{}", value),
        quote::format_ident!("__at_end_{}", value),
    )
}

/// `Self { <field>: #value, <zero-bit fields>: Default::default() }` of a `transparent` struct (DER-18): flags are
/// part of their `Option` / `Result`, `PhantomData` and `()` are their default.
pub(crate) fn transparent_init(
    fields: &[Field],
    kind: StructKind,
    idx: usize,
    value: TokenStream,
) -> TokenStream {
    let inits = fields.iter().enumerate().filter_map(|(i, f)| {
        if matches!(f.ty, Type::IsOk(_) | Type::IsSome(_)) {
            return None;
        }
        let member = kind.member(&f.ident);
        Some(if i == idx {
            quote! { #member: #value }
        } else {
            quote! { #member: ::core::default::Default::default() }
        })
    });
    quote! { Self { #(#inits),* } }
}
