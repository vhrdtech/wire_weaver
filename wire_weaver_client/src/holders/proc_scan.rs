//! Naming the holder of a device nothing in [`super`] knows about: a non-WireWeaver program (probe-rs's own
//! CLI, `minicom`, `dfu-util`) never writes a lock file, so the only way to say who it is comes from the OS. On
//! Linux that's `/proc/*/fd`: every process with the device node open has a symlink to it there. Other platforms
//! get no name, just "another program" (see `with_hint` in `usb/connect.rs`).

use std::path::{Path, PathBuf};

/// Describes whichever process holds `node` open, e.g. `"openocd (pid 4711)"`. `None` when nothing is found (the
/// holder exited between the busy error and this scan, or `/proc/<pid>/fd` isn't readable by us) or on a
/// platform other than Linux.
#[cfg(target_os = "linux")]
pub(crate) fn describe_holder_of_node(node: &Path) -> Option<String> {
    let target = std::fs::canonicalize(node).unwrap_or_else(|_| node.to_path_buf());
    let self_pid = std::process::id();
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == self_pid {
            continue;
        }
        let Ok(fds) = std::fs::read_dir(entry.path().join("fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if std::fs::read_link(fd.path()).ok().as_deref() == Some(target.as_path()) {
                let name = process_name(pid).unwrap_or_else(|| "unknown".to_string());
                return Some(format!("{name} (pid {pid})"));
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn process_name(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim().to_string())
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn describe_holder_of_node(_node: &Path) -> Option<String> {
    None
}

/// Linux device node for a USB device, from the bus and device numbers `nusb::DeviceInfo` reports
/// (`busnum()`/`device_address()`): `/dev/bus/usb/BBB/DDD`.
#[cfg(target_os = "linux")]
pub(crate) fn usb_device_node(busnum: u8, device_address: u8) -> PathBuf {
    PathBuf::from(format!("/dev/bus/usb/{busnum:03}/{device_address:03}"))
}

/// Names whichever process holds the USB device identified by `vid:pid` (and `serial`, to tell apart several
/// devices sharing that pair) open: used for a busy error coming from a non-WireWeaver program attached to a
/// debug probe's raw USB device, which has no lock file of its own to read. Linux only; `None` elsewhere, or
/// when no matching device or holder is found.
pub fn holder_of_usb_device(vid: u16, pid: u16, serial: Option<&str>) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        use nusb::MaybeFuture;
        let devices = nusb::list_devices().wait().ok()?;
        let dev = devices
            .filter(|d| d.vendor_id() == vid && d.product_id() == pid)
            .find(|d| serial.is_none_or(|s| d.serial_number() == Some(s)))?;
        describe_holder_of_node(&usb_device_node(dev.busnum(), dev.device_address()))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (vid, pid, serial);
        None
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::fs::File;

    /// A real (non-USB, non-WireWeaver) process holding a node open, the exact situation the busy error falls
    /// back to this scan for: `sleep`'s stdin is redirected from our test file, so `/proc/<its pid>/fd/0` is a
    /// link to it, the same shape as a probe's `/dev/bus/usb/BBB/DDD`.
    #[test]
    fn finds_the_process_holding_a_node_open() {
        let path = std::env::temp_dir().join(format!(
            "ww_proc_scan_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"x").unwrap();
        let stdin = File::open(&path).unwrap();
        let mut child = std::process::Command::new("sleep")
            .arg("5")
            .stdin(stdin)
            .spawn()
            .expect("spawn sleep");

        let found = (0..50)
            .find_map(|_| {
                let found = describe_holder_of_node(&path);
                if found.is_none() {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                found
            })
            .unwrap_or_default();
        child.kill().ok();
        child.wait().ok();
        let _ = std::fs::remove_file(&path);

        assert!(found.contains("sleep"), "expected sleep in {found:?}");
        assert!(
            found.contains(&child.id().to_string()),
            "expected pid {} in {found:?}",
            child.id()
        );
    }

    #[test]
    fn usb_device_node_format() {
        assert_eq!(
            usb_device_node(1, 7).to_str().unwrap(),
            "/dev/bus/usb/001/007"
        );
    }
}
