use crate::ast::docs::Docs;
use crate::ast::item_struct::Field;
use crate::ast::object_size::ObjectSize;
use crate::ast::ty::Type;
use crate::codegen::ty::FieldPath;
use crate::codegen::util::{
    CratePath, assert_sized_before_tail_size, maybe_quote, rewrite_positional_placeholders,
    serdes_scaffold,
};
use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, quote};
use syn::{LitStr, Path};

#[derive(Clone, Debug)]
pub(crate) struct CGItemStruct<'i> {
    pub(crate) docs: &'i Docs,
    pub(crate) ident: &'i Ident,
    pub(crate) fields: &'i [Field],
    /// `sparse` (DER-13): the slot of each field
    pub(crate) sparse_slots: Option<&'i [u32]>,
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

impl<'i> CGItemStruct<'i> {
    pub(crate) fn def_rust(&self, is_ref: bool) -> TokenStream {
        let cfg = self.cfg.map(|cond| quote! { #[cfg(#cond)] });
        let docs = &self.docs;
        let derive = maybe_quote(!self.derive.is_empty(), || {
            let d = self.derive.iter();
            quote! { #[derive( #(#d),* )] }
        });
        let cfg_attr = self.cfg_attr.iter();
        let ident = &self.ident;
        let fields = CGStructFieldsDef {
            fields: self.fields,
            is_ref,
            cp: self.crate_path,
        };
        let lifetime = maybe_quote(is_ref && !self.ambiguous, || quote! { <'i> });
        let assert_size = if let Some(size) = &self.size_assumption {
            size.assert_element_size(self.ident, self.cfg, is_ref, quote! {}, "", self.crate_path)
        } else {
            quote! {}
        };
        let assert_tail_size = assert_sized_before_tail_size(
            &self.fields.iter().map(|f| &f.ty).collect::<Vec<_>>(),
            self.ident,
            self.cfg,
            is_ref,
            self.crate_path,
        );
        let ts = quote! {
            #cfg
            #docs
            #derive
            #(#[cfg_attr(#cfg_attr)])*
            pub struct #ident #lifetime { #fields }
            #assert_size
            #assert_tail_size
        };
        ts
    }

    pub(crate) fn serdes_rust(&self, is_ref: bool) -> TokenStream {
        if let Some(slots) = self.sparse_slots {
            return self.sparse_serdes_rust(slots, is_ref);
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
            struct_ser,
            struct_des,
            self.cfg,
            element_size,
            Type::any_pushes_len(self.fields.iter().map(|f| &f.ty), self.crate_path),
            Type::seq_max_size(self.fields.iter().map(|f| &f.ty), self.crate_path),
            is_ref,
            is_ref && !self.ambiguous,
            self.crate_path,
        )
    }

    /// `sparse` (DER-13): an `OptionAnyVec` with the present fields in their slots, written in place.
    fn sparse_serdes_rust(&self, slots: &[u32], is_ref: bool) -> TokenStream {
        let cp = self.crate_path;
        let struct_name = &self.ident;
        let idents: Vec<_> = self.fields.iter().map(|f| &f.ident).collect();
        let count = slots.len();
        let builder = if is_ref {
            cp.item(quote! { OptionAnyVecBuilder })
        } else {
            cp.item(quote! { OptionAnyVecBuilderOwned })
        };
        let ser = quote! {
            let __present: [Option<u32>; #count] = [#(self.#idents.as_ref().map(|_| #slots)),*];
            let mut __builder = #builder::new(wr, __present.iter().flatten().copied())?;
            #(
                if let Some(value) = &self.#idents {
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
            Ok(#struct_name { #(#idents),* })
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
            ser,
            des,
            self.cfg,
            ObjectSize::Unsized.tokens(cp),
            Type::any_pushes_len(self.fields.iter().map(|f| &f.ty), cp),
            max_size,
            is_ref,
            is_ref && !self.ambiguous,
            cp,
        )
    }

    /// DER-12: `core::fmt::Display` (from `message`, the struct's `display` message) and `core::error::Error`.
    pub(crate) fn display_and_error_rust(&self, is_ref: bool, message: &str) -> TokenStream {
        let cfg = self.cfg.map(|cond| quote! { #[cfg(#cond)] });
        let ident = &self.ident;
        let lifetime = maybe_quote(is_ref && !self.ambiguous, || quote! { <'i> });
        let lit = LitStr::new(&rewrite_positional_placeholders(message), Span::call_site());
        let binds = self.fields.iter().filter_map(|f| {
            if matches!(f.ty, Type::IsOk(_) | Type::IsSome(_)) {
                return None;
            }
            let field = &f.ident;
            Some(quote! { let #field = &self.#field; })
        });
        quote! {
            #cfg
            impl #lifetime ::core::fmt::Display for #ident #lifetime {
                #[allow(unused_variables)]
                fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    #(#binds)*
                    write!(f, #lit)
                }
            }

            #cfg
            impl #lifetime ::core::error::Error for #ident #lifetime {}
        }
    }
}

struct CGStructFieldsDef<'a> {
    fields: &'a [Field],
    is_ref: bool,
    cp: &'a CratePath,
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
            tokens.append_all(quote! {
                #docs
                pub #ident: #ty,
            });
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
        for struct_field in self.item_struct.fields {
            let field_name = &struct_field.ident;
            let field_path = if matches!(struct_field.ty, Type::IsOk(_) | Type::IsSome(_)) {
                FieldPath::Value(quote! {self})
            } else {
                FieldPath::Value(quote! {self.#field_name})
            };
            struct_field
                .ty
                .buf_write(field_path, self.is_ref, quote! { ? }, tokens);
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
                field_names.push(field_name.clone());
            }
            struct_field.buf_read(!self.is_ref, self.item_struct.crate_path, tokens);
        }
        let struct_name = &self.item_struct.ident;
        tokens.append_all(quote! {
            rd.latched()?;
            Ok(#struct_name {
                #(#field_names),*
            })
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
