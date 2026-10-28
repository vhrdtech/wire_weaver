//! Compatibility of one resource or type with another version of it: what one side writes, can the other read?
//!
//! Shared by the evolution checker (an old and a new version of a crate) and `wire_weaver_client` (the client's API and
//! the connected device's). Types are compared structurally, following
//! [evolution rules](https://github.com/vhrdtech/wire_weaver/blob/master/docs/evolution/rules.md): names don't matter,
//! positions do (but a variant found under another discriminant on the other side moved). Direction matters as well:
//! an `Unsized` struct written by one side can have more trailing fields than the reader knows about (they are
//! skipped), but a reader's extra fields must have `#[default]`, and a variant unknown to the reader needs the
//! reader's `#[future_version]` catch-all.

use std::borrow::Cow;
use std::cell::OnceCell;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::fmt;

use exact_units::{DimOwned, Magnitude};
use shrink_wrap::ElementSize;
use ww_self::signature;
use ww_self::units::{self, CanonicalUnit};
use ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ArgumentOwned, Delivery,
    FieldOwned, FieldsOwned, FutureVersionOwned, GenericArgOwned, GenericParamKindOwned,
    GenericParamOwned, InstanceOfOwned, ItemEnumOwned, ItemStructOwned, Length, MarkerDefOwned,
    OriginDefOwned, PropertyAccess, TypeLocationOwned, TypeOwned, ValueOwned, VariantOwned,
};
use ww_version::FullVersionOwned;

use crate::Rule;
use crate::layout;
pub type Side<'a> = (&'a ApiBundleOwned, &'a ApiItemOwned, &'static str);

/// Checks that the resource on the client side can be used with the one on the device side.
/// How [TypeCheck] treats a reader whose [MaxLen](TypeOwned::MaxLen) is smaller than the writer's: values the
/// writer can produce are then rejected by the reader (with an error, the bytes are the same).
#[derive(Default)]
pub struct Bounds<'a> {
    /// A smaller max length on the reader is an error when this bundle is the writer (the old version of a crate:
    /// lowering a bound breaks data the old version writes).
    pub strict_writer: Option<&'a ApiBundleOwned>,
    /// The smaller max lengths that were not an error.
    pub narrower: Vec<String>,
    /// A smaller max length was an error (see `strict_writer`).
    pub lowered: bool,
    /// The rule an error broke when it is a marker or a bound, see [TypeCheck::rule].
    pub rule: Option<Rule>,
    /// Transparent wrappers on one side only, see [TypeCheck::unwrapped].
    pub unwrapped: Vec<Unwrapped>,
}

/// A transparent wrapper (DER-18, `ww_si::Fixed`) on one side and the type it wraps on the other: the same bytes,
/// compatible.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unwrapped {
    /// Arguments, fields and variants leading to it (`` argument `shared` ``), empty for the type itself.
    pub path: Vec<String>,
    pub writer_name: &'static str,
    /// The type as the writer has it, and as the reader has it.
    pub written: String,
    pub read: String,
    /// What makes them compatible: `same bytes` for a wrapper and its field, `same value` for two markers that mean
    /// the same (`prefix::Milli` -> `prefix::Pow10<-3>`, EVO-17).
    pub same: &'static str,
}

impl Unwrapped {
    /// `` field `shared`: u64 -> Fixed<Second, Micro, u64, Unbounded>, same bytes ``, from the `from` side's type to
    /// the other's.
    pub fn describe(&self, from: &str) -> String {
        let (from, to) = if self.writer_name == from {
            (&self.written, &self.read)
        } else {
            (&self.read, &self.written)
        };
        let mut out = String::new();
        for segment in &self.path {
            out.push_str(segment);
            out.push_str(": ");
        }
        out.push_str(&format!("{from} -> {to}, {}", self.same));
        out
    }
}

pub fn compare_items<'a>(
    (client, client_item, client_name): Side<'a>,
    (device, device_item, device_name): Side<'a>,
    bounds: &mut Bounds<'a>,
) -> Result<(), String> {
    let array = |item: &ApiItemOwned| {
        if item.is_array() {
            "an array"
        } else {
            "not an array"
        }
    };
    if client_item.is_array() != device_item.is_array() {
        return Err(format!(
            "{} on the {client_name}, {} on the {device_name}",
            array(client_item),
            array(device_item)
        ));
    }
    // types flowing from client to device, and from device to client
    let strict = |writer| {
        bounds
            .strict_writer
            .is_some_and(|strict| std::ptr::eq(strict, writer))
    };
    let mut to_device =
        TypeCheck::new(client, device, client_name, device_name).strict_bounds(strict(client));
    let mut to_client =
        TypeCheck::new(device, client, device_name, client_name).strict_bounds(strict(device));
    let result = compare_kinds(
        (client, client_item, client_name),
        (device, device_item, device_name),
        (&mut to_device, &mut to_client),
    );
    bounds.narrower.append(&mut to_device.narrower);
    bounds.narrower.append(&mut to_client.narrower);
    bounds.lowered |= to_device.lowered || to_client.lowered;
    bounds.rule = bounds.rule.or(to_device.rule).or(to_client.rule);
    bounds.unwrapped.append(&mut to_device.unwrapped);
    bounds.unwrapped.append(&mut to_client.unwrapped);
    result
}

fn compare_kinds<'a>(
    (client, client_item, client_name): Side<'a>,
    (device, device_item, device_name): Side<'a>,
    (to_device, to_client): (&mut TypeCheck<'a>, &mut TypeCheck<'a>),
) -> Result<(), String> {
    use ApiItemKindOwned::*;
    match (&client_item.kind, &device_item.kind) {
        (
            // error lists don't matter here: nothing on the wire, only the client checks them (API-32)
            Method {
                args, return_ty, ..
            },
            Method {
                args: device_args,
                return_ty: device_return_ty,
                ..
            },
        ) => check_signature(
            (args, return_ty.as_ref()),
            (device_args, device_return_ty.as_ref()),
            (client_name, device_name),
            (to_device, to_client),
        )?,
        (
            // error lists don't matter, as for methods
            Property {
                ty,
                access,
                write_err_ty,
                batched,
                ..
            },
            Property {
                ty: device_ty,
                access: device_access,
                write_err_ty: device_write_err_ty,
                batched: device_batched,
                ..
            },
        ) => {
            let (readable, writable, observable) = capabilities(access);
            let (device_readable, device_writable, device_observable) = capabilities(device_access);
            if (readable && !device_readable)
                || (writable && !device_writable)
                || (observable && !device_observable)
                // a `+batched` property's observe slot is a `Batched<T>` (API-39): both sides must agree, on
                // the form too
                || (observable && batched != device_batched)
            {
                return Err(format!(
                    "{access:?} {batched:?} on the {client_name}, {device_access:?} {device_batched:?} on the \
                     {device_name}"
                ));
            }
            if readable {
                to_client.check(device_ty, ty)?;
            }
            if writable {
                to_device.check(ty, device_ty)?;
                match (write_err_ty, device_write_err_ty) {
                    (None, None) => {}
                    (Some(ty), Some(device_ty)) => to_client
                        .check(device_ty, ty)
                        .map_err(|e| format!("write error type: {e}"))?,
                    _ => return Err("write error type is only defined on one side".into()),
                }
            }
        }
        (
            Stream { ty, is_up },
            Stream {
                ty: device_ty,
                is_up: device_is_up,
            },
        ) => {
            if is_up != device_is_up {
                return Err("stream on one side, sink on the other".into());
            }
            if *is_up {
                to_client.check(device_ty, ty)?;
            } else {
                to_device.check(ty, device_ty)?;
            }
        }
        (
            FlowSink { ty, flow },
            FlowSink {
                ty: device_ty,
                flow: device_flow,
            },
        ) => {
            if flow != device_flow {
                return Err(format!(
                    "{flow:?} flow control on the {client_name}, {device_flow:?} on the {device_name}"
                ));
            }
            to_device.check(ty, device_ty)?;
        }
        (
            // error lists don't matter, as for methods
            StreamFn {
                ty,
                delivery,
                args,
                return_ty,
                ..
            },
            StreamFn {
                ty: device_ty,
                delivery: device_delivery,
                args: device_args,
                return_ty: device_return_ty,
                ..
            },
        ) => {
            if delivery != device_delivery {
                return Err(format!(
                    "{delivery:?} stream on the {client_name}, {device_delivery:?} on the {device_name}"
                ));
            }
            to_client.check(device_ty, ty)?;
            check_signature(
                (args, return_ty.as_ref()),
                (device_args, device_return_ty.as_ref()),
                (client_name, device_name),
                (to_device, to_client),
            )?;
        }
        (
            Trait { trait_idx, .. },
            Trait {
                trait_idx: device_trait_idx,
                ..
            },
        ) => {
            let (Some(identity), Some(device_identity)) = (
                trait_identity(client, trait_idx.0),
                trait_identity(device, device_trait_idx.0),
            ) else {
                // not enough information (e.g., compact version only), items are still compared one by one
                return Ok(());
            };
            check_same_origin("trait", &identity, &device_identity)?;
            // when both are in-line, their items are compared one by one instead
            let skipped = |bundle: &ApiBundleOwned, idx: u32| {
                !matches!(
                    bundle.traits.get(idx as usize),
                    Some(ApiLevelLocationOwned::InLine { .. })
                )
            };
            if skipped(client, trait_idx.0) || skipped(device, device_trait_idx.0) {
                check_same_definition(
                    "trait",
                    (&identity, client_name),
                    (&device_identity, device_name),
                    trait_signature(client, trait_idx.0),
                    trait_signature(device, device_trait_idx.0),
                )?;
            }
        }
        (kind, device_kind) => {
            return Err(format!(
                "{} on the {client_name}, {} on the {device_name}",
                kind_name(kind),
                kind_name(device_kind)
            ));
        }
    }
    Ok(())
}

