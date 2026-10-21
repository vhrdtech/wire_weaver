use std::io;
use std::path::PathBuf;

/// Overrides the runtime directory, e.g. for tests that run their own hub: the `ww` directory is created inside it.
pub const RUNTIME_DIR_ENV: &str = "WW_RUNTIME_DIR";

/// The per-user directory for sockets and lock files, `ww` inside the OS's runtime directory:
/// `$XDG_RUNTIME_DIR/ww` on Linux (a per-user tmpfs; `/tmp/ww-<uid>` when it is not set), `$TMPDIR/ww` on macOS
/// (per user), `%LOCALAPPDATA%\ww\run` on Windows. [RUNTIME_DIR_ENV] takes precedence.
///
/// Created with mode 0700 on Unix. An existing directory must belong to the current user, else this fails rather
/// than put a socket where another user could reach or replace it; one of ours with looser permissions is
/// tightened.
pub fn runtime_dir() -> io::Result<PathBuf> {
    let dir = base_dir()?;
    create_private(&dir)?;
    Ok(dir)
}

fn base_dir() -> io::Result<PathBuf> {
    if let Some(dir) = std::env::var_os(RUNTIME_DIR_ENV).filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir).join("ww"));
    }
    platform_base_dir()
}

#[cfg(target_os = "linux")]
fn platform_base_dir() -> io::Result<PathBuf> {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        Some(dir) => Ok(PathBuf::from(dir).join("ww")),
        // SAFETY: geteuid has no preconditions and cannot fail
        None => Ok(std::env::temp_dir().join(format!("ww-{}", unsafe { libc::geteuid() }))),
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn platform_base_dir() -> io::Result<PathBuf> {
    // macOS: $TMPDIR is per user (/var/folders/..), std falls back to /tmp without it, then the uid keeps it apart
    let tmp = std::env::temp_dir();
    if std::env::var_os("TMPDIR").is_some() {
        Ok(tmp.join("ww"))
    } else {
        // SAFETY: geteuid has no preconditions and cannot fail
        Ok(tmp.join(format!("ww-{}", unsafe { libc::geteuid() })))
    }
}

#[cfg(windows)]
fn platform_base_dir() -> io::Result<PathBuf> {
    // %LOCALAPPDATA% is in the user's profile, only the user (and admins) can read it
    let local = std::env::var_os("LOCALAPPDATA")
        .filter(|d| !d.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not set"))?;
    Ok(PathBuf::from(local).join("ww").join("run"))
}

#[cfg(unix)]
fn create_private(dir: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    // symlink_metadata: a symlink planted in a shared /tmp must not lead elsewhere
    let meta = std::fs::symlink_metadata(dir)?;
    // SAFETY: geteuid has no preconditions and cannot fail
    let me = unsafe { libc::geteuid() };
    if !meta.is_dir() || meta.uid() != me {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} is not a directory of this user (uid {me}), refusing to put sockets there",
                dir.display()
            ),
        ));
    }
    if meta.mode() & 0o077 != 0 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(windows)]
fn create_private(dir: &std::path::Path) -> io::Result<()> {
    // inherits the profile's ACL (the user, SYSTEM and Administrators)
    std::fs::create_dir_all(dir)
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn created_private_and_tightened() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("ww-dir-test-{}", std::process::id()));
        let dir = base.join("ww");
        super::create_private(&dir).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        super::create_private(&dir).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_refused() {
        let base = std::env::temp_dir().join(format!("ww-dir-link-{}", std::process::id()));
        std::fs::create_dir_all(base.join("elsewhere")).unwrap();
        std::os::unix::fs::symlink(base.join("elsewhere"), base.join("ww")).unwrap();
        let e = super::create_private(&base.join("ww")).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
        std::fs::remove_dir_all(&base).unwrap();
    }
}
