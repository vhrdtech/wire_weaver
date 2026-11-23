//! The shape of the API model as RON and serde see it, frozen in `model_format.txt` with the format version
//! ([ww_self::VERSION]) it belongs to. A change of the model (a field added, renamed or retyped, a variant, a type of
//! a dependency the model uses) changes how snapshot files read and encode: it needs a new [ww_self::VERSION], so
//! that a file of the old format is refused with the step that re-saves it instead of a hash mismatch.

use serde_reflection::{Samples, Tracer, TracerConfig};
use std::fmt::Write;
use std::path::PathBuf;

/// Set to rewrite `model_format.txt` after [ww_self::VERSION] was bumped.
const UPDATE_ENV: &str = "WW_UPDATE_MODEL_FORMAT";

fn frozen_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/model_format.txt")
}

fn version_line() -> String {
    let v = ww_self::VERSION;
    format!(
        "ww_self model format {}.{}.{}",
        v.major.0, v.minor.0, v.patch.0
    )
}

fn model_shape() -> String {
    // numbers read as 1, not 0: a fraction's denominator, a count of exponents must be valid where they are checked
    let config = TracerConfig::default()
        .record_samples_for_structs(true)
        .default_u8_value(1)
        .default_u16_value(1)
        .default_u32_value(1)
        .default_u64_value(1)
        .default_u128_value(1)
        .default_i8_value(1)
        .default_i16_value(1)
        .default_i32_value(1)
        .default_i64_value(1)
        .default_i128_value(1);
    let mut tracer = Tracer::new(config);
    // the fields of a dimension as RON holds them: traced without samples before the checked type gets one
    tracer
        .trace_type::<exact_units::DimWireOwned>(&Samples::new())
        .expect("tracing a dimension's fields");
    let mut samples = Samples::new();
    // a dimension and a magnitude are checked on read: the tracer's made-up ones are refused, real ones are the samples
    tracer
        .trace_value(
            &mut samples,
            &exact_units::DimOwned::from(exact_units::Dim::ONE),
        )
        .expect("tracing a dimension");
    tracer
        .trace_value(&mut samples, &exact_units::Magnitude::ONE)
        .expect("tracing a magnitude");
    tracer
        .trace_type::<ww_self::ApiBundleOwned>(&samples)
        .expect("tracing the model");
    trace_enums(&mut tracer, &samples);
    let registry = tracer
        .registry()
        .expect("every model enum traced: add the one named to trace_enums");
    let mut out = String::new();
    for (name, format) in &registry {
        writeln!(out, "{name}: {format:#?}").expect("writing to a String");
    }
    out
}

#[test]
fn model_format_frozen() {
    let current = format!("{}\n{}", version_line(), model_shape());
    let path = frozen_path();
    if std::env::var_os(UPDATE_ENV).is_some() {
        std::fs::write(&path, &current).expect("writing model_format.txt");
        return;
    }
    let frozen = std::fs::read_to_string(&path).unwrap_or_default();
    let (frozen_version, frozen_shape) = frozen.split_once('\n').unwrap_or(("", ""));
    let shape = current.split_once('\n').map_or("", |(_, s)| s);
    if frozen_shape != shape && frozen_version == version_line() {
        panic!(
            "the API model changed but ww_self::VERSION did not: snapshot files saved before this change can't be \
             read. Bump ww_self::VERSION (ww_self/src/lib.rs), re-save every stored snapshot (`ww api save`, `just \
             save-snapshots`), then run `{UPDATE_ENV}=1 cargo nextest run -p ww_snapshots model_format`"
        );
    }
    assert_eq!(
        frozen, current,
        "ww_self::VERSION changed: run `{UPDATE_ENV}=1 cargo nextest run -p ww_snapshots model_format` to record \
         the model shape of this format"
    );
}

/// The tracer sees one variant of an enum per pass from its parent: each enum of the model is traced by itself,
/// which covers all of its variants.
fn trace_enums(tracer: &mut Tracer, samples: &Samples) {
    macro_rules! trace {
        ($($ty:ty),* $(,)?) => {
            $(tracer.trace_type::<$ty>(samples).expect(stringify!($ty));)*
        };
    }
    trace!(
        ww_self::ApiItemKindOwned,
        ww_self::ApiLevelLocationOwned,
        ww_self::Batching,
        ww_self::ClaimMode,
        ww_self::Delivery,
        ww_self::FieldsOwned,
        ww_self::FieldsValueOwned,
        ww_self::FlowControl,
        ww_self::FutureVersionOwned,
        ww_self::GenericArgOwned,
        ww_self::GenericParamKindOwned,
        ww_self::Length,
        ww_self::MarkerDefOwned,
        ww_self::Multiplicity,
        ww_self::PropertyAccess,
        ww_self::ReferenceHow,
        ww_self::Repr,
        ww_self::TimeCounting,
        ww_self::TimeZero,
        ww_self::TrackLayout,
        ww_self::TypeLocationOwned,
        ww_self::TypeOwned,
        ww_self::ValueOwned,
        Result<Box<ww_self::ValueOwned>, Box<ww_self::ValueOwned>>,
        exact_units::Character,
        exact_units::codec::CodecShapeOwned,
        exact_units::codec::CurveForm,
        exact_units::codec::ExpBase,
        exact_units::codec::Referred,
        exact_units::PrefixPolicy,
        exact_units::ProvenanceOwned,
        shrink_wrap::ElementSize,
        shrink_wrap::un::IN,
        shrink_wrap::un::UN,
        ww_numeric::NumericAnyTypeOwned,
        ww_numeric::NumericBaseType,
        ww_numeric::NumericValue,
        ww_numeric::SubTypeKindOwned,
    );
}
