use crate::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned,
    ArgumentOwned, Batching, ClaimMode, Delivery, ErrorVariantOwned, FieldOwned, FieldsOwned,
    FlowControl, GenericArgOwned, GenericParamOwned, ItemEnumOwned, ItemStructOwned, Length,
    Multiplicity, Repr, TypeLocationOwned, TypeOwned,
};
use anyhow::{Result, anyhow, bail};
use shrink_wrap::ElementSize;
use std::collections::HashSet;
use ww_numeric::{NumericAnyTypeOwned, NumericBaseType};
use ww_version::FullVersionOwned;

impl ApiLevelOwned {
    pub fn crate_name<'i>(&self, bundle: &'i ApiBundleOwned) -> Result<&'i str> {
        bundle.crate_name(self.crate_idx.0)
    }

    fn get_item<'i>(
        &'i self,
        id_chain: &[u32],
        api_bundle: &'i ApiBundleOwned,
    ) -> Result<&'i ApiItemOwned> {
        if let Some(idx) = id_chain.first() {
            let item = self
                .items
                .iter()
                .find(|i| i.id.0 == *idx)
                .ok_or(anyhow!("no item with id: {}", idx))?;
            if id_chain.len() == 1 {
                Ok(item)
            } else {
                if let ApiItemKindOwned::Trait { trait_idx, .. } = &item.kind {
                    let level = api_bundle.get_trait(trait_idx.0)?;
                    level.get_item(&id_chain[1..], api_bundle)
                } else {
                    Err(anyhow!("cannot index not a trait: {:?}", item))
                }
            }
        } else {
            Err(anyhow!("internal: empty index_chain"))
        }
    }
}

impl ApiItemOwned {
    pub fn get_as_level<'i>(&self, bundle: &'i ApiBundleOwned) -> Result<&'i ApiLevelOwned> {
        let ApiItemKindOwned::Trait { trait_idx, .. } = &self.kind else {
            return Err(anyhow!("ApiItem is not Trait"));
        };
        bundle.get_trait(trait_idx.0)
    }

    pub fn is_array(&self) -> bool {
        matches!(self.multiplicity, Multiplicity::Array { .. })
    }

    /// How a client can claim this `ww_impl!` through the hub (SRV-34): `#[claim(..)]` on it, else on its trait's
    /// definition; `None` for an item that can't be claimed. A trait left out of `bundle` (skipped, not restored
    /// from a snapshot) counts only by the `ww_impl!`.
    pub fn claim_mode(&self, bundle: &ApiBundleOwned) -> Option<ClaimMode> {
        let ApiItemKindOwned::Trait { trait_idx, .. } = &self.kind else {
            return None;
        };
        if let Some(mode) = self.meta.as_ref().and_then(|m| m.claim) {
            return Some(mode);
        }
        match bundle.traits.get(trait_idx.0 as usize) {
            Some(ApiLevelLocationOwned::InLine { level, .. }) => level.meta.as_ref()?.claim,
            _ => None,
        }
    }

    /// `#[label(..)]` of the item, of element `index` for a resource array (`None`: the first one, as for a flat
    /// item); `None` without one.
    pub fn label(&self, index: Option<u32>) -> Option<&str> {
        let labels = &self.meta.as_ref()?.labels;
        labels.get(index.unwrap_or(0) as usize).map(String::as_str)
    }
}

impl ClaimMode {
    /// As written in `#[claim(..)]`: `exclusive`, `shared` or `observe`.
    pub fn name(&self) -> &'static str {
        match self {
            ClaimMode::Exclusive => "exclusive",
            ClaimMode::Shared => "shared",
            ClaimMode::Observe => "observe",
        }
    }
}

impl ApiItemKindOwned {
    /// Delivery kind of a stream (device to host), `None` for everything else: `Plain` for `stream!` and plain
    /// stream functions.
    pub fn delivery(&self) -> Option<Delivery> {
        match self {
            ApiItemKindOwned::StreamFn { delivery, .. } => Some(*delivery),
            ApiItemKindOwned::Stream { is_up: true, .. } => Some(Delivery::Plain),
            _ => None,
        }
    }

