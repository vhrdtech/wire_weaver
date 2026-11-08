//! The scenario of `lines.rs` through the dynamic client (EVO-28): no generated code, every device of the three lines
//! read from its own introspection data. What a script or a tool (`ww`, Python) does against several wire lines.
//!
//! - Units are in the types, so a value in another scale or storage needs no per-line code: a temperature is read as
//!   a quantity whatever its step and storage, a gain of 1.5 or an offset of 0.001 K is refused by
//!   [DynQuantity::to_value] before anything is sent to a line that can't take it exactly.
//! - What moved or changed shape is looked up by what the device has, never by its line number: `configure` or
//!   `set_rate`, `uptime` at the root or in `health`, a `Sample` or a `Batch` on the stream, `Mode::Triggered` in the
//!   enum or not.

use thermo_devices as devices;
use wire_weaver::ww_version::WireVersion;
use wire_weaver_client::quantity::{DynQuantity, DynUnit};
use wire_weaver_client::ww_self::{
    ApiBundleOwned, ApiItemKindOwned, FieldOwned, FieldsOwned, FieldsValueOwned, ItemStructOwned,
    TypeOwned, ValueOwned,
};
use wire_weaver_client::{ClientConfig, DynClient, DynResource};
use ww_client_server::observe::ObserveMode;
use ww_si::Prefix;
use ww_si::ww_numeric::NumericValue;

struct Device {
    client: DynClient,
    root: DynResource,
}

impl Device {
    async fn connect(device: &tests_common::TestDevice) -> Self {
        let client = DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        let bundle = client.device_introspect().unwrap().api_bundle.clone();
        Device {
            client,
            root: DynResource::root(bundle),
        }
    }

    fn bundle(&self) -> &ApiBundleOwned {
        self.root.bundle()
    }

    /// The first of `paths` (`a.b` for a sub-level) the device has: where an item is on its line.
    fn find(&self, paths: &[&str]) -> Option<DynResource> {
        paths.iter().find_map(|path| {
            path.split('.')
                .try_fold(self.root.clone(), |r, name| r.child(name).ok())
        })
    }

    fn get(&self, path: &str) -> DynResource {
        self.find(&[path]).unwrap_or_else(|| panic!("no {path}"))
    }

    async fn call(&self, r: &DynResource, args: &[ValueOwned]) -> Option<ValueOwned> {
        r.call(self.client.cmd(), args).await.unwrap()
    }

    async fn read(&self, path: &str) -> (ValueOwned, TypeOwned) {
        let r = self.get(path);
        (r.read(self.client.cmd()).await.unwrap(), property_ty(&r))
    }

    /// Writes `q` into the property at `path` in the property's own scale and storage, or refuses it: then nothing
    /// is sent.
    async fn write_quantity(&self, path: &str, q: DynQuantity) -> Result<(), String> {
        let r = self.get(path);
        let value = q.to_value(&property_ty(&r), self.bundle())?;
        r.write(self.client.cmd(), &value).await.unwrap();
        Ok(())
    }

    async fn requests(&self, shared: &devices::Shared) -> u32 {
        shared.lock().unwrap().requests
    }
}

fn property_ty(r: &DynResource) -> TypeOwned {
    match &r.item().unwrap().kind {
        ApiItemKindOwned::Property { ty, .. } => ty.clone(),
        k => panic!("not a property: {k:?}"),
    }
}

fn arg_tys(r: &DynResource) -> Vec<TypeOwned> {
    match &r.item().unwrap().kind {
        ApiItemKindOwned::Method { args, .. } => args.iter().map(|a| a.ty.clone()).collect(),
        k => panic!("not a method: {k:?}"),
    }
}

fn return_ty(r: &DynResource) -> TypeOwned {
    match &r.item().unwrap().kind {
        ApiItemKindOwned::Method { return_ty, .. } => return_ty.clone().unwrap(),
        k => panic!("not a method: {k:?}"),
    }
}

fn q(prefix: Prefix, value: i64, unit: &str) -> DynQuantity {
    DynQuantity::new(
        prefix,
        NumericValue::I64(value),
        DynUnit::from_name(unit).unwrap(),
    )
}

fn fields(s: &ItemStructOwned) -> &[FieldOwned] {
    match &s.fields {
        FieldsOwned::Named(f) | FieldsOwned::Unnamed(f) => f,
        FieldsOwned::Unit => &[],
    }
}

