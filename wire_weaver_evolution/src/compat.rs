//! Compatibility of one resource or type with another version of it: what one side writes, can the other read?
//!
//! Shared by the evolution checker (an old and a new version of a crate) and `wire_weaver_client` (the client's API and
//! the connected device's). Types are compared structurally, following
//! [evolution rules](https://github.com/vhrdtech/wire_weaver/blob/master/docs/evolution/rules.md): names don't matter,
//! positions do. Direction matters as well: an `Unsized` struct written by one side can have more trailing fields than
//! the reader knows about (they are skipped), but a reader's extra fields must have `#[default]`.

use std::borrow::Cow;
use std::collections::HashSet;

use shrink_wrap::ElementSize;
use ww_self::signature;
use ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ArgumentOwned, Delivery,
    FieldOwned, FieldsOwned, GenericArgOwned, GenericParamKindOwned, GenericParamOwned,
    ItemEnumOwned, ItemStructOwned, Length, PropertyAccess, TypeLocationOwned, TypeOwned,
};
use ww_version::FullVersionOwned;

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
        to_device
            .check(&arg.ty, &device_arg.ty)
            .map_err(|e| format!("argument `{}`: {e}", arg.ident))?;
    }
    match (return_ty, device_return_ty) {
        (None, None) => {}
        (Some(ty), Some(device_ty)) => to_client
            .check(device_ty, ty)
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
        }
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

    pub fn check(&mut self, w: &'a TypeOwned, r: &'a TypeOwned) -> Result<(), String> {
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

    fn check_inner(&mut self, w: &'a TypeOwned, r: &'a TypeOwned) -> Result<(), String> {
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
                    self.check(w, r).map_err(|e| format!(".{i}: {e}"))?;
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
        (wi, wa): (u32, &'a [GenericArgOwned]),
        (ri, ra): (u32, &'a [GenericArgOwned]),
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
        for (i, (w_arg, r_arg)) in wa.iter().zip(ra).enumerate() {
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

    fn check_struct(
        &mut self,
        w: &'a ItemStructOwned,
        r: &'a ItemStructOwned,
    ) -> Result<(), String> {
        check_generics(&w.generics, &r.generics)?;
        self.check_fields(&w.size, &w.fields, &r.size, &r.fields)
    }

    fn check_enum(&mut self, w: &'a ItemEnumOwned, r: &'a ItemEnumOwned) -> Result<(), String> {
        check_generics(&w.generics, &r.generics)?;
        if w.repr != r.repr {
            return Err(format!("repr {:?} vs {:?}", w.repr, r.repr));
        }
        for variant in &w.variants {
            // unknown variants can't be read, extra variants on the reader side are never sent
            let Some(r_variant) = r
                .variants
                .iter()
                .find(|v| v.discriminant == variant.discriminant)
            else {
                return Err(format!(
                    "variant {} is unknown to the {}",
                    variant.ident, self.reader_name
                ));
            };
            self.check_fields(&w.size, &variant.fields, &r.size, &r_variant.fields)
                .map_err(|e| format!("variant {}: {e}", r_variant.ident))?;
        }
        Ok(())
    }

    fn check_fields(
        &mut self,
        w_size: &ElementSize,
        w: &'a FieldsOwned,
        r_size: &ElementSize,
        r: &'a FieldsOwned,
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
            self.check(&w_field.ty, &r_field.ty)
                .map_err(|e| format!("field {}: {e}", field_name(i, r_field)))?;
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
        w: &'a [FieldOwned],
        r: &'a [FieldOwned],
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
                self.check(&w_field.ty, &r_field.ty)
                    .map_err(|e| format!("field {}: {e}", field_name(i, r_field)))?;
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