/// (readable, writable, observable)
fn capabilities(access: &PropertyAccess) -> (bool, bool, bool) {
    match access {
        PropertyAccess::Const => (true, false, false),
        PropertyAccess::ReadOnly { observe, .. } => (true, false, *observe),
        PropertyAccess::ReadWrite { observe, .. } => (true, true, *observe),
        PropertyAccess::WriteOnly => (false, true, false),
    }
}

fn kind_name(kind: &ApiItemKindOwned) -> &'static str {
    match kind {
        ApiItemKindOwned::Method { .. } => "a method",
        ApiItemKindOwned::Property { .. } => "a property",
        ApiItemKindOwned::Stream { is_up: true, .. } => "a stream",
        ApiItemKindOwned::Stream { is_up: false, .. } => "a sink",
        ApiItemKindOwned::FlowSink { .. } => "a sink with flow control",
        ApiItemKindOwned::StreamFn {
            delivery: Delivery::Plain,
            ..
        } => "a stream function",
        ApiItemKindOwned::StreamFn {
            delivery: Delivery::Counted,
            ..
        } => "a counted stream",
        ApiItemKindOwned::StreamFn {
            delivery: Delivery::Reliable,
            ..
        } => "a reliable stream",
        ApiItemKindOwned::Trait { .. } => "a trait",
    }
}

/// Arguments go to the device, the result comes back: of a method or a stream function.
fn check_signature<'a>(
    (args, return_ty): (&'a [ArgumentOwned], Option<&'a TypeOwned>),
    (device_args, device_return_ty): (&'a [ArgumentOwned], Option<&'a TypeOwned>),
    (client_name, device_name): (&str, &str),
    (to_device, to_client): (&mut TypeCheck<'a>, &mut TypeCheck<'a>),
) -> Result<(), String> {
    if args.len() != device_args.len() {
        return Err(format!(
            "takes {} argument(s) on the {client_name}, {} on the {device_name}",
            args.len(),
            device_args.len()
        ));
    }
    for (arg, device_arg) in args.iter().zip(device_args) {
        let segment = format!("argument `{}`", arg.ident);
        to_device
            .within(&segment, |c| c.check(&arg.ty, &device_arg.ty))
            .map_err(|e| format!("{segment}: {e}"))?;
    }
    match (return_ty, device_return_ty) {
        (None, None) => {}
        (Some(ty), Some(device_ty)) => to_client
            .within("return type", |c| c.check(device_ty, ty))
            .map_err(|e| format!("return type: {e}"))?,
        (Some(_), None) => return Err(format!("{device_name} returns nothing")),
        (None, Some(_)) => {
            return Err(format!(
                "{device_name} returns a value, {client_name} expects none"
            ));
        }
    }
    Ok(())
}

/// Crate and name of a trait or type, and its version. A type's name has its module path (`units::Volt`, API-46), as
/// a skipped one is named.
type Identity<'a> = (&'a FullVersionOwned, Cow<'a, str>);

fn trait_identity(bundle: &ApiBundleOwned, trait_idx: u32) -> Option<Identity<'_>> {
    let (crate_idx, name) = match bundle.traits.get(trait_idx as usize)? {
        ApiLevelLocationOwned::InLine { level, .. } => (level.crate_idx, level.trait_name.as_str()),
        ApiLevelLocationOwned::SkippedFullVersion {
            crate_idx,
            trait_name,
            ..
        } => (*crate_idx, trait_name.as_str()),
        ApiLevelLocationOwned::SkippedCompactVersion { .. } => return None,
    };
    Some((bundle.crate_version(crate_idx.0).ok()?, Cow::Borrowed(name)))
}

fn check_same_origin(what: &str, a: &Identity, b: &Identity) -> Result<(), String> {
    if a.0.crate_id != b.0.crate_id || a.1 != b.1 {
        return Err(format!(
            "{what} {}::{} vs {}::{}",
            a.0.crate_id, a.1, b.0.crate_id, b.1
        ));
    }
    if !a.0.is_protocol_compatible(b.0) {
        return Err(format!(
            "{what} {} from incompatible crate versions: {:?} vs {:?}",
            a.1, a.0, b.0
        ));
    }
    Ok(())
}