/// The field `name` of a struct value, with its type.
fn field(
    v: &ValueOwned,
    ty: &TypeOwned,
    b: &ApiBundleOwned,
    name: &str,
) -> (ValueOwned, TypeOwned) {
    let (
        ValueOwned::Struct {
            fields: FieldsValueOwned::Named(values),
        },
        TypeOwned::Struct(s),
    ) = (v, ty.get_in_line(b).unwrap())
    else {
        panic!("not a struct: {v:?}");
    };
    let value = values.iter().find(|(n, _)| n == name).unwrap().1.clone();
    let ty = fields(s).iter().find(|f| f.ident.as_deref() == Some(name));
    (value, ty.unwrap().ty.clone())
}

/// `v` as a quantity: a `Fixed`, or a `Point` of one, whose one field is the `Fixed` (the dynamic client reads no
/// point as a quantity yet: the origin is left to the caller).
fn quantity(v: &ValueOwned, ty: &TypeOwned, b: &ApiBundleOwned) -> DynQuantity {
    if let Some(q) = DynQuantity::from_value(v, ty, b) {
        return q;
    }
    let (Some(inner), TypeOwned::Struct(s)) = (only_field(v), ty.get_in_line(b).unwrap()) else {
        panic!("not a quantity: {v:?}");
    };
    quantity(inner, &fields(s)[0].ty, b)
}

/// In 0.001 K, whatever step and storage the device sends.
fn milli_kelvin(q: &DynQuantity) -> i128 {
    assert_eq!(q.unit.name(), "Kelvin");
    (q.to_f64() * 1000.0).round() as i128
}

/// The value of a struct of one field (`Point`'s `coord`, `Seq`'s number).
fn only_field(v: &ValueOwned) -> Option<&ValueOwned> {
    match v {
        ValueOwned::Struct {
            fields: FieldsValueOwned::Unnamed(f),
        } if f.len() == 1 => Some(&f[0]),
        ValueOwned::Struct {
            fields: FieldsValueOwned::Named(f),
        } if f.len() == 1 => Some(&f[0].1),
        _ => None,
    }
}

fn number(v: &ValueOwned) -> i128 {
    match v {
        ValueOwned::Numeric(n) => n.to_i128().unwrap(),
        v => number(only_field(v).unwrap_or_else(|| panic!("not a number: {v:?}"))),
    }
}

fn unit_variant(name: &str) -> ValueOwned {
    ValueOwned::Enum {
        variant: name.into(),
        fields: FieldsValueOwned::Unit,
    }
}

/// Each item on the stream as samples `(seq, milli-kelvin)`: a `Sample`, or a `Batch` of temperatures from `first`.
fn samples(v: &ValueOwned, ty: &TypeOwned, b: &ApiBundleOwned) -> Vec<(i128, i128)> {
    let TypeOwned::Struct(s) = ty.get_in_line(b).unwrap() else {
        panic!("{ty:?}")
    };
    if fields(s)
        .iter()
        .any(|f| f.ident.as_deref() == Some("temps"))
    {
        let first = number(&field(v, ty, b, "first").0);
        let (ValueOwned::Vec(temps), TypeOwned::Vec(temp_ty)) = field(v, ty, b, "temps") else {
            panic!("temps")
        };
        temps
            .iter()
            .enumerate()
            .map(|(i, t)| (first + i as i128, milli_kelvin(&quantity(t, &temp_ty, b))))
            .collect()
    } else {
        let (temp, temp_ty) = field(v, ty, b, "temp");
        vec![(
            number(&field(v, ty, b, "seq").0),
            milli_kelvin(&quantity(&temp, &temp_ty, b)),
        )]
    }
}