    /// A stream (device to host) seen as a stream function: item type, delivery, arguments and return type. A plain
    /// `stream!(x: T)` is `#[stream(T)] fn x()`.
    pub fn as_stream_fn(&self) -> Option<StreamFnRef<'_>> {
        match self {
            ApiItemKindOwned::StreamFn {
                ty,
                delivery,
                args,
                return_ty,
                errors,
                batched,
                batch_item_ty,
            } => Some(StreamFnRef {
                ty,
                delivery: *delivery,
                args,
                return_ty: return_ty.as_ref(),
                errors: errors.as_deref(),
                batched: *batched,
                batch_item_ty: batch_item_ty.as_ref(),
            }),
            ApiItemKindOwned::Stream { ty, is_up: true } => Some(StreamFnRef {
                ty,
                delivery: Delivery::Plain,
                args: &[],
                return_ty: None,
                errors: None,
                batched: None,
                batch_item_ty: None,
            }),
            _ => None,
        }
    }

    /// Stream or sink: its item type, whether it goes from device to host (a stream), and the flow control of a
    /// sink that has one. A counted or reliable stream is a stream here, see [Self::delivery].
    pub fn as_stream(&self) -> Option<(&TypeOwned, bool, Option<FlowControl>)> {
        match self {
            ApiItemKindOwned::Stream { ty, is_up } => Some((ty, *is_up, None)),
            ApiItemKindOwned::FlowSink { ty, flow } => Some((ty, false, Some(*flow))),
            ApiItemKindOwned::StreamFn { ty, .. } => Some((ty, true, None)),
            _ => None,
        }
    }
}

/// A stream as a stream function, see [ApiItemKindOwned::as_stream_fn].
#[derive(Debug, Clone, Copy)]
pub struct StreamFnRef<'a> {
    pub ty: &'a TypeOwned,
    pub delivery: Delivery,
    pub args: &'a [ArgumentOwned],
    pub return_ty: Option<&'a TypeOwned>,
    /// `#[errors(..)]` of the stream function (API-32).
    pub errors: Option<&'a [ErrorVariantOwned]>,
    /// Items sent in batches (API-41).
    pub batched: Option<Batching>,
    /// [Batching::Timed] only: the `Timed<T>` instance, see
    /// [ApiItemKind::StreamFn::batch_item_ty](crate::ApiItemKind::StreamFn).
    pub batch_item_ty: Option<&'a TypeOwned>,
}

impl<'a> StreamFnRef<'a> {
    /// What one event of this stream carries on the wire: `ty` directly when it isn't batched, `Series<T>`
    /// (untimed) or `Series<Timed<T>>` (timed) otherwise (API-41), for a dynamic decoder. Errors on a form of
    /// batching from a future `ww_self` this one doesn't know.
    pub fn wire_item_ty(&self) -> Result<TypeOwned> {
        Ok(match self.batched {
            None => self.ty.clone(),
            Some(Batching::Untimed) => TypeOwned::Series(Box::new(self.ty.clone())),
            Some(Batching::Timed) => TypeOwned::Series(Box::new(
                self.batch_item_ty
                    .cloned()
                    .ok_or_else(|| anyhow!("a batched timed stream without batch_item_ty"))?,
            )),
            Some(Batching::SharedClock) => {
                bail!("a stream with the batching of properties (SharedClock)")
            }
            Some(Batching::Unknown(_)) => {
                bail!("a form of batching this ww_self doesn't know, update it")
            }
        })
    }
}

impl ApiBundleOwned {
    pub fn get_trait(&self, trait_idx: u32) -> Result<&ApiLevelOwned> {
        let location = self
            .traits
            .get(trait_idx as usize)
            .ok_or(anyhow!("Bad ApiBundle: no trait with index: {}", trait_idx))?;
        let ApiLevelLocationOwned::InLine {
            level,
            crate_idx: _,
        } = location
        else {
            return Err(anyhow!(
                "Not resolved ApiBundle: trait with index: {} is not inlined",
                trait_idx
            ));
        };
        Ok(level)
    }

    pub fn get_ty(&self, type_idx: u32) -> Result<(&TypeOwned, u32)> {
        let location = self
            .types
            .get(type_idx as usize)
            .ok_or(anyhow!("Bad ApiBundle: no type with index: {}", type_idx))?;
        if let TypeLocationOwned::InLine { ty, crate_idx } = location {
            // only structs and enums are definitions: one that refers to another type (or to itself) would send
            // every walker following it into a loop on an untrusted bundle
            if !matches!(ty, TypeOwned::Struct(_) | TypeOwned::Enum(_)) {
                return Err(anyhow!(
                    "Bad ApiBundle: type with index: {} is not a struct or an enum",
                    type_idx
                ));
            }
            Ok((ty, crate_idx.0))
        } else {
            Err(anyhow!(
                "Not resolved ApiBundle: type with index: {} is not inlined",
                type_idx
            ))
        }
    }