/// Catches a definition that is different on each side under the same crate version, when one side has left it out
/// (with both in-line, they are compared by the evolution rules instead). Such a definition was not found by its
/// signature in the snapshots or in the definition cache. Only possible to check when both signatures are known.
fn check_same_definition(
    what: &str,
    (a, a_name): (&Identity, &str),
    (b, b_name): (&Identity, &str),
    a_signature: Option<Vec<u8>>,
    b_signature: Option<Vec<u8>>,
) -> Result<(), String> {
    let (Some(a_signature), Some(b_signature)) = (a_signature, b_signature) else {
        return Ok(());
    };
    if a.0 == b.0 && a_signature != b_signature {
        let v = &a.0.version;
        let cache = match ww_self::definition_cache::default_dir() {
            Some(dir) => dir.display().to_string(),
            None => "~/.wire_weaver/definitions".into(),
        };
        return Err(format!(
            "{what} {crate_id}::{name} {major}.{minor}.{patch} has signature {} on the {a_name} and {} on the \
             {b_name}, and the other definition is not known here (not in a snapshot, not in the definition cache \
             {cache}). Likely the two were built from different revisions of {crate_id}, a crate version that is not \
             published and changed without a version bump. Rebuild (reflash) both from one revision, or build the \
             other side once on this machine, so that codegen puts its definitions into {cache}",
            hex(&a_signature),
            hex(&b_signature),
            crate_id = a.0.crate_id,
            name = a.1,
            major = v.major.0,
            minor = v.minor.0,
            patch = v.patch.0,
        ));
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Signature of a trait: stored for a skipped one, calculated for an in-line one (definitions it refers to that are
/// skipped are looked up in embedded snapshots).
fn trait_signature(bundle: &ApiBundleOwned, trait_idx: u32) -> Option<Vec<u8>> {
    match bundle.traits.get(trait_idx as usize)? {
        ApiLevelLocationOwned::InLine { .. } => {
            signature::trait_signature(bundle, trait_idx, &|v| wire_weaver_snapshots::get(v)).ok()
        }
        ApiLevelLocationOwned::SkippedFullVersion { signature, .. }
        | ApiLevelLocationOwned::SkippedCompactVersion { signature, .. } => {
            (!signature.is_empty()).then(|| signature.clone())
        }
    }
}

/// Signature of a type: stored for a skipped one, calculated for an in-line one (definitions it refers to that are
/// skipped are looked up in embedded snapshots).
fn type_signature(bundle: &ApiBundleOwned, type_idx: u32) -> Option<Vec<u8>> {
    match bundle.types.get(type_idx as usize)? {
        TypeLocationOwned::InLine { .. } => {
            signature::type_signature(bundle, type_idx, &|v| wire_weaver_snapshots::get(v)).ok()
        }
        TypeLocationOwned::SkippedFullVersion { signature, .. } => {
            (!signature.is_empty()).then(|| signature.clone())
        }
    }
}

/// Deepest nesting of [TypeCheck::check]: types nested [MAX_DEPTH](ww_self::generics::MAX_DEPTH) levels on both
/// sides, a self-referential one written out a few levels further on one side.
const MAX_CHECK_DEPTH: usize = 4 * ww_self::generics::MAX_DEPTH;

/// Checks that a value written as one type can be read as the other.
pub struct TypeCheck<'a> {
    writer: &'a ApiBundleOwned,
    reader: &'a ApiBundleOwned,
    writer_name: &'static str,
    reader_name: &'static str,
    /// Struct and enum definitions (by address, in-line ones too) already compared or being compared, to stop on
    /// self-referential types: also when one side writes them out a level further than the other, as a bundle from
    /// a device can.
    seen: HashSet<(*const TypeOwned, *const TypeOwned)>,
    /// Nesting depth of [Self::check] calls, bounded by [MAX_CHECK_DEPTH].
    depth: usize,
    /// A smaller max length on the reader is an error, see [Bounds].
    strict_bounds: bool,
    /// Smaller max lengths on the reader found when they are not an error.
    pub narrower: Vec<String>,
    /// A smaller max length on the reader was an error.
    pub lowered: bool,
    /// The rule a failed check broke, when it is not the general one of a resource or a type: a marker argument
    /// ([Rule::Meaning]) or a bound ([Rule::Bound]) changed.
    pub rule: Option<Rule>,
    /// Transparent wrappers on one side only, with their plain field on the other: compatible.
    pub unwrapped: Vec<Unwrapped>,
    /// Arguments, fields and variants leading to the type being checked, for [Self::unwrapped].
    path: Vec<String>,
    /// The writer's and the reader's bundle with skipped definitions put back from the embedded snapshots, made the
    /// first time a marker is compared by its definition (EVO-17). Indices of the bundle stay valid in it.
    defs: [OnceCell<ApiBundleOwned>; 2],
}

/// Type with out-of-line references resolved.
enum ResolvedTy<'a> {
    Ty(&'a TypeOwned),
    /// Type definition is not in the bundle, only where it comes from.
    Skipped(Identity<'a>),
}

impl<'a> TypeCheck<'a> {
    pub fn new(
        writer: &'a ApiBundleOwned,
        reader: &'a ApiBundleOwned,
        writer_name: &'static str,
        reader_name: &'static str,
    ) -> Self {
        TypeCheck {
            writer,
            reader,
            writer_name,
            reader_name,
            seen: HashSet::new(),
            depth: 0,
            strict_bounds: false,
            narrower: vec![],
            lowered: false,
            rule: None,
            unwrapped: vec![],
            path: vec![],
            defs: Default::default(),
        }
    }

    /// Runs `f` with `segment` (a field, an argument) added to [Self::path].
    fn within<R>(&mut self, segment: &str, f: impl FnOnce(&mut Self) -> R) -> R {
        self.path.push(segment.to_string());
        let result = f(self);
        self.path.pop();
        result
    }

    pub fn strict_bounds(mut self, strict: bool) -> Self {
        self.strict_bounds = strict;
        self
    }

    /// Max lengths of a list or string on both sides, `None` for none.
    fn check_bound(&mut self, w: Option<u32>, r: Option<u32>) -> Result<(), String> {
        let narrower = match (w, r) {
            (_, None) => false,
            (None, Some(_)) => true,
            (Some(w), Some(r)) => r < w,
        };
        if !narrower {
            return Ok(());
        }
        let show = |len: Option<u32>| match len {
            Some(len) => format!("max length {len}"),
            None => "no max length".to_string(),
        };
        let msg = format!(
            "{} on the {}, {} on the {}",
            show(w),
            self.writer_name,
            show(r),
            self.reader_name
        );
        if self.strict_bounds {
            self.lowered = true;
            return Err(format!(
                "{msg}: longer values the {} writes are rejected by the {}",
                self.writer_name, self.reader_name
            ));
        }
        self.narrower.push(msg);
        Ok(())
    }

    pub fn check(&mut self, w: &TypeOwned, r: &TypeOwned) -> Result<(), String> {
        if self.depth >= MAX_CHECK_DEPTH {
            return Err(format!(
                "types are nested deeper than {MAX_CHECK_DEPTH} levels, can't compare them"
            ));
        }
        self.depth += 1;
        let result = self.check_inner(w, r);
        self.depth -= 1;
        result
    }

    fn check_inner(&mut self, w: &TypeOwned, r: &TypeOwned) -> Result<(), String> {
        if let Some(result) = self.check_wrappers(w, r) {
            return result;
        }
        // instances are left after ww_self::generics::prepare only when their definition is not known
        match (w, r) {
            (
                TypeOwned::Instance {
                    type_idx: wi,
                    args: wa,
                },
                TypeOwned::Instance {
                    type_idx: ri,
                    args: ra,
                },
            ) => return self.check_instances((wi.0, wa), (ri.0, ra)),
            (TypeOwned::Instance { .. }, _) | (_, TypeOwned::Instance { .. }) => {
                return Err(format!(
                    "{} on the {}, {} on the {}: a generic type definition is not available on one side",
                    describe(w),
                    self.writer_name,
                    describe(r),
                    self.reader_name
                ));
            }
            _ => {}
        }
        let (w_ty, r_ty) = (w, r);
        let (w, r) = match (resolve(self.writer, w)?, resolve(self.reader, r)?) {
            (ResolvedTy::Ty(w), ResolvedTy::Ty(r)) => (w, r),
            (w, r) => {
                let identity = |bundle, resolved| match resolved {
                    ResolvedTy::Skipped(identity) => Some(identity),
                    ResolvedTy::Ty(ty) => user_type_identity(bundle, ty),
                };
                let (Some(w), Some(r)) = (identity(self.writer, w), identity(self.reader, r))
                else {
                    return Err("type definition is not available on one side".into());
                };
                check_same_origin("type", &w, &r)?;
                let signature = |bundle, ty: &TypeOwned| match ty {
                    TypeOwned::OutOfLine { type_idx } => type_signature(bundle, type_idx.0),
                    _ => None,
                };
                return check_same_definition(
                    "type",
                    (&w, self.writer_name),
                    (&r, self.reader_name),
                    signature(self.writer, w_ty),
                    signature(self.reader, r_ty),
                );
            }
        };
        if matches!(w, TypeOwned::Struct(_) | TypeOwned::Enum(_))
            && matches!(r, TypeOwned::Struct(_) | TypeOwned::Enum(_))
            && !self.seen.insert((w as *const _, r as *const _))
        {
            return Ok(());
        }
        use TypeOwned::*;
        match (w, r) {
            // parameters of generic definitions, compared by position (the names don't matter)
            (Param(w_idx), Param(r_idx)) if w_idx == r_idx => Ok(()),
            (
                MaxLen {
                    max_len: Length::Param(w_idx),
                    ty: w,
                },
                MaxLen {
                    max_len: Length::Param(r_idx),
                    ty: r,
                },
            ) if w_idx == r_idx => self.check(w, r),
            (
                MaxLen { max_len, ty: w },
                MaxLen {
                    max_len: r_max_len,
                    ty: r,
                },
            ) => {
                self.check_bound(Some(fixed(max_len)?), Some(fixed(r_max_len)?))?;
                self.check(w, r)
            }
            (MaxLen { max_len, ty: w }, r) => {
                self.check_bound(Some(fixed(max_len)?), None)?;
                self.check(w, r)
            }
            (w, MaxLen { max_len, ty: r }) => {
                self.check_bound(None, Some(fixed(max_len)?))?;
                self.check(w, r)
            }
            // a Series of T evolves as T does (SW-60)
            // TODO(CLT-29): a field's #[series(opaque)] changed is other bytes inside a Series only
            (Vec(w), Vec(r)) | (Box(w), Box(r)) | (Series(w), Series(r)) => self.check(w, r),
            (Option { some_ty: w }, Option { some_ty: r }) => self.check(w, r),
            (Array { len, ty: w }, Array { len: r_len, ty: r }) => {
                if len != r_len {
                    return Err(format!(
                        "[_; {}] vs [_; {}]",
                        len.human_name(&[]),
                        r_len.human_name(&[])
                    ));
                }
                self.check(w, r)
            }
            (
                Result { ok_ty, err_ty },
                Result {
                    ok_ty: r_ok_ty,
                    err_ty: r_err_ty,
                },
            ) => {
                self.check(ok_ty, r_ok_ty)?;
                self.check(err_ty, r_err_ty)
            }
            (Tuple(w), Tuple(r)) => {
                if w.len() != r.len() {
                    return Err(format!("tuple of {} vs tuple of {}", w.len(), r.len()));
                }
                for (i, (w, r)) in w.iter().zip(r).enumerate() {
                    self.within(&format!(".{i}"), |c| c.check(w, r))
                        .map_err(|e| format!(".{i}: {e}"))?;
                }
                Ok(())
            }
            (Struct(w), Struct(r)) => self
                .check_struct(w, r)
                .map_err(|e| format!("struct {}: {e}", r.ident)),
            (Enum(w), Enum(r)) => self
                .check_enum(w, r)
                .map_err(|e| format!("enum {}: {e}", r.ident)),
            (Bool, Bool) | (Flag, Flag) | (String, String) => Ok(()),
            (NumericAny(_), NumericAny(_))
            | (Range(_), Range(_))
            | (RangeInclusive(_), RangeInclusive(_))
                if w == r =>
            {
                Ok(())
            }
            _ => Err(format!(
                "{} on the {}, {} on the {}",
                describe(w),
                self.writer_name,
                describe(r),
                self.reader_name
            )),
        }
    }

    /// Instances of generic definitions that are not known: the same definition, the same arguments.
    fn check_instances(
        &mut self,
        (wi, wa): (u32, &[GenericArgOwned]),
        (ri, ra): (u32, &[GenericArgOwned]),
    ) -> Result<(), String> {
        let identity = |bundle: &'a ApiBundleOwned, idx: u32| match bundle.types.get(idx as usize) {
            Some(TypeLocationOwned::InLine { ty, .. }) => user_type_identity(bundle, ty),
            Some(TypeLocationOwned::SkippedFullVersion {
                crate_idx,
                type_name,
                ..
            }) => Some((
                bundle.crate_version(crate_idx.0).ok()?,
                Cow::Borrowed(type_name.as_str()),
            )),
            None => None,
        };
        let (Some(w), Some(r)) = (identity(self.writer, wi), identity(self.reader, ri)) else {
            return Err("bad API bundle: no generic type definition".into());
        };
        check_same_origin("type", &w, &r)?;
        check_same_definition(
            "type",
            (&w, self.writer_name),
            (&r, self.reader_name),
            type_signature(self.writer, wi),
            type_signature(self.reader, ri),
        )?;
        if wa.len() != ra.len() {
            return Err(format!(
                "{} with {} arguments vs {}",
                w.1,
                wa.len(),
                ra.len()
            ));
        }
        let markers = type_def(self.writer, wi)
            .map(marker_params)
            .unwrap_or_default();
        self.check_markers(&w.1, &markers, wa, ra)?;
        for (i, (w_arg, r_arg)) in wa.iter().zip(ra).enumerate() {
            if markers.iter().any(|(m, _)| *m == i) {
                continue;
            }
            match (w_arg, r_arg) {
                (GenericArgOwned::Type(w_ty), GenericArgOwned::Type(r_ty)) => self
                    .check(w_ty, r_ty)
                    .map_err(|e| format!("argument {i} of {}: {e}", w.1))?,
                (GenericArgOwned::Const(w_value), GenericArgOwned::Const(r_value))
                    if w_value == r_value => {}
                _ => {
                    return Err(format!("argument {i} of {}: {w_arg:?} vs {r_arg:?}", w.1));
                }
            }
        }
        Ok(())
    }

    fn check_struct(&mut self, w: &ItemStructOwned, r: &ItemStructOwned) -> Result<(), String> {
        check_generics(&w.generics, &r.generics)?;
        self.check_marker_defs(w, r)?;
        if let (Some(wi), Some(ri)) = (&w.instance_of, &r.instance_of) {
            self.check_instance_markers(wi, ri)?;
        }
        self.check_fields(&w.size, &w.fields, &r.size, &r.fields)
    }

    /// Two versions of a marker struct (INT-13): a unit, scale or origin redefined under the same name means
    /// something else (EVO-17); its symbol may change.
    fn check_marker_defs(
        &mut self,
        w: &ItemStructOwned,
        r: &ItemStructOwned,
    ) -> Result<(), String> {
        use MarkerDefOwned::*;
        let (Some(w_def), Some(r_def)) = (w.marker(), r.marker()) else {
            return Ok(());
        };
        let step = |dim: &DimOwned, magnitude| Step {
            dim: dim.clone(),
            magnitude,
        };
        let one = || exact_units::DimOwned::from(exact_units::Dim::ONE);
        let what = |kind: &str| format!("{kind} {} redefined", r.ident);
        match (w_def, r_def) {
            (Unit(w_unit), Unit(r_unit)) => self.check_steps(
                &what("unit"),
                &step(&w_unit.dim, w_unit.magnitude),
                &step(&r_unit.dim, r_unit.magnitude),
            ),
            (Scale(w_scale), Scale(r_scale)) => self.check_steps(
                &what("scale"),
                &step(&one(), w_scale.magnitude),
                &step(&one(), r_scale.magnitude),
            ),
            (Origin(w_origin), Origin(r_origin)) => {
                self.check_origin(&w.ident, w_origin, &r.ident, r_origin)
            }
            (w_def, r_def) if std::mem::discriminant(w_def) == std::mem::discriminant(r_def) => {
                Ok(())
            }
            _ => {
                self.rule = Some(Rule::Meaning);
                Err(format!(
                    "{} is another kind of marker on the {} than on the {}",
                    r.ident, self.writer_name, self.reader_name
                ))
            }
        }
    }

    /// Marker arguments of two instances of the same generic definition (after [ww_self::generics::prepare]), whose
    /// fields are compared as those of any struct.
    fn check_instance_markers(
        &mut self,
        wi: &InstanceOfOwned,
        ri: &InstanceOfOwned,
    ) -> Result<(), String> {
        let (Some(w_def), Some(r_def)) = (
            def_name(self.writer, wi.type_idx.0),
            def_name(self.reader, ri.type_idx.0),
        ) else {
            return Ok(());
        };
        if w_def != r_def || wi.args.len() != ri.args.len() {
            return Ok(());
        }
        let Some(def) = type_def(self.writer, wi.type_idx.0) else {
            return Ok(());
        };
        self.check_markers(&w_def.1, &marker_params(def), &wi.args, &ri.args)
    }

    /// The marker arguments `markers` (positions and parameter names) of two instances of the generic definition
    /// `of`. Markers with a definition (INT-13: units, scales, origins) are compared by what they mean (EVO-17), the
    /// units and scales of one instance together, as the value of one step of its number: `Fixed<Volt, Milli, ..>`
    /// and `Fixed<Volt, Pow10<-3>, ..>` are the same. Any other marker is compared by name, a bound by value.
    fn check_markers(
        &mut self,
        of: &str,
        markers: &[(usize, &str)],
        wa: &[GenericArgOwned],
        ra: &[GenericArgOwned],
    ) -> Result<(), String> {
        let mut by_meaning = vec![];
        for &(i, param) in markers {
            let (Some(w_arg), Some(r_arg)) = (wa.get(i), ra.get(i)) else {
                continue;
            };
            if let (GenericArgOwned::Type(w), GenericArgOwned::Type(r)) = (w_arg, r_arg)
                && let (Some(w_meaning), Some(r_meaning)) = (self.meaning(0, w), self.meaning(1, r))
            {
                let names = (
                    marker_name(self.writer, w, false),
                    marker_name(self.reader, r, false),
                );
                let renamed =
                    marker_name(self.writer, w, true) != marker_name(self.reader, r, true);
                by_meaning.push((param, names, renamed, w_meaning, r_meaning));
                continue;
            }
            self.check_marker(w_arg, r_arg)
                .map_err(|e| format!("{param} of {of}: {e}"))?;
        }
        if by_meaning.is_empty() {
            return Ok(());
        }
        let (mut w_step, mut r_step) = (Step::one(), Step::one());
        // the markers that changed by name, or else those whose definition changed under the same name
        let (mut steps, mut redefined) = (vec![], vec![]);
        for (param, (w_name, r_name), renamed, w_meaning, r_meaning) in &by_meaning {
            let at = |e: String| format!("{param} of {of}: {e}");
            match (w_meaning, r_meaning) {
                (Meaning::Origin(w), Meaning::Origin(r)) => {
                    self.check_origin(w_name, w, r_name, r).map_err(at)?
                }
                (Meaning::Origin(_), _) | (_, Meaning::Origin(_)) => {
                    self.rule = Some(Rule::Meaning);
                    return Err(at(format!(
                        "marker {w_name} on the {}, {r_name} on the {}: an origin on one side only",
                        self.writer_name, self.reader_name
                    )));
                }
                (w, r) => {
                    w_step.mul(w).map_err(at)?;
                    r_step.mul(r).map_err(at)?;
                    let (mut w_alone, mut r_alone) = (Step::one(), Step::one());
                    w_alone.mul(w).map_err(at)?;
                    r_alone.mul(r).map_err(at)?;
                    if !w_alone.same(&r_alone) {
                        redefined.push(format!("{param} of {of}: {r_name} redefined"));
                    }
                    if *renamed {
                        steps.push(format!(
                            "{param} of {of}: marker {w_name} on the {}, {r_name} on the {}",
                            self.writer_name, self.reader_name
                        ));
                    }
                }
            }
        }
        if steps.is_empty() {
            steps = redefined;
        }
        if !steps.is_empty() {
            self.check_steps(&steps.join(", "), &w_step, &r_step)?;
        }
        for (param, (w_name, r_name), renamed, ..) in by_meaning {
            if renamed {
                let mut path = self.path.clone();
                path.push(format!("{param} of {of}"));
                self.note_compatible(Unwrapped {
                    path,
                    writer_name: self.writer_name,
                    written: w_name,
                    read: r_name,
                    same: "same value",
                });
            }
        }
        Ok(())
    }

    /// A step of the number as the writer and as the reader have it: the same dimension and the same exact factor,
    /// or breaking. A different factor is the Mars Climate Orbiter's mistake: lbf·s read as N·s.
    fn check_steps(&mut self, changed: &str, w: &Step, r: &Step) -> Result<(), String> {
        if !w.dim.as_dim().same(r.dim.as_dim()) {
            self.rule = Some(Rule::Meaning);
            return Err(format!(
                "{changed}: {} vs {}, another dimension: the {} would read them as another quantity",
                w.dim.as_dim(),
                r.dim.as_dim(),
                self.reader_name
            ));
        }
        if w.magnitude == r.magnitude {
            return Ok(());
        }
        self.rule = Some(Rule::Meaning);
        let (w_name, r_name) = (self.writer_name, self.reader_name);
        let shown = match w.magnitude.checked_div(r.magnitude) {
            Ok(f) => format!("{} (exactly 1{f})", round6(f.to_f64())),
            Err(_) => "out of range".into(),
        };
        Err(format!(
            "{changed}: the same bytes in another scale, 1 on the {w_name} is {shown} on the {r_name}, which \
             would read it as 1"
        ))
    }

    /// Two origins of points: the same zero (from the same reference), the same dimension and the same period, in
    /// coherent SI; how a point is shown (symbol, step, the shown zero) may differ.
    fn check_origin(
        &mut self,
        w_name: &str,
        w: &OriginDefOwned,
        r_name: &str,
        r: &OriginDefOwned,
    ) -> Result<(), String> {
        let differs = |what: &str| {
            format!(
                "origin {w_name} on the {}, {r_name} on the {}: {what}, the {} would read the points as other \
                 positions",
                self.writer_name, self.reader_name, self.reader_name
            )
        };
        let si = |m: Option<Magnitude>, unit: &Magnitude| m.map(|m| m.checked_mul(*unit).ok());
        let reason = if !w.unit.dim.as_dim().same(r.unit.dim.as_dim()) {
            Some(format!(
                "another dimension ({} vs {})",
                w.unit.dim.as_dim(),
                r.unit.dim.as_dim()
            ))
        } else if w.reference != r.reference {
            let name = |o: &OriginDefOwned| match &o.reference {
                Some(m) => format!("{}::{}", m.crate_id, m.name),
                None => "the unit's zero".into(),
            };
            Some(format!("another reference ({} vs {})", name(w), name(r)))
        } else if w.offset.is_negative() != r.offset.is_negative()
            || si(w.offset.magnitude(), &w.unit.magnitude)
                != si(r.offset.magnitude(), &r.unit.magnitude)
        {
            Some(format!(
                "another zero ({} {} vs {} {})",
                w.offset, w.unit.symbol, r.offset, r.unit.symbol
            ))
        } else if si(w.modulus, &w.unit.magnitude) != si(r.modulus, &r.unit.magnitude) {
            let show = |o: &OriginDefOwned| match o.modulus {
                Some(m) => format!("wraps every 1{m} {}", o.unit.symbol),
                None => "doesn't wrap".into(),
            };
            Some(format!("another period ({} vs {})", show(w), show(r)))
        } else {
            None
        };
        match reason {
            Some(reason) => {
                self.rule = Some(Rule::Meaning);
                Err(differs(&reason))
            }
            None => Ok(()),
        }
    }

    /// What marker `ty` of the writer (`side` 0) or the reader (1) means, from its definition (INT-13); `None` for a
    /// marker without one (a bound, a hand-written marker, a model from before INT-13).
    fn meaning(&self, side: usize, ty: &TypeOwned) -> Option<Meaning> {
        let bundle = [self.writer, self.reader][side];
        let defs = match bundle
            .types
            .iter()
            .any(|t| matches!(t, TypeLocationOwned::SkippedFullVersion { .. }))
        {
            true => self.defs[side].get_or_init(|| {
                let mut inlined = bundle.clone();
                wire_weaver_snapshots::inline_skipped(&mut inlined);
                inlined
            }),
            false => bundle,
        };
        meaning(defs, ty)
    }

    fn note_compatible(&mut self, unwrapped: Unwrapped) {
        if !self.unwrapped.contains(&unwrapped) {
            self.unwrapped.push(unwrapped);
        }
    }

    /// A marker argument (of a type parameter no field uses, `U` and `P` of `ww_si::Fixed<U, P, T, B>`): the same by
    /// name, or a bound that lets the reader read everything the writer writes.
    fn check_marker(&mut self, w: &GenericArgOwned, r: &GenericArgOwned) -> Result<(), String> {
        let (GenericArgOwned::Type(w), GenericArgOwned::Type(r)) = (w, r) else {
            if w == r {
                return Ok(());
            }
            return Err(format!("{w:?} vs {r:?}"));
        };
        if let (Some(w_bound), Some(r_bound)) = (bound(self.writer, w), bound(self.reader, r)) {
            return self.check_bound_marker(&w_bound, &r_bound);
        }
        // the same marker by crate and name (one side may have it in-line, the other skipped): then its
        // definition and crate version are checked as for any type
        let identity = |bundle, ty| match resolve(bundle, ty).ok()? {
            ResolvedTy::Skipped(identity) => Some(identity),
            ResolvedTy::Ty(ty) => user_type_identity(bundle, ty),
        };
        let same = match (identity(self.writer, w), identity(self.reader, r)) {
            (Some(w_id), Some(r_id)) => w_id.0.crate_id == r_id.0.crate_id && w_id.1 == r_id.1,
            _ => marker_name(self.writer, w, true) == marker_name(self.reader, r, true),
        };
        if same {
            return self.check(w, r);
        }
        self.rule = Some(Rule::Meaning);
        Err(format!(
            "marker {} on the {}, {} on the {}: the same bytes, but the {} would \
                     read them as another quantity",
            marker_name(self.writer, w, false),
            self.writer_name,
            marker_name(self.reader, r, false),
            self.reader_name,
            self.reader_name
        ))
    }

    fn check_bound_marker(&mut self, w: &Bound, r: &Bound) -> Result<(), String> {
        // a wrapping number on one side only: the same values, but a wrap read as a jump (or the other way around)
        if (*w == Bound::Wrapping) != (*r == Bound::Wrapping) {
            self.rule = Some(Rule::Meaning);
            return Err(format!(
                "{w} on the {}, {r} on the {}: a wrapping number is a position modulo its storage, the {} would \
                 take a wrap for a jump or the other way around",
                self.writer_name, self.reader_name, self.reader_name
            ));
        }
        if r.contains(w) {
            return Ok(());
        }
        self.rule = Some(Rule::Bound);
        Err(format!(
            "bound {w} on the {}, {r} on the {}: values the {} writes outside {r} are refused by the {}",
            self.writer_name, self.reader_name, self.writer_name, self.reader_name
        ))
    }

    /// A transparent wrapper (DER-18) is the bytes of its field: compared with a plain type, its field is (and a
    /// reader's bound against all the values a plain writer can write); two wrappers must be of the same definition,
    /// compared as any two instances then. `None` when neither side is a transparent wrapper.
    fn check_wrappers(&mut self, w: &TypeOwned, r: &TypeOwned) -> Option<Result<(), String>> {
        let (writer, reader) = (self.writer, self.reader);
        let w_wrapper = match wrapper(writer, w) {
            Ok(wrapper) => wrapper,
            Err(e) => return Some(Err(e)),
        };
        let r_wrapper = match wrapper(reader, r) {
            Ok(wrapper) => wrapper,
            Err(e) => return Some(Err(e)),
        };
        let (written, read) = match (w_wrapper, r_wrapper) {
            (None, None) => return None,
            (Some(w_wrapper), Some(r_wrapper)) => {
                if w_wrapper.def == r_wrapper.def {
                    return None;
                }
                self.rule = Some(Rule::Meaning);
                return Some(Err(format!(
                    "{} on the {}, {} on the {}: wrappers of the same bytes with another meaning, the {} would \
                     read them as another quantity",
                    w_wrapper.name,
                    self.writer_name,
                    r_wrapper.name,
                    self.reader_name,
                    self.reader_name
                )));
            }
            (Some(w_wrapper), None) => {
                // a plain reader takes any value
                if is_quantity(reader, r) && w_wrapper.is_fixed() {
                    return Some(Err(
                        self.quantity_to_fixed(w_wrapper.name, type_name(reader, r))
                    ));
                }
                if let Err(e) = self.check(&w_wrapper.field, r) {
                    return Some(Err(format!("{}: {e}", w_wrapper.name)));
                }
                (w_wrapper.name, type_name(reader, r))
            }
            (None, Some(r_wrapper)) => {
                // a plain number states no arithmetic: it can become a wrapping or saturating one
                for bound in r_wrapper
                    .bounds
                    .iter()
                    .filter(|b| matches!(b, Bound::Range { .. }))
                {
                    if let Err(e) = self.check_bound_marker(&Bound::Unbounded, bound) {
                        return Some(Err(format!("{}: {e}", r_wrapper.name)));
                    }
                }
                if is_quantity(writer, w) && r_wrapper.is_fixed() {
                    return Some(Err(
                        self.quantity_to_fixed(type_name(writer, w), r_wrapper.name)
                    ));
                }
                if let Err(e) = self.check(w, &r_wrapper.field) {
                    return Some(Err(format!("{}: {e}", r_wrapper.name)));
                }
                (type_name(writer, w), r_wrapper.name)
            }
        };
        let unwrapped = Unwrapped {
            path: self.path.clone(),
            writer_name: self.writer_name,
            written,
            read,
            same: "same bytes",
        };
        self.note_compatible(unwrapped);
        Some(Ok(()))
    }

    /// A `ww_si::Quantity` on one side, a `ww_si::Fixed` on the other: the intended move from one to the other, and
    /// breaking.
    fn quantity_to_fixed(&self, written: String, read: String) -> String {
        format!(
            "{written} on the {}, {read} on the {}: a Quantity sends its prefix and number type with every value, \
             a Fixed only the number: moving to Fixed is intended, but needs a major version",
            self.writer_name, self.reader_name
        )
    }

    fn check_enum(&mut self, w: &ItemEnumOwned, r: &ItemEnumOwned) -> Result<(), String> {
        check_generics(&w.generics, &r.generics)?;
        if w.repr != r.repr {
            return Err(format!("repr {:?} vs {:?}", w.repr, r.repr));
        }
        // renaming keeps the discriminant, a variant whose name is at another one on the other side moved
        // (inserted before it or reordered), its values would be read as another variant
        for variant in &w.variants {
            if let Some(moved) = r
                .variants
                .iter()
                .find(|v| v.ident == variant.ident && v.discriminant != variant.discriminant)
            {
                return Err(format!(
                    "variant {} moved: discriminant {} on the {}, {} on the {}",
                    variant.ident,
                    variant.discriminant.0,
                    self.writer_name,
                    moved.discriminant.0,
                    self.reader_name
                ));
            }
        }
        for variant in &w.variants {
            let Some(r_variant) = r
                .variants
                .iter()
                .find(|v| v.discriminant == variant.discriminant)
            else {
                self.check_unknown_variant(variant, r)?;
                continue;
            };
            let segment = format!("variant {}", r_variant.ident);
            self.within(&segment, |c| {
                c.check_fields(&w.size, &variant.fields, &r.size, &r_variant.fields)
            })
            .map_err(|e| format!("{segment}: {e}"))?;
        }
        Ok(())
    }

    /// A variant the reader doesn't know is read as its `#[future_version]` catch-all (SW-47, EVO-10), if it has one
    /// and can skip the variant's fields: an `Unsized` enum skips them with its size slot, any other only reads past
    /// a unit variant. A catch-all narrower than `u32` (DER-25) doesn't hold a discriminant beyond its field.
    fn check_unknown_variant(
        &self,
        variant: &VariantOwned,
        r: &ItemEnumOwned,
    ) -> Result<(), String> {
        let unknown = format!(
            "variant {} is unknown to the {}",
            variant.ident, self.reader_name
        );
        match &r.future_version {
            Some(FutureVersionOwned::Variant { ident, bits })
                if bits.is_some_and(|bits| variant.discriminant.0 >= 1u32 << bits) =>
            {
                Err(format!(
                    "{unknown}, its discriminant {} doesn't fit the catch-all {ident}(u{})",
                    variant.discriminant.0,
                    bits.unwrap_or(32)
                ))
            }
            Some(FutureVersionOwned::Variant { ident, bits }) => {
                if matches!(r.size, ElementSize::Unsized)
                    || matches!(variant.fields, FieldsOwned::Unit)
                {
                    Ok(())
                } else {
                    Err(format!(
                        "{unknown}, its catch-all {ident}(u{}) can't skip the variant's fields: only Unsized \
                         enums can gain variants with fields",
                        bits.unwrap_or(32)
                    ))
                }
            }
            Some(FutureVersionOwned::Off) => {
                Err(format!("{unknown}, which has no catch-all for it"))
            }
            None => Err(format!(
                "{unknown}: whether the {} has a catch-all for it is not recorded (the API model is from before \
                 EVO-10, save its snapshot again)",
                self.reader_name
            )),
        }
    }

    fn check_fields(
        &mut self,
        w_size: &ElementSize,
        w: &FieldsOwned,
        r_size: &ElementSize,
        r: &FieldsOwned,
    ) -> Result<(), String> {
        // Sized and SelfDescribing are laid out the same way: a parent stores a size only for Unsized, and
        // FinalStructure shares the parent's size slots
        let layout = |size: &ElementSize| match size {
            ElementSize::Sized { .. } => std::mem::discriminant(&ElementSize::SelfDescribing),
            other => std::mem::discriminant(other),
        };
        if layout(w_size) != layout(r_size) {
            return Err(format!("{w_size:?} vs {r_size:?}"));
        }
        let (w, r) = (fields(w), fields(r));
        let is_unsized = matches!(r_size, ElementSize::Unsized);
        if w.len() != r.len() {
            let (fewer, more) = if w.len() < r.len() { (w, r) } else { (r, w) };
            // Unsized types can gain fields at the end as well, compared by position below
            let in_between =
                layout::last_old_field(fewer, more).is_some_and(|last| last + 1 != fewer.len());
            if !is_unsized || in_between {
                return self.check_padding_reuse(w, r, is_unsized);
            }
        }
        for (i, (w_field, r_field)) in w.iter().zip(r).enumerate() {
            let segment = format!("field {}", field_name(i, r_field));
            self.within(&segment, |c| c.check(&w_field.ty, &r_field.ty))
                .map_err(|e| format!("{segment}: {e}"))?;
        }
        if is_unsized {
            self.check_trailing(w.len(), r)?;
        }
        Ok(())
    }

    /// Writer's extra trailing fields are skipped, reader's extra fields must have a default.
    fn check_trailing(&self, written: usize, r: &[FieldOwned]) -> Result<(), String> {
        for (i, r_field) in r.iter().enumerate().skip(written) {
            if r_field.default.is_none() {
                return Err(format!(
                    "field {} is not sent by the {} and has no #[default]",
                    field_name(i, r_field),
                    self.writer_name
                ));
            }
        }
        Ok(())
    }

    /// New fields in between the old ones, in their unused padding bits, see [layout]. `Unsized` types can also
    /// gain fields at the end.
    fn check_padding_reuse(
        &mut self,
        w: &[FieldOwned],
        r: &[FieldOwned],
        is_unsized: bool,
    ) -> Result<(), String> {
        let ((fewer_bundle, fewer), (more_bundle, more)) = if w.len() < r.len() {
            ((self.writer, w), (self.reader, r))
        } else {
            ((self.reader, r), (self.writer, w))
        };
        let in_padding = match (is_unsized, layout::last_old_field(fewer, more)) {
            (true, Some(last)) => &more[..=last],
            _ => more,
        };
        layout::check_padding_reuse(fewer_bundle, fewer, more_bundle, in_padding, is_unsized)
            .map_err(|e| {
                let can_grow = if is_unsized {
                    "Unsized types can gain new fields at the end, and in between old ones only in unused padding bits"
                } else {
                    "only Unsized types can gain new fields at the end, other types only in unused padding bits"
                };
                format!(
                    "{} fields on the {}, {} on the {}, {can_grow}: {e}",
                    w.len(),
                    self.writer_name,
                    r.len(),
                    self.reader_name
                )
            })?;
        if is_unsized && r.len() > w.len() {
            self.check_trailing(in_padding.len(), r)?;
        }
        // old fields are matched by name, new ones are in between
        for (i, w_field) in w.iter().enumerate() {
            let r_field = match &w_field.ident {
                Some(_) => r.iter().find(|f| f.ident == w_field.ident),
                None => r.get(i),
            };
            if let Some(r_field) = r_field {
                let segment = format!("field {}", field_name(i, r_field));
                self.within(&segment, |c| c.check(&w_field.ty, &r_field.ty))
                    .map_err(|e| format!("{segment}: {e}"))?;
            }
        }
        Ok(())
    }
}

fn resolve<'a>(bundle: &'a ApiBundleOwned, ty: &'a TypeOwned) -> Result<ResolvedTy<'a>, String> {
    let TypeOwned::OutOfLine { type_idx } = ty else {
        return Ok(ResolvedTy::Ty(ty));
    };
    match bundle.types.get(type_idx.0 as usize) {
        Some(TypeLocationOwned::InLine { ty, .. }) => Ok(ResolvedTy::Ty(ty)),
        Some(TypeLocationOwned::SkippedFullVersion {
            crate_idx,
            type_name,
            ..
        }) => {
            let version = bundle
                .crate_version(crate_idx.0)
                .map_err(|e| e.to_string())?;
            Ok(ResolvedTy::Skipped((
                version,
                Cow::Borrowed(type_name.as_str()),
            )))
        }
        None => Err(format!("bad API bundle: no type with index {}", type_idx.0)),
    }
}

/// A `transparent` struct (DER-18) as one side of a check.
struct Wrapper<'b> {
    /// Its name with the arguments, `Fixed<Second, Micro, u64, Unbounded>`.
    name: String,
    /// Crate and qualified name of its definition, the generic one for an instance.
    def: Option<(String, String)>,
    /// The type of its one field, the arguments filled in.
    field: Cow<'b, TypeOwned>,
    /// Bounds among its marker arguments (`B` of `ww_si::Fixed`).
    bounds: Vec<Bound>,
}

impl Wrapper<'_> {
    fn is_fixed(&self) -> bool {
        self.def
            .as_ref()
            .is_some_and(|(krate, name)| is_fixed(krate, name))
    }
}