/// `lines.rs`'s `exercise`, by name and introspection only. `wire` is only for the test's own checks of which way
/// each step went.
async fn exercise(d: &Device, shared: &devices::Shared, wire: WireVersion) {
    let line = wire.line.0;
    let b = d.bundle();
    let cmd = d.client.cmd();
    let continuous = unit_variant("Continuous");

    // `configure(Config)` where the device has it, else `set_rate` and the mode
    let rate = q(Prefix::Unit, 50, "Hertz");
    if let Some(configure) = d.find(&["configure"]) {
        let config_ty = &arg_tys(&configure)[0];
        let TypeOwned::Struct(s) = config_ty.get_in_line(b).unwrap() else {
            panic!()
        };
        let rate_ty = &fields(s).iter().next().unwrap().ty;
        let config = ValueOwned::Struct {
            fields: FieldsValueOwned::Named(vec![
                ("rate".into(), rate.to_value(rate_ty, b).unwrap()),
                ("mode".into(), continuous.clone()),
            ]),
        };
        d.call(&configure, &[config]).await;
    } else {
        let set_rate = d.get("set_rate");
        let rate = rate.to_value(&arg_tys(&set_rate)[0], b).unwrap();
        d.call(&set_rate, &[rate]).await;
        d.get("mode").write(cmd, &continuous).await.unwrap();
    }
    assert_eq!(shared.lock().unwrap().rate, 50);
    assert_eq!(d.read("mode").await.0, continuous);

    // the same temperature from every line, read from the type: 0.01 K in an i16 or 0.001 K in an i32
    let read_once = d.get("read_once");
    let s = d.call(&read_once, &[]).await.unwrap();
    let (temp, temp_ty) = field(&s, &return_ty(&read_once), b, "temp");
    assert_eq!(milli_kelvin(&quantity(&temp, &temp_ty, b)), 21_500);

    // whole steps on line 1 and hundredths on lines 2 and 3, from the type: 1.5 is refused before sending
    d.write_quantity("gain", q(Prefix::Unit, 2, "Ratio"))
        .await
        .unwrap();
    let (gain, gain_ty) = d.read("gain").await;
    assert_eq!(quantity(&gain, &gain_ty, b).to_f64(), 2.0);
    let before = d.requests(shared).await;
    let line_takes_hundredths = d
        .write_quantity("gain", q(Prefix::Centi, 150, "Ratio"))
        .await;
    match line_takes_hundredths {
        Ok(()) => {
            assert_ne!(line, 1);
            let (gain, gain_ty) = d.read("gain").await;
            assert_eq!(quantity(&gain, &gain_ty, b).to_f64(), 1.5);
        }
        Err(e) => {
            assert_eq!(line, 1);
            assert!(e.contains("Fixed"), "{e}");
            assert_eq!(d.requests(shared).await, before, "nothing sent");
        }
    }

    // an observed property, one subscription for the root level's +observe properties
    let (index, _, _) = d
        .root
        .observable_properties()
        .unwrap()
        .into_iter()
        .find(|(_, name, _)| name == "gain")
        .unwrap();
    let mut observer = d
        .root
        .observe(
            cmd,
            [(
                index,
                ObserveMode::OnChange {
                    min_us: 0u32.into(),
                },
            )]
            .into(),
        )
        .await
        .unwrap();
    d.write_quantity("gain", q(Prefix::Unit, 3, "Ratio"))
        .await
        .unwrap();
    loop {
        let update = observer.recv().await.unwrap();
        let gain = update.values.iter().find(|(n, _)| n == "gain");
        if gain.is_some_and(|(_, v)| quantity(v, &gain_ty, b).to_f64() == 3.0) {
            break;
        }
    }

    // a stream up and a sink down; `Mode::Triggered` where the device's enum has it
    let mut stream = d.get("samples").stream(cmd).await.unwrap();
    let mut sink = d.get("commands").sink(cmd).await.unwrap();
    let triggered = d.get("mode").write(cmd, &unit_variant("Triggered")).await;
    let mode_ty = property_ty(&d.get("mode"));
    let TypeOwned::Enum(mode) = mode_ty.get_in_line(b).unwrap() else {
        panic!()
    };
    let has_triggered = mode.variants.iter().any(|v| v.ident == "Triggered");
    assert_eq!(triggered.is_ok(), has_triggered, "{triggered:?}");
    assert_eq!(has_triggered, line == 1);
    let mark = ValueOwned::Enum {
        variant: "Mark".into(),
        fields: FieldsValueOwned::Named(vec![(
            "seq".into(),
            // a `Seq` is a struct of one field, its number
            ValueOwned::Struct {
                fields: FieldsValueOwned::Named(vec![(
                    "seq".into(),
                    ValueOwned::Numeric(NumericValue::U16(100)),
                )]),
            },
        )]),
    };
    sink.send(&mark).await.unwrap();
    sink.send(&unit_variant("Trigger")).await.unwrap();
    let item = stream.recv().await.unwrap();
    let got = samples(&item, &stream.ty().clone(), b);
    assert_eq!(got[0], (100, 21_500));
    assert_eq!(got.len(), if line == 3 { devices::w3::BURST } else { 1 });
    for (i, (seq, _)) in got.iter().enumerate() {
        assert_eq!(*seq, 100 + i as i128);
    }

    // an array of sub-levels
    let channel = |i| d.get("channel").index(i).unwrap();
    channel(2)
        .child("enabled")
        .unwrap()
        .write(cmd, &ValueOwned::Bool(false))
        .await
        .unwrap();
    let enabled = |i| async move {
        channel(i)
            .child("enabled")
            .unwrap()
            .read(cmd)
            .await
            .unwrap()
    };
    assert_eq!(enabled(2).await, ValueOwned::Bool(false));
    assert_eq!(enabled(1).await, ValueOwned::Bool(true));

    // an argument in another resolution: 0.25 K to every line, 0.001 K only where the type takes it
    if let Some(calibrate) = d.find(&["calibrate"]) {
        let offset_ty = &arg_tys(&calibrate)[0];
        let offset = q(Prefix::Milli, 250, "Kelvin")
            .to_value(offset_ty, b)
            .unwrap();
        d.call(&calibrate, &[offset]).await;
        let s = d.call(&read_once, &[]).await.unwrap();
        let (temp, temp_ty) = field(&s, &return_ty(&read_once), b, "temp");
        assert_eq!(milli_kelvin(&quantity(&temp, &temp_ty, b)), 21_750);
        let before = d.requests(shared).await;
        match q(Prefix::Milli, 1, "Kelvin").to_value(offset_ty, b) {
            Ok(fine) => {
                assert_ne!(line, 1);
                d.call(&calibrate, &[fine]).await;
                let s = d.call(&read_once, &[]).await.unwrap();
                let (temp, temp_ty) = field(&s, &return_ty(&read_once), b, "temp");
                assert_eq!(milli_kelvin(&quantity(&temp, &temp_ty, b)), 21_501);
            }
            Err(_) => {
                assert_eq!(line, 1);
                assert_eq!(d.requests(shared).await, before, "nothing sent")
            }
        }
    }

    // uptime at the root or in `health`, in whole seconds or in ms: 42 s wherever the device has it
    let uptime = d.find(&["uptime", "health.uptime"]);
    assert_eq!(uptime.is_some(), wire != WireVersion::new(1, 0));
    if let Some(uptime) = uptime {
        let up = uptime.read(cmd).await.unwrap();
        assert_eq!(quantity(&up, &property_ty(&uptime), b).to_f64(), 42.0);
    }

    // a method where the device has it, at the root or in `health`
    let self_test = d.find(&["self_test", "health.self_test"]);
    assert_eq!(self_test.is_some(), wire >= WireVersion::new(2, 1));
    if let Some(self_test) = self_test {
        assert_eq!(
            d.call(&self_test, &[]).await,
            Some(ValueOwned::Result(Ok(Box::new(ValueOwned::Tuple(vec![])))))
        );
    }

    // a ww_stdlib trait, the same path and types on every line
    let indication = d.get("indication.mode");
    indication
        .write(cmd, &unit_variant("OnlyEssential"))
        .await
        .unwrap();
    assert_eq!(
        indication.read(cmd).await.unwrap(),
        unit_variant("OnlyEssential")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_line_dynamically() {
    let _ = tracing_subscriber::fmt::try_init();
    let devices = [
        (
            devices::w1::start("thermo-dyn/w1", thermo_api_w1::THERMO_WIRE_VERSION),
            WireVersion::new(1, 2),
        ),
        (
            devices::w2::start("thermo-dyn/w2", thermo_api_w2::THERMO_WIRE_VERSION),
            WireVersion::new(2, 1),
        ),
        (
            devices::w3::start("thermo-dyn/w3", thermo_api_w3::THERMO_WIRE_VERSION),
            WireVersion::new(3, 0),
        ),
        (
            devices::w1_0::start("thermo-dyn/w1_0"),
            WireVersion::new(1, 0),
        ),
        (
            devices::w2_0::start("thermo-dyn/w2_0"),
            WireVersion::new(2, 0),
        ),
    ];
    for ((shared, device), wire) in &devices {
        let d = Device::connect(device).await;
        assert_eq!(d.client.device_api_info().user_api_wire, Some(*wire));
        exercise(&d, shared, *wire).await;
    }
}