    pub fn crate_name(&self, crate_idx: u32) -> Result<&str> {
        let version = self.crate_version(crate_idx)?;
        Ok(version.crate_id.as_str())
    }

    pub fn crate_version(&self, crate_idx: u32) -> Result<&FullVersionOwned> {
        let Some(version) = self.ext_crates.get(crate_idx as usize) else {
            return Err(anyhow!("Bad ApiBundle: no crate with index: {}", crate_idx));
        };
        Ok(version)
    }

    /// Get an API item at the provided path.
    /// Note that array indices must not be provided, as only the structure exists in this AST, not actual elements.
    /// I.e., for a resource tree:
    /// /TraitA[]
    ///   /item_a
    /// /TraitB
    ///   /item_c
    ///   /TraitC[]/item_d
    /// item_a path is: `0, 0`, TraitB: `1`, item_c: `1, 1` and item_d: `1, 1, 0`.
    pub fn get_item(&self, id_chain: &[u32]) -> Result<&ApiItemOwned> {
        self.root.get_item(id_chain, self)
    }
}

impl TypeOwned {
    /// Returns true if this type contains a string, vector, or box at any depth.
    pub fn is_lifetime(&self, api_bundle: &ApiBundleOwned) -> Result<bool> {
        self.is_lifetime_in(api_bundle, &mut HashSet::new())
    }