/// `ty` as a transparent wrapper, `None` when it is not one (or its definition is not known here).
fn wrapper<'b>(
    bundle: &'b ApiBundleOwned,
    ty: &'b TypeOwned,
) -> Result<Option<Wrapper<'b>>, String> {
    let field_of = |s: &ItemStructOwned| -> Result<TypeOwned, String> {
        Ok(s.transparent_field().map_err(|e| e.to_string())?.ty.clone())
    };
    if let TypeOwned::Instance { type_idx, args } = ty {
        let Some(def @ TypeOwned::Struct(def_struct)) = type_def(bundle, type_idx.0) else {
            return Ok(None);
        };
        if !def_struct.is_transparent() {
            return Ok(None);
        }
        let (field, name) = match bundle.types.get(type_idx.0 as usize) {
            Some(TypeLocationOwned::InLine { .. }) => {
                let TypeOwned::Struct(s) = ww_self::generics::substitute(bundle, type_idx.0, args)
                    .map_err(|e| e.to_string())?
                else {
                    return Ok(None);
                };
                (field_of(&s)?, s.ident)
            }
            // from a snapshot: only a field that is one of the parameters is known here
            _ => match field_of(def_struct)? {
                TypeOwned::Param(p) => match args.get(p.0 as usize) {
                    Some(GenericArgOwned::Type(field)) => (field.clone(), type_name(bundle, ty)),
                    _ => return Ok(None),
                },
                _ => return Ok(None),
            },
        };
        return Ok(Some(Wrapper {
            field: Cow::Owned(field),
            def: def_name(bundle, type_idx.0).map(|(c, n)| (c, n.into_owned())),
            bounds: bounds_of(bundle, def, args),
            name,
        }));
    }
    let ResolvedTy::Ty(TypeOwned::Struct(s)) = resolve(bundle, ty)? else {
        return Ok(None);
    };
    if !s.is_transparent() {
        return Ok(None);
    }
    let field = Cow::Borrowed(&s.transparent_field().map_err(|e| e.to_string())?.ty);
    let (def, bounds) = match &s.instance_of {
        Some(instance_of) => {
            let idx = instance_of.type_idx.0;
            let bounds = type_def(bundle, idx)
                .map(|def| bounds_of(bundle, def, &instance_of.args))
                .unwrap_or_default();
            (
                def_name(bundle, idx).map(|(c, n)| (c, n.into_owned())),
                bounds,
            )
        }
        None => (
            user_type_identity(bundle, ty_of_struct(bundle, ty))
                .map(|(v, n)| (v.crate_id.clone(), n.into_owned())),
            vec![],
        ),
    };
    Ok(Some(Wrapper {
        name: s.ident.clone(),
        def,
        field,
        bounds,
    }))
}

