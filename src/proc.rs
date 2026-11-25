//! Whether a process id is still running. Used to drop an attach lock or a
//! temp file left behind by a killed writer. No crate: `kill(pid, 0)` on Unix
//! and `OpenProcess` on Windows.

/// `true` when `pid` is a live process. A pid of 0 is never live. An error
/// other than "no such process" is treated as live, so a lock is not stolen
/// from a process we cannot query.
pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        unix_alive(pid)
    }
    #[cfg(windows)]
    {
        windows_alive(pid)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        true
    }
}

#[cfg(unix)]
fn unix_alive(pid: u32) -> bool {
    if pid > i32::MAX as u32 {
        return false;
    }
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    // SAFETY: pid is a process id, signal 0 does not deliver a signal. It
    // only checks that the process exists.
    let rc = unsafe { kill(pid as i32, 0) };
    if rc == 0 {
        return true;
    }
    // ESRCH is 3 on Linux and on macOS. EPERM means the process exists.
    std::io::Error::last_os_error().raw_os_error() != Some(3)
}

#[cfg(windows)]
fn windows_alive(pid: u32) -> bool {
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn GetExitCodeProcess(process: isize, code: *mut u32) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }
    // SAFETY: the handle comes from OpenProcess and is closed on every path
    // that received one. The exit-code pointer is a live local.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle == 0 {
        // Access denied: the process exists and we may not query it.
        return std::io::Error::last_os_error().raw_os_error() == Some(5);
    }
    let mut code = 0u32;
    let ok = unsafe { GetExitCodeProcess(handle, &mut code) };
    unsafe { CloseHandle(handle) };
    ok != 0 && code == STILL_ACTIVE
}

/// The first whitespace-separated token, when it is a pid.
pub fn leading_pid(text: &str) -> Option<u32> {
    let token = text.split_whitespace().next()?;
    let pid: u32 = token.parse().ok()?;
    if pid == 0 {
        None
    } else {
        Some(pid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_alive_and_a_huge_pid_is_not() {
        assert!(pid_alive(std::process::id()));
        assert!(!pid_alive(0));
        assert!(!pid_alive(u32::MAX));
    }
}
