//! Ticket 07: a second Hexagon instance must not acquire the same physical desktop.
//! The kernel owns this lease until the last descriptor closes, including a crash.
#[cfg(unix)]
pub(super) fn acquire() -> Result<std::fs::File, String> {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("user runtime directory unavailable")?;
    acquire_at(&home.join(".hexagon-desktop-runtime"))
}

#[cfg(unix)]
pub(super) fn acquire_at(directory: &std::path::Path) -> Result<std::fs::File, String> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    match std::fs::DirBuilder::new().mode(0o700).create(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.to_string()),
    }
    let dir = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(directory)
        .map_err(|_| "unsafe desktop runtime directory")?;
    let metadata = dir.metadata().map_err(|error| error.to_string())?;
    let uid = unsafe { libc::geteuid() };
    if metadata.uid() != uid || metadata.mode() & 0o777 != 0o700 {
        return Err("desktop runtime directory must be private and owned by this user".into());
    }
    // openat pins the verified directory even if its pathname changes. Never
    // truncate an existing lock file, follow a link, or use a project-local lock.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            c"desktop.lock".as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            0o600,
        )
    };
    if fd < 0 {
        return Err("cannot open desktop lease safely".into());
    }
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file()
        || metadata.uid() != uid
        || metadata.nlink() != 1
        || metadata.mode() & 0o777 != 0o600
    {
        return Err("unsafe desktop lease file".into());
    }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("another Hexagon process controls this computer".into());
    }
    Ok(file)
}

#[cfg(not(unix))]
pub(super) fn acquire() -> Result<std::fs::File, String> {
    Err("macOS is required".into())
}