/// The struct or enum definition `ty` refers to (or `ty` itself).
fn ty_of_struct<'b>(bundle: &'b ApiBundleOwned, ty: &'b TypeOwned) -> &'b TypeOwned {
    match resolve(bundle, ty) {
        Ok(ResolvedTy::Ty(ty)) => ty,
        _ => ty,
    }
}

/// Crate and qualified name of the type at `type_idx`, in-line or skipped.
fn def_name(bundle: &ApiBundleOwned, type_idx: u32) -> Option<(String, Cow<'_, str>)> {
    match bundle.types.get(type_idx as usize)? {
        TypeLocationOwned::InLine { ty, .. } => {
            user_type_identity(bundle, ty).map(|(version, name)| (version.crate_id.clone(), name))
        }
        TypeLocationOwned::SkippedFullVersion {
            crate_idx,
            type_name,
            ..
        } => Some((
            bundle.crate_version(crate_idx.0).ok()?.crate_id.clone(),
            Cow::Borrowed(type_name.as_str()),
        )),
    }
}

/// Crate and name of the generic definition `ty` is an instance of, prepared or not.
fn instance_def_name<'b>(
    bundle: &'b ApiBundleOwned,
    ty: &'b TypeOwned,
) -> Option<(String, Cow<'b, str>)> {
    match ty {
        TypeOwned::Instance { type_idx, .. } => def_name(bundle, type_idx.0),
        _ => match resolve(bundle, ty).ok()? {
            ResolvedTy::Ty(TypeOwned::Struct(s)) => {
                def_name(bundle, s.instance_of.as_ref()?.type_idx.0)
            }
            _ => None,
        },
    }
}

