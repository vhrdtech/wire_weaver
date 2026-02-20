use driver::{DeviceFilter, LedState, MyDeviceDriver, OnError};
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use tokio::runtime::Runtime;

#[pymodule]
fn blinky(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Device>()?;
    Ok(())
}

#[pyclass]
struct Device {
    runtime: Runtime,
    drv: Option<MyDeviceDriver>,
}

#[pymethods]
impl DriverPy {
    #[new]
    fn new() -> PyResult<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        Ok(DriverPy { runtime, drv: None })
    }

    pub fn connect(&mut self) -> PyResult<()> {
        let _guard = self.runtime.enter();
        let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
        match MyDeviceDriver::connect_blocking(filter, OnError::ExitImmediately) {
            Ok(d) => {
                self.drv = Some(d);
                Ok(())
            }
            Err(e) => Err(PyTypeError::new_err(format!("Failed to connect: {:?}", e))),
        }
    }

    pub fn led_on(&mut self) -> PyResult<()> {
        let Some(drv) = &mut self.drv else {
            return Err(PyTypeError::new_err("Not connected, use connect() first"));
        };
        drv.set_led_state(LedState::On)
            .blocking_call()
            .map_err(|e| PyTypeError::new_err(e.to_string()))?;
        Ok(())
    }

    pub fn led_off(&mut self) -> PyResult<()> {
        let Some(drv) = &mut self.drv else {
            return Err(PyTypeError::new_err("Not connected, use connect() first"));
        };
        drv.set_led_state(LedState::Off)
            .blocking_call()
            .map_err(|e| PyTypeError::new_err(e.to_string()))?;
        Ok(())
    }
}
