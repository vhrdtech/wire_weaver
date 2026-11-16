//! `ww.Opaque`: the payload of a value whose type this host has no definition of (EVO-80).

use pyo3::prelude::*;
use pyo3::types::PyBytes;
use wire_weaver_client::ww_self::OpaqueOwned;

/// A value the client couldn't decode: its type is defined in a crate whose API snapshot this host doesn't have
/// (`crate` at `version`), so the payload is given as received. `ww api fetch CRATE VERSION`, or `ww api cache DIR`
/// over a checkout of the crate, puts the snapshot into the cache; the value then decodes as usual. An `Opaque`
/// can't be written into a property or passed as an argument.
#[pyclass(module = "wire_weaver", frozen)]
pub(crate) struct Opaque {
    crate_id: String,
    version: String,
    name: String,
    signature: Vec<u8>,
    bytes: Vec<u8>,
}

impl Opaque {
    /// The Python object of a `ValueOwned::Opaque`.
    pub(crate) fn of(opaque: &OpaqueOwned) -> Self {
        let v = &opaque.crate_version.version;
        Opaque {
            crate_id: opaque.crate_version.crate_id.clone(),
            version: format!("{}.{}.{}", v.major.0, v.minor.0, v.patch.0),
            name: opaque.type_name.clone(),
            signature: opaque.signature.clone(),
            bytes: opaque.bytes.clone(),
        }
    }
}

#[pymethods]
impl Opaque {
    /// The crate that defines the type: `"ww_adc"`.
    #[getter]
    #[pyo3(name = "crate")]
    fn crate_id(&self) -> &str {
        &self.crate_id
    }

    /// The crate's version: `"0.3.0"`.
    #[getter]
    fn version(&self) -> &str {
        &self.version
    }

    /// The type's name in its crate: `"Reading"`, `"units::Volt"`.
    #[getter]
    fn name(&self) -> &str {
        &self.name
    }

    /// The signature the device named the definition by.
    #[getter]
    fn signature<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.signature)
    }

    /// The payload as received, undecoded.
    #[getter]
    fn bytes<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.bytes)
    }

    fn __repr__(&self) -> String {
        let hex: String = self.bytes.iter().map(|b| format!("{b:02x}")).collect();
        format!(
            "Opaque({}::{} {}, bytes=0x{hex}: no snapshot of {} {}, see `ww api fetch`)",
            self.crate_id, self.name, self.version, self.crate_id, self.version
        )
    }
}
