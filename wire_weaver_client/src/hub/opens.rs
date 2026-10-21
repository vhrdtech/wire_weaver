//! The open counter (CLT-39): every open of a device in [HubMode::Auto](super::HubMode::Auto) leaves a line in
//! `<runtime>/ww/opens/<key>`, `<unix ms> <weight>`, next to the holders' lock files. When the weights within the
//! window reach [OpenCounter::opens](super::OpenCounter::opens), the device goes into the hub.

use std::fs::OpenOptions;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use super::OpenCounter;

/// What [record_open] counted.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Opens {
    /// Weights of the opens within the window, this one included
    pub weight: f32,
    /// [Opens::weight] reached the counter's limit
    pub tripped: bool,
}

/// Records an open of `key` with `weight` (1 for a shell-launched call, 1/4 otherwise) and says whether the opens
/// within `counter`'s window trip it. Lines older than the window are dropped; the file is rewritten under an OS
/// lock, so programs opening at once all count.
pub fn record_open(key: &str, weight: f32, counter: OpenCounter) -> io::Result<Opens> {
    let dir = ww_local_socket::runtime_dir()?.join(ww_local_socket::files::OPENS_DIR);
    match std::fs::create_dir(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let path = dir.join(crate::holders::sanitize_file_name(key));
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    // held for a read and a write of a few lines
    file.lock()?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let since = now.saturating_sub(counter.window.as_millis() as u64);
    let mut kept: Vec<(u64, f32)> = content
        .lines()
        .filter_map(|line| {
            let (t, w) = line.split_once(' ')?;
            Some((t.parse().ok()?, w.parse().ok()?))
        })
        // a clock set back leaves lines "in the future": they age out once it catches up
        .filter(|(t, w): &(u64, f32)| *t >= since && w.is_finite() && *w >= 0.0)
        .collect();
    kept.push((now, weight));
    let mut out = String::new();
    for (t, w) in &kept {
        out.push_str(&format!("{t} {w}\n"));
    }
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(out.as_bytes())?;
    file.flush()?;
    let total: f32 = kept.iter().map(|(_, w)| w).sum();
    Ok(Opens {
        weight: total,
        // 1/4 + 1/4 + ... in f32 is exact, no epsilon needed
        tripped: total >= counter.opens,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn shell_calls_trip_on_the_third_gui_restarts_dont() {
        crate::hub::start::tests::with_runtime_dir(|_| {
            let c = OpenCounter::default();
            let cli = |k| record_open(k, 1.0, c).unwrap().tripped;
            assert!(!cli("usb:1:2:cli"));
            assert!(!cli("usb:1:2:cli"));
            assert!(cli("usb:1:2:cli"), "third shell call in a minute");
            let gui = |k| record_open(k, 0.25, c).unwrap();
            for _ in 0..3 {
                assert!(!gui("usb:1:2:gui").tripped);
            }
            assert_eq!(gui("usb:1:2:gui").weight, 1.0);
        });
    }

    #[test]
    fn old_opens_age_out() {
        crate::hub::start::tests::with_runtime_dir(|_| {
            let c = OpenCounter {
                opens: 2.0,
                window: Duration::from_millis(50),
            };
            assert!(!record_open("usb:1:2:old", 1.0, c).unwrap().tripped);
            std::thread::sleep(Duration::from_millis(80));
            assert!(!record_open("usb:1:2:old", 1.0, c).unwrap().tripped);
            assert!(record_open("usb:1:2:old", 1.0, c).unwrap().tripped);
        });
    }
}
