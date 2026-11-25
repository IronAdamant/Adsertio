//! A shared file lock, using the same system calls Chisel uses, and no crate.
//!
//! Unix takes `flock`. Windows takes `LockFileEx` on the first byte, which
//! overlaps both that one-byte lock and a whole-file lock. The lock is
//! non-blocking: if another process holds it, the caller hears so at once
//! and can report the evidence as unreadable. Nothing here creates a file.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// Holds a lock until dropped. Dropping unlocks, including on a panic.
#[must_use = "the lock is released when this guard is dropped"]
pub struct SharedGuard {
    file: File,
}

impl Drop for SharedGuard {
    fn drop(&mut self) {
        let _ = lock_fd(&self.file, Kind::Unlock);
    }
}

/// Open `path` (it must already exist) and take a shared lock. Does not
/// create the file and does not write to it.
pub fn try_shared(path: &Path) -> Result<SharedGuard, String> {
    let file = open_existing(path)?;
    lock_fd(&file, Kind::Shared).map_err(|e| lock_error(path, &e))?;
    Ok(SharedGuard { file })
}

/// Open `path` and take an exclusive lock. Used to prove a held lock is
/// visible to a reader. Does not create the file.
#[cfg(test)]
pub fn try_exclusive(path: &Path) -> Result<SharedGuard, String> {
    let file = open_existing(path)?;
    lock_fd(&file, Kind::Exclusive).map_err(|e| lock_error(path, &e))?;
    Ok(SharedGuard { file })
}

fn open_existing(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .or_else(|_| OpenOptions::new().read(true).open(path))
        .map_err(|e| format!("cannot open lock {}: {e}", path.display()))
}

fn lock_error(path: &Path, err: &io::Error) -> String {
    if contended(err) {
        format!(
            "cannot take a shared lock on {}: it is held",
            path.display()
        )
    } else {
        format!("cannot lock {}: {err}", path.display())
    }
}

fn contended(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::WouldBlock
        || matches!(
            err.raw_os_error(),
            // EAGAIN / EWOULDBLOCK on Linux and macOS, and the Windows lock errors.
            Some(11 | 35 | 33 | 36 | 167)
        )
}

enum Kind {
    Shared,
    #[cfg(test)]
    Exclusive,
    Unlock,
}

fn lock_fd(file: &File, kind: Kind) -> io::Result<()> {
    #[cfg(unix)]
    {
        unix_flock(file, kind)
    }
    #[cfg(windows)]
    {
        windows_lock(file, kind)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (file, kind);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "file locking is not implemented on this platform",
        ))
    }
}

#[cfg(unix)]
fn unix_flock(file: &File, kind: Kind) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;
    const LOCK_SH: i32 = 1;
    #[cfg(test)]
    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;
    const LOCK_UN: i32 = 8;
    extern "C" {
        fn flock(fd: i32, op: i32) -> i32;
    }
    let op = match kind {
        Kind::Shared => LOCK_SH | LOCK_NB,
        #[cfg(test)]
        Kind::Exclusive => LOCK_EX | LOCK_NB,
        Kind::Unlock => LOCK_UN,
    };
    loop {
        // SAFETY: `fd` is a live descriptor of `file`, and `op` is one of the
        // flock operations above. The call does not touch Rust memory.
        let rc = unsafe { flock(file.as_raw_fd(), op) };
        if rc == 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(4) {
            continue; // EINTR
        }
        return Err(err);
    }
}

#[cfg(windows)]
fn windows_lock(file: &File, kind: Kind) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    const LOCKFILE_FAIL_IMMEDIATELY: u32 = 0x0000_0001;
    #[cfg(test)]
    const LOCKFILE_EXCLUSIVE_LOCK: u32 = 0x0000_0002;
    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        event: isize,
    }
    extern "system" {
        fn LockFileEx(
            file: isize,
            flags: u32,
            reserved: u32,
            low: u32,
            high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
        fn UnlockFileEx(
            file: isize,
            reserved: u32,
            low: u32,
            high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
    }
    let handle = file.as_raw_handle() as isize;
    let mut ov = Overlapped {
        internal: 0,
        internal_high: 0,
        offset: 0,
        offset_high: 0,
        event: 0,
    };
    // One byte at the start. Chisel locks that byte. A whole-file lock, which
    // is what a Rust `File::lock` takes, overlaps it, so either writer blocks us.
    let rc = match kind {
        Kind::Shared => unsafe { LockFileEx(handle, LOCKFILE_FAIL_IMMEDIATELY, 0, 1, 0, &mut ov) },
        #[cfg(test)]
        Kind::Exclusive => unsafe {
            LockFileEx(
                handle,
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                1,
                0,
                &mut ov,
            )
        },
        Kind::Unlock => unsafe { UnlockFileEx(handle, 0, 1, 0, &mut ov) },
    };
    if rc != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn a_missing_lock_is_not_created() {
        let dir = std::env::temp_dir().join(format!("adsertio-lock-miss-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lock");
        assert!(try_shared(&path).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn an_exclusive_lock_blocks_a_shared_one() {
        let dir = std::env::temp_dir().join(format!("adsertio-lock-held-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lock");
        fs::write(&path, b"").unwrap();
        let held = try_exclusive(&path).unwrap();
        let second = try_shared(&path);
        drop(held);
        match second {
            Err(message) => assert!(
                message.contains("held") || message.contains("lock"),
                "{message}"
            ),
            Ok(guard) => {
                // Some kernels treat flock as per-process, so a second
                // descriptor in this process does not conflict. A child
                // process must, and the reader reports that as unreadable.
                drop(guard);
                assert!(
                    child_cannot_share(&path),
                    "a second process took a shared lock while this one held it"
                );
            }
        }
    }

    fn child_cannot_share(path: &Path) -> bool {
        let ready = path.with_extension("ready");
        let release = path.with_extension("release");
        let _ = fs::remove_file(&ready);
        let _ = fs::remove_file(&release);
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("lock::tests::child_holds_exclusive")
            .arg("--exact")
            .env("ADSERTIO_HOLD_LOCK", path)
            .env("ADSERTIO_HOLD_READY", &ready)
            .env("ADSERTIO_HOLD_RELEASE", &release)
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !ready.exists() {
            if std::time::Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let blocked = try_shared(path).is_err();
        fs::write(&release, b"1").unwrap();
        let _ = child.wait();
        blocked
    }

    #[test]
    fn child_holds_exclusive() {
        let Ok(path) = std::env::var("ADSERTIO_HOLD_LOCK") else {
            return;
        };
        let ready = std::env::var("ADSERTIO_HOLD_READY").unwrap();
        let release = std::env::var("ADSERTIO_HOLD_RELEASE").unwrap();
        let guard = try_exclusive(Path::new(&path)).unwrap();
        fs::write(&ready, b"1").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !Path::new(&release).exists() {
            if std::time::Instant::now() > deadline {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        drop(guard);
    }
}