    /// `seen`: out-of-line definitions already looked into, each one is once (self-referential types).
    fn is_lifetime_in(&self, api_bundle: &ApiBundleOwned, seen: &mut HashSet<u32>) -> Result<bool> {
        match self {
            TypeOwned::Bool => Ok(false),
            TypeOwned::NumericAny(_) => Ok(false),
            TypeOwned::OutOfLine { type_idx } => {
                if !seen.insert(type_idx.0) {
                    return Ok(false);
                }
                let ty = api_bundle.get_ty(type_idx.0)?;
                ty.0.is_lifetime_in(api_bundle, seen)
            }
            // the definition's own fields (parameters don't count), or one of the arguments
            TypeOwned::Instance { type_idx, args } => {
                if seen.insert(type_idx.0)
                    && api_bundle
                        .get_ty(type_idx.0)?
                        .0
                        .is_lifetime_in(api_bundle, seen)?
                {
                    return Ok(true);
                }
                for arg in args {
                    if let GenericArgOwned::Type(ty) = arg
                        && ty.is_lifetime_in(api_bundle, seen)?
                    {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            TypeOwned::Param(_) => Ok(false),
            TypeOwned::Flag => Ok(false),
            TypeOwned::String => Ok(true),
            TypeOwned::Vec(_) => Ok(true),
            TypeOwned::Array { ty, .. } => ty.is_lifetime_in(api_bundle, seen),
            TypeOwned::Tuple(types) => {
                for ty in types {
                    if ty.is_lifetime_in(api_bundle, seen)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            TypeOwned::Struct(item_struct) => item_struct.fields.is_lifetime(api_bundle, seen),
            TypeOwned::Enum(item_enum) => item_enum.is_lifetime_in(api_bundle, seen),
            TypeOwned::Option { some_ty } => some_ty.is_lifetime_in(api_bundle, seen),
            TypeOwned::Result { ok_ty, err_ty } => {
                if ok_ty.is_lifetime_in(api_bundle, seen)? {
                    return Ok(true);
                }
                err_ty.is_lifetime_in(api_bundle, seen)
            }
            TypeOwned::Box(_) => Ok(true),
            TypeOwned::Range(_) => Ok(false),
            TypeOwned::RangeInclusive(_) => Ok(false),
            TypeOwned::MaxLen { ty, .. } => ty.is_lifetime_in(api_bundle, seen),
            // a slice or a reader of the buffer, like RefVec
            TypeOwned::Series(_) => Ok(true),
        }
    }

    /// Whether this type is [ElementSize::Unsized], i.e., its size is recorded (in the FIFO of sizes at the back of
    /// the buffer) when written as a field, an element of a Vec, etc. (`BufWriter::write`), so that it can evolve.
    ///
    /// Strings, Vecs, arrays and tuples are `UnsizedFinalStructure`, `Option` and `Result` are `SelfDescribing`, so
    /// their size is never written, even if they contain Unsized types.
    pub fn is_unsized(&self, api_bundle: &ApiBundleOwned) -> Result<bool> {
        match self {
            TypeOwned::OutOfLine { type_idx } | TypeOwned::Instance { type_idx, .. } => {
                let ty = api_bundle.get_ty(type_idx.0)?;
                ty.0.is_unsized(api_bundle)
            }
            TypeOwned::Param(idx) => Err(anyhow!(
                "type parameter {} of a generic definition, not an instance of it",
                idx.0
            )),
            TypeOwned::Struct(item_struct) => Ok(item_struct.is_unsized()),
            TypeOwned::Enum(item_enum) => Ok(item_enum.is_unsized()),
            // its byte length goes into the parent's FIFO, nothing inside uses it
            TypeOwned::Box(_) | TypeOwned::Series(_) => Ok(true),
            TypeOwned::Bool
            | TypeOwned::NumericAny(_)
            | TypeOwned::Flag
            | TypeOwned::String
            | TypeOwned::Vec(_)
            | TypeOwned::Array { .. }
            | TypeOwned::Tuple(_)
            | TypeOwned::Option { .. }
            | TypeOwned::Result { .. }
            | TypeOwned::Range(_)
            | TypeOwned::RangeInclusive(_)
            | TypeOwned::MaxLen { .. } => Ok(false),
        }
    }

    /// A `Vec<u8>` / `RefVec<u8>`, also with a [MaxLen](TypeOwned::MaxLen) (see [max_len](Self::max_len)).
    pub fn is_byte_slice(&self, api_bundle: &ApiBundleOwned) -> Result<bool> {
        match self {
            TypeOwned::OutOfLine { type_idx } => {
                let ty = api_bundle.get_ty(type_idx.0)?;
                ty.0.is_byte_slice(api_bundle)
            }
            // sent the same way, the bound is checked on the side
            TypeOwned::MaxLen { ty, .. } => ty.is_byte_slice(api_bundle),
            TypeOwned::Vec(inner) => Ok(matches!(
                inner.as_ref(),
                TypeOwned::NumericAny(NumericAnyTypeOwned::Base(NumericBaseType::U8))
            )),
            _ => Ok(false),
        }
    }

    /// The definition of an out-of-line type, `self` otherwise. For an [Instance](TypeOwned::Instance), the generic
    /// definition, its fields still refer to its parameters (see [crate::generics::substitute] for the type with
    /// the arguments filled in).
    pub fn get_in_line<'i>(&'i self, api_bundle: &'i ApiBundleOwned) -> Result<&'i TypeOwned> {
        if let TypeOwned::OutOfLine { type_idx } | TypeOwned::Instance { type_idx, .. } = self {
            let (ty, _) = api_bundle.get_ty(type_idx.0)?;
            Ok(ty)
        } else {
            Ok(self)
        }
    }

    /// Name of the type as written: `Vec<u8>`, `Timed<Ampere>`, `ww_si::Timed<ww_si::Ampere>` with crate names.
    pub fn human_name(&self, show_crate_name: bool, api_bundle: &ApiBundleOwned) -> Result<String> {
        self.human_name_in(show_crate_name, api_bundle, &[])
    }

    /// [Self::human_name] of a type in a generic definition with `params`, parameters named as written.
    pub fn human_name_in(
        &self,
        show_crate_name: bool,
        api_bundle: &ApiBundleOwned,
        params: &[GenericParamOwned],
    ) -> Result<String> {
        self.human_name_bounded(show_crate_name, api_bundle, params, usize::MAX)
    }

    /// [Self::human_name_in], failing once the name is longer than `max_len` bytes: names of instances of generic
    /// types repeat the names of their arguments, a few nested ones in a bundle from a device can name a type with
    /// gigabytes.
    pub fn human_name_bounded(
        &self,
        show_crate_name: bool,
        api_bundle: &ApiBundleOwned,
        params: &[GenericParamOwned],
        max_len: usize,
    ) -> Result<String> {
        let mut cx = NameCx {
            show_crate_name,
            api_bundle,
            params,
            max_len,
            out: String::new(),
        };
        cx.ty(self)?;
        Ok(cx.out)
    }

    /// `Some(max_len)` for a [MaxLen](TypeOwned::MaxLen), out-of-line references followed. An error for a bound
    /// that is a const parameter (a generic definition, not an instance of it).
    pub fn max_len(&self, api_bundle: &ApiBundleOwned) -> Result<Option<u32>> {
        match self.get_in_line(api_bundle)? {
            TypeOwned::MaxLen { max_len, .. } => Ok(Some(max_len.fixed()?)),
            _ => Ok(None),
        }
    }

    /// The type with the definitions of the user types it uses written out: `struct ww_si::Timed<T> {t: ..}`.
    /// Fails when that is longer than [HUMAN_DEFINITION_MAX_NODES] types.
    pub fn human_definition(
        &self,
        api_bundle: &ApiBundleOwned,
        single_line: bool,
    ) -> Result<String> {
        let mut cx = DefCx {
            api_bundle,
            single_line,
            budget: HUMAN_DEFINITION_MAX_NODES,
            bytes_left: HUMAN_DEFINITION_MAX_BYTES,
            open: vec![],
        };
        cx.ty(self, &[])
    }
}

struct NameCx<'a> {
    show_crate_name: bool,
    api_bundle: &'a ApiBundleOwned,
    params: &'a [GenericParamOwned],
    max_len: usize,
    out: String,
}

impl NameCx<'_> {
    fn push(&mut self, s: &str) -> Result<()> {
        if self.out.len().saturating_add(s.len()) > self.max_len {
            return Err(anyhow!("type name is longer than {} bytes", self.max_len));
        }
        self.out.push_str(s);
        Ok(())
    }

    fn crate_prefixed(&mut self, crate_idx: u32, ident: &str) -> Result<()> {
        if self.show_crate_name {
            let crate_name = self.api_bundle.crate_name(crate_idx)?;
            self.push(crate_name)?;
            self.push("::")?;
        }
        self.push(ident)
    }

    /// `prefix<a, b>`
    fn list<'t>(
        &mut self,
        open: &str,
        close: &str,
        types: impl IntoIterator<Item = &'t TypeOwned>,
    ) -> Result<()> {
        self.push(open)?;
        for (i, ty) in types.into_iter().enumerate() {
            if i > 0 {
                self.push(", ")?;
            }
            self.ty(ty)?;
        }
        self.push(close)
    }

    fn ty(&mut self, ty: &TypeOwned) -> Result<()> {
        let api_bundle = self.api_bundle;
        match ty {
            TypeOwned::Bool => self.push("bool"),
            TypeOwned::NumericAny(numeric) => self.push(&numeric.human_name()),
            TypeOwned::OutOfLine { type_idx } => {
                // named also when its definition is left out
                if let Some(TypeLocationOwned::SkippedFullVersion {
                    crate_idx,
                    type_name,
                    ..
                }) = api_bundle.types.get(type_idx.0 as usize)
                {
                    return self.crate_prefixed(crate_idx.0, type_name);
                }
                match api_bundle.get_ty(type_idx.0)?.0 {
                    TypeOwned::Struct(ItemStructOwned {
                        crate_idx, ident, ..
                    })
                    | TypeOwned::Enum(ItemEnumOwned {
                        crate_idx, ident, ..
                    }) => self.crate_prefixed(crate_idx.0, ident),
                    _ => Err(anyhow!(
                        "Bad ApiBundle: type with index {} is not a struct or an enum",
                        type_idx.0
                    )),
                }
            }
            TypeOwned::Instance { type_idx, args } => {
                match api_bundle.types.get(type_idx.0 as usize) {
                    Some(TypeLocationOwned::SkippedFullVersion {
                        crate_idx,
                        type_name,
                        ..
                    }) => self.crate_prefixed(crate_idx.0, type_name)?,
                    Some(TypeLocationOwned::InLine {
                        ty:
                            TypeOwned::Struct(ItemStructOwned {
                                crate_idx, ident, ..
                            })
                            | TypeOwned::Enum(ItemEnumOwned {
                                crate_idx, ident, ..
                            }),
                        ..
                    }) => self.crate_prefixed(crate_idx.0, ident)?,
                    _ => {
                        return Err(anyhow!(
                            "Bad ApiBundle: no generic type with index: {}",
                            type_idx.0
                        ));
                    }
                };
                self.push("<")?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.push(", ")?;
                    }
                    match arg {
                        GenericArgOwned::Type(ty) => self.ty(ty)?,
                        GenericArgOwned::Const(value) => self.push(&const_value_name(value))?,
                        GenericArgOwned::ConstParam(idx) => {
                            self.push(&param_name(self.params, idx.0))?
                        }
                    }
                }
                self.push(">")
            }
            TypeOwned::Param(idx) => self.push(&param_name(self.params, idx.0)),
            TypeOwned::Flag => self.push("flag"),
            TypeOwned::String => self.push("String"),
            TypeOwned::Vec(inner) => self.list("Vec<", ">", [inner.as_ref()]),
            TypeOwned::Series(inner) => self.list("Series<", ">", [inner.as_ref()]),
            TypeOwned::Array { len, ty } => {
                self.push("[")?;
                self.ty(ty)?;
                self.push("; ")?;
                self.push(&len.human_name(self.params))?;
                self.push("]")
            }
            TypeOwned::Tuple(types) => self.list("(", ")", types),
            TypeOwned::Struct(item_struct) => {
                self.crate_prefixed(item_struct.crate_idx.0, &item_struct.ident)
            }
            TypeOwned::Enum(item_enum) => {
                self.crate_prefixed(item_enum.crate_idx.0, &item_enum.ident)
            }
            TypeOwned::Option { some_ty } => self.list("Option<", ">", [some_ty.as_ref()]),
            TypeOwned::Result { ok_ty, err_ty } => {
                self.list("Result<", ">", [ok_ty.as_ref(), err_ty.as_ref()])
            }
            TypeOwned::Box(inner) => self.list("Box<", ">", [inner.as_ref()]),
            TypeOwned::Range(base) => self.push(&format!("Range<{}>", base.name())),
            TypeOwned::RangeInclusive(base) => {
                self.push(&format!("RangeInclusive<{}>", base.name()))
            }
            TypeOwned::MaxLen { max_len, ty } => {
                self.push("MaxLen<")?;
                self.ty(ty)?;
                self.push(", ")?;
                self.push(&max_len.human_name(self.params))?;
                self.push(">")
            }
        }
    }
}