/// Whether `ty` is a `ww_si::Quantity`.
fn is_quantity(bundle: &ApiBundleOwned, ty: &TypeOwned) -> bool {
    instance_def_name(bundle, ty).is_some_and(|(krate, name)| is_ww_si(&krate, &name, "Quantity"))
}

fn is_ww_si(krate: &str, qualified_name: &str, name: &str) -> bool {
    krate == "ww_si" && qualified_name.rsplit("::").next() == Some(name)
}

/// Whether a definition is `Fixed`: defined in `exact_units` (STD-61), re-exported by `ww_si`.
fn is_fixed(krate: &str, qualified_name: &str) -> bool {
    (krate == "exact_units" || krate == "ww_si")
        && qualified_name.rsplit("::").next() == Some("Fixed")
}

/// The definition of type `type_idx`: in-line, or for a skipped one (a `ww_si` type in an API) the definition with the
/// same name and signature in the embedded snapshots of its crate. Field types of a definition from a snapshot refer
/// to that snapshot, only its own parameters can be read here.
fn type_def(bundle: &ApiBundleOwned, type_idx: u32) -> Option<&TypeOwned> {
    match bundle.types.get(type_idx as usize)? {
        TypeLocationOwned::InLine { ty, .. } => Some(ty),
        TypeLocationOwned::SkippedFullVersion {
            crate_idx,
            type_name,
            signature: expected,
        } => {
            let snapshot = wire_weaver_snapshots::get(bundle.crate_version(crate_idx.0).ok()?)?;
            snapshot
                .types
                .iter()
                .enumerate()
                .find_map(|(idx, location)| match location {
                    TypeLocationOwned::InLine { ty, .. }
                        if user_type_identity(snapshot, ty)
                            .is_some_and(|(_, name)| name == type_name.as_str())
                            && (expected.is_empty()
                                || type_signature(snapshot, idx as u32).as_ref()
                                    == Some(expected)) =>
                    {
                        Some(ty)
                    }
                    _ => None,
                })
        }
    }
}

