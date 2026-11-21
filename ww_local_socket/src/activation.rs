//! Socket activation: a listening socket the service manager (systemd's `.socket` units, `systemd-socket-activate`)
//! created and passed to this process, `LISTEN_PID`, `LISTEN_FDS` and `LISTEN_FDNAMES` as `sd_listen_fds(3)`
//! describes them.

use std::fmt;
use std::io;
use std::os::fd::{FromRawFd, OwnedFd, RawFd};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::socket::Listener;

/// The first passed descriptor, `SD_LISTEN_FDS_START`.
const LISTEN_FDS_START: RawFd = 3;
/// Passed descriptors this process takes at most: one bit each in [TAKEN].
const LISTEN_FDS_MAX: usize = 64;
/// Longest `LISTEN_FDNAMES` read.
const LISTEN_FDNAMES_LEN_MAX: usize = 16 * 1024;

/// The passed descriptors already made a [Listener], one bit per index: each is owned once per process, however
/// many callers ask for it.
static TAKEN: AtomicU64 = AtomicU64::new(0);

/// Why [inherited] gave no listener.
#[derive(Debug)]
pub enum ActivationError {
    /// No socket was passed to this process: `LISTEN_FDS` is not set, or `LISTEN_PID` names another process.
    NotActivated,
    /// `LISTEN_PID`, `LISTEN_FDS` or `LISTEN_FDNAMES` is not what `sd_listen_fds(3)` describes.
    Malformed(&'static str),
    /// No passed socket has this name (`FileDescriptorName=` in the `.socket` unit).
    NoSuchName(String),
    /// The socket was already taken by an earlier call.
    Taken,
    /// The descriptor is not a listening Unix stream socket, or the runtime refused it.
    NotAListener(io::Error),
}

impl fmt::Display for ActivationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActivationError::NotActivated => f.write_str("no socket was passed to this process"),
            ActivationError::Malformed(var) => write!(f, "{var} is malformed"),
            ActivationError::NoSuchName(name) => write!(f, "no passed socket is named {name:?}"),
            ActivationError::Taken => f.write_str("the passed socket was already taken"),
            ActivationError::NotAListener(e) => {
                write!(
                    f,
                    "the passed descriptor is not a listening Unix socket: {e}"
                )
            }
        }
    }
}

impl std::error::Error for ActivationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ActivationError::NotAListener(e) => Some(e),
            _ => None,
        }
    }
}

/// The listening Unix socket the service manager passed to this process: the one named `name`
/// (`FileDescriptorName=`), or the first one. Accepting checks the peer's user as [listen](crate::listen) does;
/// the socket file and its mode are the `.socket` unit's (`SocketMode=0600`).
///
/// Each passed socket is taken once per process: a second call for it is [ActivationError::Taken]. Programs this
/// process starts don't inherit it. Must be called inside a tokio runtime, which the listener registers with.
///
/// # Errors
///
/// [ActivationError::NotActivated] when the process was started without a socket, so that a program can
/// [listen](crate::listen) by itself then.
pub fn inherited(name: Option<&str>) -> Result<Listener, ActivationError> {
    let var = |key: &str| std::env::var(key).ok();
    let passed = passed(
        var("LISTEN_PID").as_deref(),
        var("LISTEN_FDS").as_deref(),
        var("LISTEN_FDNAMES").as_deref(),
        std::process::id(),
    )?;
    let index = pick(&passed, name)?;
    let bit = 1u64 << index;
    if TAKEN.fetch_or(bit, Ordering::AcqRel) & bit != 0 {
        return Err(ActivationError::Taken);
    }
    let fd = LISTEN_FDS_START + RawFd::try_from(index).expect("index is below LISTEN_FDS_MAX");
    check_listener(fd).map_err(ActivationError::NotAListener)?;
    // SAFETY: `fd` is open (check_listener's fcntl), the service manager passed it to this process (LISTEN_PID),
    // and TAKEN makes this the only owner this process creates for it.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let uds = interprocess::os::unix::uds_local_socket::tokio::Listener::try_from(fd)
        .map_err(ActivationError::NotAListener)?;
    Ok(Listener::from_inner(
        interprocess::local_socket::tokio::Listener::UdSocket(uds),
    ))
}