/// Most types [TypeOwned::human_definition] writes out (an out-of-line definition used twice is written twice).
pub const HUMAN_DEFINITION_MAX_NODES: usize = 10_000;
/// Most bytes [TypeOwned::human_definition] writes, counting the text of each type also in the ones it is part of:
/// names in a bundle from a device can be as long as the bundle.
const HUMAN_DEFINITION_MAX_BYTES: usize = 16 << 20;

fn param_name(params: &[GenericParamOwned], idx: u32) -> String {
    match params.get(idx as usize) {
        Some(p) => p.ident.clone(),
        None => format!("<param {idx}>"),
    }
}

fn const_value_name(value: &crate::ValueOwned) -> String {
    match value {
        crate::ValueOwned::Bool(b) => b.to_string(),
        crate::ValueOwned::Numeric(n) => match n.to_i128() {
            Some(v) => v.to_string(),
            None => format!("{n:?}"),
        },
        other => format!("{other:?}"),
    }
}

impl Length {
    /// The number. An error for a const parameter: a generic definition, not an instance of it
    /// ([crate::generics::instantiate]).
    pub fn fixed(&self) -> Result<u32> {
        match self {
            Length::Fixed(len) => Ok(len.0),
            Length::Param(idx) => Err(anyhow!(
                "length is const parameter {} of a generic definition, not an instance of it",
                idx.0
            )),
        }
    }

