//! `#[derive(Settings)]` for `ww_settings`: implements `Default` and `ww_settings::SettingsGroup` for a struct
//! whose every field has `#[setting(slot = N, default = <expr>)]`.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{Data, DeriveInput, Expr, Fields, Ident, LitInt, Type, parse_macro_input};

/// Highest slot id, `ww_settings::group::MAX_SLOT`.
const MAX_SLOT: u32 = 255;

/// Implements `Default` (every field at its `default`) and `ww_settings::SettingsGroup`.
///
/// ```ignore
/// #[derive(Settings, Clone, Debug, PartialEq)]
/// #[settings(retired(2))]               // slots of removed fields, never used again
/// pub struct UserSettings {
///     #[setting(slot = 0, default = 100)]
///     pub meter_window_ms: u16,
///     #[setting(slot = 1, default = false)]
///     pub plug_vbus_at_boot: bool,
/// }
/// ```
///
/// Each field type must implement `SerializeShrinkWrap`, `DeserializeShrinkWrap` (an owned type, no lifetime)
/// and `PartialEq`. Slots are `0..=255`, unique, and not retired; the field order doesn't matter.
#[proc_macro_derive(Settings, attributes(setting, settings))]
pub fn derive_settings(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match derive(input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

struct Field {
    ident: Ident,
    ty: Type,
    slot: u32,
    default: Expr,
}

fn derive(input: DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "a settings struct has no generics or lifetimes: its fields are owned values",
        ));
    }
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "Settings is derived for structs only",
        ));
    };
    let Fields::Named(named) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "Settings needs a struct with named fields",
        ));
    };

    let mut retired: Vec<(u32, Span)> = Vec::new();
    for attr in &input.attrs {
        if !attr.path().is_ident("settings") {
            continue;
        }
        // `#[settings(retired(2, 5))]`
        let list: syn::MetaList = attr.parse_args()?;
        if !list.path.is_ident("retired") {
            return Err(syn::Error::new_spanned(
                &list.path,
                "expected `retired(..)`",
            ));
        }
        let slots = list.parse_args_with(
            syn::punctuated::Punctuated::<LitInt, syn::Token![,]>::parse_terminated,
        )?;
        for lit in slots {
            let n: u32 = lit.base10_parse()?;
            retired.push((n, lit.span()));
        }
    }

    let mut fields = Vec::new();
    for f in &named.named {
        let ident = f.ident.clone().expect("named field");
        let mut slot = None;
        let mut default = None;
        for attr in &f.attrs {
            if !attr.path().is_ident("setting") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("slot") {
                    let lit: LitInt = meta.value()?.parse()?;
                    let n: u32 = lit.base10_parse()?;
                    if n > MAX_SLOT {
                        return Err(syn::Error::new_spanned(lit, "slots are 0..=255"));
                    }
                    slot = Some(n);
                    Ok(())
                } else if meta.path.is_ident("default") {
                    default = Some(meta.value()?.parse::<Expr>()?);
                    Ok(())
                } else {
                    Err(meta.error("expected `slot = N` or `default = <expr>`"))
                }
            })?;
        }
        let (Some(slot), Some(default)) = (slot, default) else {
            return Err(syn::Error::new_spanned(
                &ident,
                "every field needs #[setting(slot = N, default = <expr>)]",
            ));
        };
        fields.push(Field {
            ident,
            ty: f.ty.clone(),
            slot,
            default,
        });
    }

    for (i, f) in fields.iter().enumerate() {
        if fields[..i].iter().any(|g| g.slot == f.slot) {
            return Err(syn::Error::new_spanned(
                &f.ident,
                format!("slot {} is used twice", f.slot),
            ));
        }
        if retired.iter().any(|(r, _)| *r == f.slot) {
            return Err(syn::Error::new_spanned(
                &f.ident,
                format!(
                    "slot {} is retired: a removed field's slot is never used again, take a new one",
                    f.slot
                ),
            ));
        }
    }
    for (i, (r, span)) in retired.iter().enumerate() {
        if *r > MAX_SLOT {
            return Err(syn::Error::new(*span, "slots are 0..=255"));
        }
        if retired[..i].iter().any(|(q, _)| q == r) {
            return Err(syn::Error::new(*span, "retired twice"));
        }
    }

    let mut sorted: Vec<&Field> = fields.iter().collect();
    sorted.sort_by_key(|f| f.slot);
    let slots = sorted.iter().map(|f| {
        let id = f.slot;
        let name = f.ident.to_string();
        quote! { ::ww_settings::Slot { id: #id, name: #name } }
    });
    let mut retired_ids: Vec<u32> = retired.iter().map(|(r, _)| *r).collect();
    retired_ids.sort();

    let default_fields = fields.iter().map(|f| {
        let (ident, ty, default) = (&f.ident, &f.ty, &f.default);
        quote! { #ident: { let d: #ty = #default; d } }
    });
    let is_set = fields.iter().map(|f| {
        let (ident, ty, default, slot) = (&f.ident, &f.ty, &f.default, f.slot);
        quote! { #slot => { let d: #ty = #default; self.#ident != d } }
    });
    let write_slot = fields.iter().map(|f| {
        let (ident, slot) = (&f.ident, f.slot);
        quote! { #slot => ::ww_settings::shrink_wrap::SerializeShrinkWrap::to_ww_bytes(&self.#ident, buf) }
    });
    let read_slot = fields.iter().map(|f| {
        let (ident, ty, slot) = (&f.ident, &f.ty, f.slot);
        quote! {
            #slot => {
                self.#ident = <#ty as ::ww_settings::shrink_wrap::DeserializeShrinkWrap>::from_ww_bytes(bytes)?;
                Ok(())
            }
        }
    });
    let slot_eq = fields.iter().map(|f| {
        let (ident, ty, slot) = (&f.ident, &f.ty, f.slot);
        quote! {
            #slot => <#ty as ::ww_settings::shrink_wrap::DeserializeShrinkWrap>::from_ww_bytes(bytes)
                .is_ok_and(|v| v == self.#ident)
        }
    });
    let slot_decodes = fields.iter().map(|f| {
        let (ty, slot) = (&f.ty, f.slot);
        quote! {
            #slot => <#ty as ::ww_settings::shrink_wrap::DeserializeShrinkWrap>::from_ww_bytes(bytes).is_ok()
        }
    });
    let reset_slot = fields.iter().map(|f| {
        let (ident, ty, default, slot) = (&f.ident, &f.ty, &f.default, f.slot);
        quote! { #slot => { let d: #ty = #default; self.#ident = d; } }
    });

    Ok(quote! {
        impl ::core::default::Default for #name {
            fn default() -> Self {
                #name { #(#default_fields),* }
            }
        }

        impl ::ww_settings::SettingsGroup for #name {
            const VERSION: ::ww_settings::Version = ::ww_settings::Version::parse(env!("CARGO_PKG_VERSION"));
            const SLOTS: &'static [::ww_settings::Slot] = &[#(#slots),*];
            const RETIRED: &'static [u32] = &[#(#retired_ids),*];

            fn defaults() -> Self {
                ::core::default::Default::default()
            }

            fn is_set(&self, slot: u32) -> bool {
                match slot {
                    #(#is_set)*
                    _ => false,
                }
            }

            fn write_slot<'b>(
                &self,
                slot: u32,
                buf: &'b mut [u8],
            ) -> ::core::result::Result<&'b [u8], ::ww_settings::shrink_wrap::Error> {
                match slot {
                    #(#write_slot,)*
                    _ => Err(::ww_settings::shrink_wrap::Error::OptionAnyVecUnknownSlot),
                }
            }

            fn read_slot(
                &mut self,
                slot: u32,
                bytes: &[u8],
            ) -> ::core::result::Result<(), ::ww_settings::shrink_wrap::Error> {
                match slot {
                    #(#read_slot)*
                    _ => Err(::ww_settings::shrink_wrap::Error::OptionAnyVecUnknownSlot),
                }
            }

            fn slot_eq(&self, slot: u32, bytes: &[u8]) -> bool {
                match slot {
                    #(#slot_eq,)*
                    _ => false,
                }
            }

            fn slot_decodes(slot: u32, bytes: &[u8]) -> bool {
                match slot {
                    #(#slot_decodes,)*
                    _ => false,
                }
            }

            fn reset_slot(&mut self, slot: u32) {
                match slot {
                    #(#reset_slot)*
                    _ => {}
                }
            }
        }
    })
}