/// Type parameters of a generic struct or enum that no field uses: markers, which only give the bytes a meaning (`U`
/// and `P` of `ww_si::Fixed<U, P, T, B>`, `PhantomData` in Rust, not in the model): their positions and names.
fn marker_params(def: &TypeOwned) -> Vec<(usize, &str)> {
    let (generics, all_fields): (&[GenericParamOwned], Vec<&FieldsOwned>) = match def {
        TypeOwned::Struct(s) => (&s.generics, vec![&s.fields]),
        TypeOwned::Enum(e) => (&e.generics, e.variants.iter().map(|v| &v.fields).collect()),
        _ => return vec![],
    };
    (0..generics.len())
        .filter(|&i| matches!(generics[i].kind, GenericParamKindOwned::Type))
        .filter(|&i| {
            !all_fields
                .iter()
                .any(|f| fields(f).iter().any(|f| uses_param(&f.ty, i)))
        })
        .map(|i| (i, generics[i].ident.as_str()))
        .collect()
}

fn uses_param(ty: &TypeOwned, idx: usize) -> bool {
    use TypeOwned::*;
    match ty {
        Param(p) => p.0 as usize == idx,
        Vec(ty) | Box(ty) | Series(ty) | Array { ty, .. } | MaxLen { ty, .. } => {
            uses_param(ty, idx)
        }
        Option { some_ty } => uses_param(some_ty, idx),
        Result { ok_ty, err_ty } => uses_param(ok_ty, idx) || uses_param(err_ty, idx),
        Tuple(types) => types.iter().any(|ty| uses_param(ty, idx)),
        Instance { args, .. } => args
            .iter()
            .any(|arg| matches!(arg, GenericArgOwned::Type(ty) if uses_param(ty, idx))),
        Struct(s) => fields(&s.fields).iter().any(|f| uses_param(&f.ty, idx)),
        Enum(e) => e
            .variants
            .iter()
            .any(|v| fields(&v.fields).iter().any(|f| uses_param(&f.ty, idx))),
        Bool | NumericAny(_) | OutOfLine { .. } | Flag | String | Range(_) | RangeInclusive(_) => {
            false
        }
    }
}

/// The bounds among the marker arguments of an instance of `def`.
fn bounds_of(bundle: &ApiBundleOwned, def: &TypeOwned, args: &[GenericArgOwned]) -> Vec<Bound> {
    marker_params(def)
        .into_iter()
        .filter_map(|(i, _)| match args.get(i)? {
            GenericArgOwned::Type(ty) => bound(bundle, ty),
            _ => None,
        })
        .collect()
}

/// Name of a marker argument, with its crate for comparing.
fn marker_name(bundle: &ApiBundleOwned, ty: &TypeOwned, with_crate: bool) -> String {
    ty.human_name_bounded(with_crate, bundle, &[], MAX_NAME_LEN)
        .unwrap_or_else(|_| describe(ty))
}

fn type_name(bundle: &ApiBundleOwned, ty: &TypeOwned) -> String {
    marker_name(bundle, ty, false)
}

/// Longest name of a type shown in a message.
const MAX_NAME_LEN: usize = 256;

/// A bound marker of `ww_si` (`B` of `Fixed<U, P, T, B>`), recognised by its name and arguments in any crate, as
/// dynamic clients do: `Unbounded`, `Range<MIN, MAX>`, `RangeU64<MIN, MAX>` and `RangeDec<MIN, MAX, EXP>`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Bound {
    Unbounded,
    /// `Saturating`: no bounds, arithmetic stops at the ends of the storage (STD-63). The same values as
    /// [Self::Unbounded].
    Saturating,
    /// `Wrapping`: no bounds, but the number is a position modulo 2^bits of its storage (binary angles, free-running
    /// counters, STD-63): differences and order mean something else than for a plain number.
    Wrapping,
    /// Inclusive.
    Range {
        min: Dec,
        max: Dec,
    },
}

/// `mantissa · 10^exp`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Dec(i128, i8);

impl Dec {
    fn cmp(&self, other: &Dec) -> Ordering {
        let exp = self.1.min(other.1);
        let scaled = |d: &Dec| {
            10i128
                .checked_pow((d.1 - exp) as u32)
                .and_then(|scale| d.0.checked_mul(scale))
        };
        match (scaled(self), scaled(other)) {
            (Some(a), Some(b)) => a.cmp(&b),
            // far apart: compare as floats
            _ => (self.0 as f64 * 10f64.powi(self.1 as i32))
                .total_cmp(&(other.0 as f64 * 10f64.powi(other.1 as i32))),
        }
    }
}