    /// The number, or the name of the const parameter in `params`.
    pub fn human_name(&self, params: &[GenericParamOwned]) -> String {
        match self {
            Length::Fixed(len) => len.0.to_string(),
            Length::Param(idx) => param_name(params, idx.0),
        }
    }
}

struct DefCx<'a> {
    api_bundle: &'a ApiBundleOwned,
    single_line: bool,
    budget: usize,
    bytes_left: usize,
    /// Out-of-line definitions being written out, a self-referential one is named inside itself.
    open: Vec<u32>,
}

impl DefCx<'_> {
    fn ty(&mut self, ty: &TypeOwned, params: &[GenericParamOwned]) -> Result<String> {
        let s = self.ty_inner(ty, params)?;
        self.bytes_left = self.bytes_left.checked_sub(s.len()).ok_or_else(|| {
            anyhow!("definition is longer than {HUMAN_DEFINITION_MAX_BYTES} bytes")
        })?;
        Ok(s)
    }

    fn ty_inner(&mut self, ty: &TypeOwned, params: &[GenericParamOwned]) -> Result<String> {
        if self.budget == 0 {
            return Err(anyhow!(
                "definition is longer than {HUMAN_DEFINITION_MAX_NODES} types"
            ));
        }
        self.budget -= 1;
        let api_bundle = self.api_bundle;
        match ty {
            TypeOwned::OutOfLine { type_idx } => {
                if self.open.contains(&type_idx.0) {
                    return ty.human_name(true, api_bundle);
                }
                let def = api_bundle.get_ty(type_idx.0)?.0;
                self.open.push(type_idx.0);
                let s = self.ty(def, &[]);
                self.open.pop();
                s
            }
            TypeOwned::Instance { type_idx, args } => {
                let mut args = args.clone();
                for arg in &mut args {
                    if let GenericArgOwned::ConstParam(idx) = arg {
                        return Err(anyhow!(
                            "const parameter {} passed on outside of its definition",
                            idx.0
                        ));
                    }
                    let _ = arg;
                }
                if params.is_empty() {
                    let ty = crate::generics::substitute(api_bundle, type_idx.0, &args)?;
                    self.ty(&ty, &[])
                } else {
                    // inside a generic definition: named, not written out
                    ty.human_name_in(true, api_bundle, params)
                }
            }
            TypeOwned::Vec(inner) => Ok(format!("Vec<{}>", self.ty(inner, params)?)),
            TypeOwned::Series(inner) => Ok(format!("Series<{}>", self.ty(inner, params)?)),
            TypeOwned::Array { len, ty } => Ok(format!(
                "[{}; {}]",
                self.ty(ty, params)?,
                len.human_name(params)
            )),
            TypeOwned::Tuple(types) => {
                let mut names = Vec::with_capacity(types.len());
                for ty in types {
                    names.push(self.ty(ty, params)?);
                }
                Ok(format!("({})", names.join(", ")))
            }
            TypeOwned::Struct(item_struct) => {
                let mut s = format!(
                    "struct {}::{}{} ",
                    api_bundle.crate_name(item_struct.crate_idx.0)?,
                    item_struct.ident,
                    crate::generics::params_human_name(&item_struct.generics, api_bundle)?
                );
                s += self
                    .fields(&item_struct.fields, &item_struct.generics)?
                    .as_str();
                Ok(s)
            }
            TypeOwned::Enum(item_enum) => {
                let repr = match item_enum.repr {
                    Repr::Nibble => "nib".to_string(),
                    Repr::BitAligned(bits) => format!("ub{bits}"),
                    Repr::UNib32 => "unib32".to_string(),
                    Repr::ByteAlignedU8 => "u8".to_string(),
                    Repr::ByteAlignedU16 => "u16".to_string(),
                    Repr::ByteAlignedU32 => "u32".to_string(),
                };
                let mut s = format!(
                    "enum {repr} {}::{}{} {{",
                    api_bundle.crate_name(item_enum.crate_idx.0)?,
                    item_enum.ident,
                    crate::generics::params_human_name(&item_enum.generics, api_bundle)?
                );
                for (idx, variant) in item_enum.variants.iter().enumerate() {
                    s += &variant.ident;
                    s += self.fields(&variant.fields, &item_enum.generics)?.as_str();
                    if idx + 1 < item_enum.variants.len() {
                        s += if self.single_line { ", " } else { ",\n" };
                    }
                }
                s += "}";
                Ok(s)
            }
            TypeOwned::Option { some_ty } => Ok(format!("Option<{}>", self.ty(some_ty, params)?)),
            TypeOwned::Result { ok_ty, err_ty } => Ok(format!(
                "Result<{}, {}>",
                self.ty(ok_ty, params)?,
                self.ty(err_ty, params)?
            )),
            TypeOwned::Box(inner) => Ok(format!("Box<{}>", self.ty(inner, params)?)),
            TypeOwned::MaxLen { max_len, ty } => Ok(format!(
                "MaxLen<{}, {}>",
                self.ty(ty, params)?,
                max_len.human_name(params)
            )),
            TypeOwned::Bool
            | TypeOwned::NumericAny(_)
            | TypeOwned::Param(_)
            | TypeOwned::Flag
            | TypeOwned::String
            | TypeOwned::Range(_)
            | TypeOwned::RangeInclusive(_) => ty.human_name_in(false, api_bundle, params),
        }
    }

    fn fields(&mut self, fields: &FieldsOwned, params: &[GenericParamOwned]) -> Result<String> {
        let sep = if self.single_line { ", " } else { ",\n" };
        match fields {
            FieldsOwned::Named(named) => {
                let mut s = "{".to_string();
                for (idx, field) in named.iter().enumerate() {
                    s += field.ident.as_deref().unwrap_or("");
                    s += ": ";
                    s += &self.ty(&field.ty, params)?;
                    if idx + 1 < named.len() {
                        s += sep;
                    }
                }
                s += "}";
                Ok(s)
            }
            FieldsOwned::Unnamed(unnamed) => {
                let mut s = "(".to_string();
                for (idx, field) in unnamed.iter().enumerate() {
                    s += &self.ty(&field.ty, params)?;
                    if idx + 1 < unnamed.len() {
                        s += sep;
                    }
                }
                s += ")";
                Ok(s)
            }
            FieldsOwned::Unit => Ok("".to_string()),
        }
    }
}