/// The names of the passed descriptors, `None` for each without one; [ActivationError::NotActivated] unless `pid`
/// is `me`.
fn passed(
    pid: Option<&str>,
    fds: Option<&str>,
    names: Option<&str>,
    me: u32,
) -> Result<Vec<Option<String>>, ActivationError> {
    let (Some(pid), Some(fds)) = (pid, fds) else {
        return Err(ActivationError::NotActivated);
    };
    let pid: u32 = pid
        .parse()
        .map_err(|_| ActivationError::Malformed("LISTEN_PID"))?;
    if pid != me {
        return Err(ActivationError::NotActivated);
    }
    let count: usize = fds
        .parse()
        .map_err(|_| ActivationError::Malformed("LISTEN_FDS"))?;
    if count == 0 {
        return Err(ActivationError::NotActivated);
    }
    if count > LISTEN_FDS_MAX {
        return Err(ActivationError::Malformed("LISTEN_FDS"));
    }
    let Some(names) = names else {
        return Ok(vec![None; count]);
    };
    if names.len() > LISTEN_FDNAMES_LEN_MAX {
        return Err(ActivationError::Malformed("LISTEN_FDNAMES"));
    }
    let names: Vec<Option<String>> = names
        .split(':')
        .map(|name| (!name.is_empty()).then(|| name.to_owned()))
        .collect();
    if names.len() != count {
        return Err(ActivationError::Malformed("LISTEN_FDNAMES"));
    }
    Ok(names)
}

/// The index of the descriptor named `name`, or the first one.
fn pick(passed: &[Option<String>], name: Option<&str>) -> Result<usize, ActivationError> {
    let Some(name) = name else {
        return Ok(0);
    };
    passed
        .iter()
        .position(|n| n.as_deref() == Some(name))
        .ok_or_else(|| ActivationError::NoSuchName(name.to_owned()))
}

/// `fd` is an open, listening Unix socket; it is made close-on-exec, so that programs started later don't hold it.
fn check_listener(fd: RawFd) -> io::Result<()> {
    let not = |what: &str| io::Error::new(io::ErrorKind::InvalidInput, what.to_owned());
    // SAFETY: fcntl, getsockname and getsockopt only read the descriptor table and write the buffers given here,
    // each sized by the length passed with it.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags == -1 {
            return Err(io::Error::last_os_error());
        }
        if libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) == -1 {
            return Err(io::Error::last_os_error());
        }
        let mut addr: libc::sockaddr_storage = std::mem::zeroed();
        let mut len = libc::socklen_t::try_from(size_of::<libc::sockaddr_storage>())
            .expect("sockaddr_storage fits socklen_t");
        if libc::getsockname(fd, (&raw mut addr).cast(), &mut len) == -1 {
            return Err(io::Error::last_os_error());
        }
        if libc::c_int::from(addr.ss_family) != libc::AF_UNIX {
            return Err(not("not a Unix socket"));
        }
        let mut accepting: libc::c_int = 0;
        let mut len =
            libc::socklen_t::try_from(size_of::<libc::c_int>()).expect("c_int fits socklen_t");
        if libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_ACCEPTCONN,
            (&raw mut accepting).cast(),
            &mut len,
        ) == -1
        {
            return Err(io::Error::last_os_error());
        }
        if accepting == 0 {
            return Err(not("not listening"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_socket_passed_to_this_process_is_found() {
        let passed = passed(Some("42"), Some("1"), None, 42).unwrap();
        assert_eq!(passed, vec![None]);
        assert_eq!(pick(&passed, None).unwrap(), 0);
    }

    #[test]
    fn sockets_passed_to_another_process_are_not_taken() {
        assert!(matches!(
            passed(Some("41"), Some("1"), None, 42),
            Err(ActivationError::NotActivated)
        ));
        assert!(matches!(
            passed(None, None, None, 42),
            Err(ActivationError::NotActivated)
        ));
        assert!(matches!(
            passed(Some("42"), Some("0"), None, 42),
            Err(ActivationError::NotActivated)
        ));
    }

    #[test]
    fn a_socket_is_picked_by_its_name() {
        let passed = passed(Some("42"), Some("3"), Some("api::ww"), 42).unwrap();
        assert_eq!(
            passed,
            vec![Some("api".to_owned()), None, Some("ww".to_owned())]
        );
        assert_eq!(pick(&passed, Some("ww")).unwrap(), 2);
        assert!(matches!(
            pick(&passed, Some("web")),
            Err(ActivationError::NoSuchName(_))
        ));
    }

    #[test]
    fn malformed_variables_are_refused() {
        for (pid, fds, names) in [
            ("x", "1", None),
            ("42", "-1", None),
            ("42", "65", None),
            ("42", "2", Some("one")),
        ] {
            assert!(
                matches!(
                    passed(Some(pid), Some(fds), names, 42),
                    Err(ActivationError::Malformed(_))
                ),
                "{pid} {fds} {names:?}"
            );
        }
    }
}