impl fmt::Display for Dec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Dec(m, exp) = *self;
        if exp >= 0 {
            return write!(f, "{m}{}", "0".repeat(exp as usize));
        }
        let digits = format!("{:0>width$}", m.unsigned_abs(), width = -exp as usize + 1);
        let (int, frac) = digits.split_at(digits.len() - (-exp) as usize);
        let sign = if m < 0 { "-" } else { "" };
        write!(f, "{sign}{int}.{frac}")
    }
}

impl Bound {
    /// Whether every value within `other` is within `self`.
    fn contains(&self, other: &Bound) -> bool {
        match (self, other) {
            (Bound::Unbounded | Bound::Saturating | Bound::Wrapping, _) => true,
            (Bound::Range { .. }, Bound::Unbounded | Bound::Saturating | Bound::Wrapping) => false,
            (
                Bound::Range { min, max },
                Bound::Range {
                    min: o_min,
                    max: o_max,
                },
            ) => min.cmp(o_min) != Ordering::Greater && o_max.cmp(max) != Ordering::Greater,
        }
    }
}

impl fmt::Display for Bound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Bound::Unbounded => f.write_str("Unbounded"),
            Bound::Saturating => f.write_str("Saturating"),
            Bound::Wrapping => f.write_str("Wrapping"),
            Bound::Range { min, max } => write!(f, "{min}..={max}"),
        }
    }
}

fn bound(bundle: &ApiBundleOwned, ty: &TypeOwned) -> Option<Bound> {
    let (name, args): (Cow<str>, &[GenericArgOwned]) = match ty {
        TypeOwned::Instance { type_idx, args } => (def_name(bundle, type_idx.0)?.1, args),
        _ => match resolve(bundle, ty).ok()? {
            ResolvedTy::Ty(TypeOwned::Struct(s)) => match &s.instance_of {
                Some(instance_of) => (
                    def_name(bundle, instance_of.type_idx.0)?.1,
                    &instance_of.args,
                ),
                None if s.generics.is_empty() => (Cow::Borrowed(s.ident.as_str()), &[]),
                None => return None,
            },
            ResolvedTy::Skipped((_, name)) => (name, &[]),
            ResolvedTy::Ty(_) => return None,
        },
    };
    let num = |arg: &GenericArgOwned| match arg {
        GenericArgOwned::Const(ValueOwned::Numeric(n)) => n.to_i128(),
        _ => None,
    };
    match (name.rsplit("::").next()?, args) {
        ("Unbounded", []) => Some(Bound::Unbounded),
        ("Saturating", []) => Some(Bound::Saturating),
        ("Wrapping", []) => Some(Bound::Wrapping),
        ("Range" | "RangeU64", [min, max]) => Some(Bound::Range {
            min: Dec(num(min)?, 0),
            max: Dec(num(max)?, 0),
        }),
        ("RangeDec", [min, max, exp]) => {
            let exp = i8::try_from(num(exp)?).ok()?;
            Some(Bound::Range {
                min: Dec(num(min)?, exp),
                max: Dec(num(max)?, exp),
            })
        }
        _ => None,
    }
}

/// Type and const parameters of two versions of a generic definition: the same number, of the same kinds, in the
/// same order. Renaming one is not a change on the wire.
fn check_generics(w: &[GenericParamOwned], r: &[GenericParamOwned]) -> Result<(), String> {
    let kinds = |params: &[GenericParamOwned]| {
        params
            .iter()
            .map(|p| match &p.kind {
                GenericParamKindOwned::Type => p.ident.clone(),
                GenericParamKindOwned::Const { ty } => format!("const {}: {ty:?}", p.ident),
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let same = w.len() == r.len()
        && w.iter().zip(r).all(|(w, r)| match (&w.kind, &r.kind) {
            (GenericParamKindOwned::Type, GenericParamKindOwned::Type) => true,
            (GenericParamKindOwned::Const { ty: w }, GenericParamKindOwned::Const { ty: r }) => {
                w == r
            }
            _ => false,
        });
    if same {
        Ok(())
    } else {
        Err(format!("parameters <{}> vs <{}>", kinds(w), kinds(r)))
    }
}

/// A length or bound that is a number: a const parameter only on one side, or two different ones, can't be
/// compared.
fn fixed(len: &Length) -> Result<u32, String> {
    len.fixed().map_err(|e| e.to_string())
}

fn user_type_identity<'a>(bundle: &'a ApiBundleOwned, ty: &'a TypeOwned) -> Option<Identity<'a>> {
    let (crate_idx, name) = match ty {
        TypeOwned::Struct(s) => (s.crate_idx, s.qualified_name()),
        TypeOwned::Enum(e) => (e.crate_idx, e.qualified_name()),
        _ => return None,
    };
    Some((bundle.crate_version(crate_idx.0).ok()?, Cow::Owned(name)))
}

pub fn fields(fields: &FieldsOwned) -> &[FieldOwned] {
    match fields {
        FieldsOwned::Named(fields) | FieldsOwned::Unnamed(fields) => fields,
        FieldsOwned::Unit => &[],
    }
}

pub fn field_name(idx: usize, field: &FieldOwned) -> String {
    match &field.ident {
        Some(ident) => format!("`{ident}`"),
        None => format!("{idx}"),
    }
}

fn describe(ty: &TypeOwned) -> String {
    match ty {
        TypeOwned::Bool => "bool".into(),
        TypeOwned::NumericAny(n) => {
            // Base(U8) -> u8
            let n = format!("{n:?}");
            match n.strip_prefix("Base(").and_then(|n| n.strip_suffix(')')) {
                Some(base) => base.to_lowercase(),
                None => n,
            }
        }
        TypeOwned::OutOfLine { type_idx } => format!("type #{}", type_idx.0),
        TypeOwned::Flag => "flag".into(),
        TypeOwned::String => "String".into(),
        TypeOwned::Vec(_) => "Vec".into(),
        TypeOwned::Series(_) => "Series".into(),
        TypeOwned::Array { len, .. } => format!("array of {}", len.human_name(&[])),
        TypeOwned::Instance { type_idx, .. } => format!("instance of generic type #{}", type_idx.0),
        TypeOwned::Param(idx) => format!("type parameter {}", idx.0),
        TypeOwned::Tuple(t) => format!("tuple of {}", t.len()),
        TypeOwned::Struct(s) => format!("struct {}", s.ident),
        TypeOwned::Enum(e) => format!("enum {}", e.ident),
        TypeOwned::Option { .. } => "Option".into(),
        TypeOwned::Result { .. } => "Result".into(),
        TypeOwned::Box(_) => "Box".into(),
        TypeOwned::Range(_) => "Range".into(),
        TypeOwned::RangeInclusive(_) => "RangeInclusive".into(),
        TypeOwned::MaxLen { max_len, ty } => {
            format!("{} of at most {}", describe(ty), max_len.human_name(&[]))
        }
    }
}

/// What a marker with a definition (INT-13) means, see [TypeCheck::check_markers].
enum Meaning {
    Unit(Box<CanonicalUnit>),
    Scale(Magnitude),
    Origin(Box<OriginDefOwned>),
}

fn meaning(bundle: &ApiBundleOwned, ty: &TypeOwned) -> Option<Meaning> {
    if let Ok(unit) = units::unit_of(ty, bundle) {
        return Some(Meaning::Unit(Box::new(unit)));
    }
    if let Ok(scale) = units::scale_of(ty, bundle) {
        return Some(Meaning::Scale(scale));
    }
    units::origin_of(ty, bundle)
        .ok()
        .map(|o| Meaning::Origin(Box::new(o)))
}

/// One step of a number: the units and scales of an instance multiplied, in coherent SI.
struct Step {
    dim: DimOwned,
    magnitude: Magnitude,
}

impl Step {
    fn one() -> Self {
        Step {
            dim: exact_units::Dim::ONE.into(),
            magnitude: Magnitude::ONE,
        }
    }

    fn same(&self, other: &Step) -> bool {
        self.dim.as_dim().same(other.dim.as_dim()) && self.magnitude == other.magnitude
    }

    fn mul(&mut self, meaning: &Meaning) -> Result<(), String> {
        let (dim, magnitude) = match meaning {
            Meaning::Unit(unit) => (Some(&unit.dim), unit.magnitude),
            Meaning::Scale(scale) => (None, *scale),
            Meaning::Origin(_) => return Ok(()),
        };
        if let Some(dim) = dim {
            self.dim = self
                .dim
                .as_dim()
                .checked_mul(dim.as_dim())
                .ok_or("dimension out of range")?
                .into();
        }
        self.magnitude = self
            .magnitude
            .checked_mul(magnitude)
            .map_err(|e| format!("factor out of range: {e:?}"))?;
        Ok(())
    }
}

/// `x` with at most 6 decimals, without trailing zeros: `4.448222`, `1000`.
fn round6(x: f64) -> String {
    let s = format!("{x:.6}");
    let s = s.trim_end_matches('0');
    s.trim_end_matches('.').to_string()
}