impl FieldOwned {
    /// Relocated flag of the `Option` or `Result` field with the same name, see [TypeOwned::Flag].
    pub fn is_flag(&self) -> bool {
        matches!(self.ty, TypeOwned::Flag)
    }
}

impl FieldsOwned {
    fn is_lifetime(&self, api_bundle: &ApiBundleOwned, seen: &mut HashSet<u32>) -> Result<bool> {
        match self {
            FieldsOwned::Named(fields) | FieldsOwned::Unnamed(fields) => {
                for field in fields {
                    if field.ty.is_lifetime_in(api_bundle, seen)? {
                        return Ok(true);
                    }
                }
            }
            FieldsOwned::Unit => {}
        }
        Ok(false)
    }
}

/// `units::Volt` for a definition named `ident` at `path` (API-46), `Volt` at the crate root: its identity in its
/// crate, as [TypeLocationOwned::SkippedFullVersion]'s `type_name` has it.
pub fn qualified_name(path: Option<&[String]>, ident: &str) -> String {
    let mut name: String = path
        .unwrap_or_default()
        .iter()
        .map(|m| format!("{m}::"))
        .collect();
    name.push_str(ident);
    name
}

impl ItemStructOwned {
    /// `units::Volt`: [qualified_name] of this definition.
    pub fn qualified_name(&self) -> String {
        qualified_name(self.path.as_deref(), &self.ident)
    }

    /// Returns true if this type contains a string, vector, or box at any depth.
    pub fn is_lifetime(&self, api_bundle: &ApiBundleOwned) -> Result<bool> {
        self.fields.is_lifetime(api_bundle, &mut HashSet::new())
    }

    pub fn is_unsized(&self) -> bool {
        self.size == ElementSize::Unsized
    }
}

impl ItemEnumOwned {
    /// `units::Volt`: [qualified_name] of this definition.
    pub fn qualified_name(&self) -> String {
        qualified_name(self.path.as_deref(), &self.ident)
    }

    /// Returns true if this type contains a string, vector, or box at any depth.
    pub fn is_lifetime(&self, api_bundle: &ApiBundleOwned) -> Result<bool> {
        self.is_lifetime_in(api_bundle, &mut HashSet::new())
    }

    fn is_lifetime_in(&self, api_bundle: &ApiBundleOwned, seen: &mut HashSet<u32>) -> Result<bool> {
        for variant in &self.variants {
            if variant.fields.is_lifetime(api_bundle, seen)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn is_unsized(&self) -> bool {
        self.size == ElementSize::Unsized
    }
}

impl ItemEnumOwned {
    pub fn discriminant(&self, variant_name: &str) -> Result<u32> {
        self.variants
            .iter()
            .find_map(|variant| {
                if variant.ident == variant_name {
                    Some(variant.discriminant.0)
                } else {
                    None
                }
            })
            .ok_or(anyhow!(
                "Enum {} does not have variant {}",
                self.ident,
                variant_name
            ))
    }
}
