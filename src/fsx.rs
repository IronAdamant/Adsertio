//! File helpers: atomic writes with cleanup, strict reads, path guards.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Removes its file on drop unless disarmed, so a failed or panicking write
/// never leaves a temp file behind.
struct TempGuard {
    path: PathBuf,
    armed: bool,
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(5);
const LOCK_OWN_PID: u64 = 30;
const LOCK_GRACE: u64 = 1;
const TEMP_ABANDON: u64 = 30;
/// A file time a couple of seconds ahead is clock skew, not a lock planted
/// in the future. Farther than this, and no live pid, the lock is not held.
const CLOCK_SKEW: std::time::Duration = std::time::Duration::from_secs(2);

struct HeldLock {
    path: PathBuf,
    /// The line this holder wrote. The name is removed only while it still
    /// holds this line, so a lock another writer took over is left alone.
    text: String,
}

impl Drop for HeldLock {
    fn drop(&mut self) {
        if fs::read_to_string(&self.path).is_ok_and(|t| t == self.text) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// How long a `.break` guard may stay before it counts as abandoned.
/// Breaking takes microseconds; only a writer killed mid-break leaves one.
const BREAK_STALE: u64 = 5;

/// Remove `lock` when no live writer holds it, and say whether it was removed.
/// Only the holder of the `.break` guard beside it removes, after judging the
/// lock again while holding the guard. The lock name cannot be created while
/// the old file is there, so the file judged abandoned is the file removed: a
/// lock taken a moment after another contender's check is never deleted.
pub fn break_abandoned_lock(lock: &Path) -> bool {
    if !lock.exists() || lock_held(lock) {
        return false;
    }
    let mut guard = lock.as_os_str().to_owned();
    guard.push(".break");
    let guard = PathBuf::from(guard);
    let held = match OpenOptions::new().write(true).create_new(true).open(&guard) {
        Ok(file) => file,
        Err(_) => {
            if matches!(file_age(&guard), Age::Secs(secs) if secs > BREAK_STALE) {
                let _ = fs::remove_file(&guard);
            }
            return false;
        }
    };
    let removed = lock.exists() && !lock_held(lock) && fs::remove_file(lock).is_ok();
    drop(held);
    let _ = fs::remove_file(&guard);
    removed
}

/// Hold an exclusive lock file for a read-modify-write.
///
/// The lock is an atomic `create_new`. The file's first line is this process
/// id. A lock whose pid is not running is removed at once, as is a lock with
/// no pid once it is a second old, and a lock whose mtime is in the future
/// when no live pid holds it. A live pid is left alone: after 5 seconds this
/// returns an error and does not run `body`.
pub fn with_claim_lock<T>(
    lock: &Path,
    body: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    if let Some(dir) = lock.parent() {
        sweep_abandoned_temps(dir);
    }
    let started = std::time::Instant::now();
    loop {
        match OpenOptions::new().write(true).create_new(true).open(lock) {
            Ok(mut file) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
                let text = format!(
                    "{} {} {:x}-{n}\n",
                    std::process::id(),
                    now.as_secs(),
                    now.as_nanos()
                );
                let _ = file.write_all(text.as_bytes());
                let _ = file.sync_all();
                let held = HeldLock {
                    path: lock.to_path_buf(),
                    text,
                };
                let result = body();
                drop(held);
                return result;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if break_abandoned_lock(lock) {
                    continue;
                }
                if started.elapsed() >= LOCK_WAIT {
                    return Err(
                        "this claim is being updated by another attach. Retry adsertio_attach; nothing was written."
                            .into(),
                    );
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(error) => return Err(format!("cannot lock the claim: {error}")),
        }
    }
}

/// `true` when a live process still holds `lock`. A missing file is not held.
pub fn lock_held(lock: &Path) -> bool {
    let age = file_age(lock);
    if matches!(age, Age::Missing) {
        return false;
    }
    let text = fs::read_to_string(lock).unwrap_or_default();
    match crate::proc::leading_pid(&text) {
        Some(pid) if crate::proc::pid_alive(pid) => {
            pid != std::process::id() || !own_pid_is_leftover(&age)
        }
        Some(_) => false,
        // No pid yet: the holder may still be writing the line. A mtime a
        // moment ahead is the same window. An hour ahead is not.
        None => within_grace(&age),
    }
}

fn within_grace(age: &Age) -> bool {
    match age {
        Age::Secs(secs) => *secs < LOCK_GRACE,
        Age::Future(ahead) => *ahead < CLOCK_SKEW,
        Age::Missing => false,
    }
}

/// This process is not inside the lock. A file that names us and is old, or
/// far in the future, is a reused pid or a planted time.
fn own_pid_is_leftover(age: &Age) -> bool {
    match age {
        Age::Secs(secs) => *secs >= LOCK_OWN_PID,
        Age::Future(ahead) => *ahead >= CLOCK_SKEW,
        Age::Missing => true,
    }
}

/// `true` when `lock` should be removed rather than waited on.
pub fn lock_file_is_stale(lock: &Path) -> bool {
    lock.exists() && !lock_held(lock)
}

/// Delete temp files in `dir` whose writer is gone. A temp whose name does
/// not carry a pid is removed once it is 30 seconds old, or at once when its
/// mtime is in the future. A live writer's temp is left in place.
pub fn sweep_abandoned_temps(dir: &Path) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with('.') && name.contains(".tmp-")) {
            continue;
        }
        if temp_abandoned(entry.path().as_path(), &name) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn temp_abandoned(path: &Path, name: &str) -> bool {
    if let Some(pid) = temp_pid(name) {
        return !crate::proc::pid_alive(pid);
    }
    match file_age(path) {
        Age::Future(ahead) if ahead >= CLOCK_SKEW => true,
        Age::Secs(secs) if secs >= TEMP_ABANDON => true,
        _ => false,
    }
}

/// `.{file}.tmp-{pid}-{n}` -> pid.
fn temp_pid(name: &str) -> Option<u32> {
    let rest = name.rsplit_once(".tmp-")?.1;
    let (pid, n) = rest.rsplit_once('-')?;
    if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let pid: u32 = pid.parse().ok()?;
    if pid == 0 {
        None
    } else {
        Some(pid)
    }
}

enum Age {
    Missing,
    /// How far the mtime is ahead of now.
    Future(std::time::Duration),
    Secs(u64),
}

fn file_age(path: &Path) -> Age {
    let Ok(meta) = fs::metadata(path) else {
        return Age::Missing;
    };
    match meta.modified() {
        Ok(when) => match when.elapsed() {
            Ok(age) => Age::Secs(age.as_secs()),
            Err(error) => Age::Future(error.duration()),
        },
        Err(_) => Age::Future(CLOCK_SKEW),
    }
}

/// Replace `dest` with `tmp`. On Windows a just-closed file can refuse the
/// replace for a moment (`Access is denied` or a sharing violation). Retry
/// that, and leave any other error as a failure.
fn replace_file(tmp: &Path, dest: &Path) -> Result<(), String> {
    let mut last = None;
    for attempt in 0..8 {
        match fs::rename(tmp, dest) {
            Ok(()) => return Ok(()),
            Err(error) if attempt < 7 && replace_busy(&error) => {
                last = Some(error);
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(error) => {
                return Err(format!("cannot replace {}: {error}", dest.display()));
            }
        }
    }
    Err(format!(
        "cannot replace {}: {}",
        dest.display(),
        last.map(|error| error.to_string())
            .unwrap_or_else(|| "the file stayed busy".into())
    ))
}

fn replace_busy(error: &std::io::Error) -> bool {
    // Windows: ERROR_ACCESS_DENIED is 5, ERROR_SHARING_VIOLATION is 32.
    cfg!(windows) && matches!(error.raw_os_error(), Some(5 | 32))
}

/// Why `meta` is not something a pointer can read.
pub fn not_a_file(meta: &fs::Metadata) -> &'static str {
    if meta.is_dir() {
        "is a folder; point at a file"
    } else if is_fifo(meta) {
        "is a pipe, not a file"
    } else {
        "is not a file"
    }
}

#[cfg(unix)]
fn is_fifo(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt;
    meta.file_type().is_fifo()
}

#[cfg(not(unix))]
fn is_fifo(_meta: &fs::Metadata) -> bool {
    false
}

/// Write `bytes` to `dest` whole: a temp file beside it, synced, then either
/// renamed over `dest` (`overwrite`) or hard-linked into place so an existing
/// file is never replaced. A reader sees the old file, the new file, or no
/// file, never a partial one.
pub fn write_atomic(dest: &Path, bytes: &[u8], overwrite: bool) -> Result<(), String> {
    let dir = dest
        .parent()
        .ok_or_else(|| format!("{} has no parent folder", dest.display()))?;
    let name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "bad file name".to_string())?;
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.tmp-{}-{n}", std::process::id()));
    let mut guard = TempGuard {
        path: tmp.clone(),
        armed: true,
    };
    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| format!("cannot create {}: {e}", tmp.display()))?;
        restrict(&tmp, false);
        f.write_all(bytes)
            .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        f.sync_all()
            .map_err(|e| format!("cannot sync {}: {e}", tmp.display()))?;
    }
    if overwrite {
        replace_file(&tmp, dest)?;
        guard.armed = false;
    } else {
        match fs::hard_link(&tmp, dest) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(format!("{} already exists", dest.display()));
            }
            Err(e) => return Err(format!("cannot create {}: {e}", dest.display())),
        }
    }
    Ok(())
}

/// Create a folder (and parents) readable only by the current user on Unix.
pub fn make_private_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    restrict(dir, true);
    Ok(())
}

#[cfg(unix)]
fn restrict(path: &Path, dir: bool) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(
        path,
        fs::Permissions::from_mode(if dir { 0o700 } else { 0o600 }),
    );
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _dir: bool) {}

/// Read a whole file as UTF-8. Invalid UTF-8 is an error naming the file,
/// never a lossy read.
pub fn read_strict(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    String::from_utf8(bytes).map_err(|_| format!("{} is not valid UTF-8", path.display()))
}

/// A slug: `[a-z0-9][a-z0-9-]*`, at most 64 bytes.
pub fn valid_slug(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Join slug segments (`a/b`, at most `max_segments`) under `base`, refusing
/// anything that is not a plain slug and any existing symlink on the way.
pub fn join_id(base: &Path, id: &str, max_segments: usize) -> Result<PathBuf, String> {
    let segs: Vec<&str> = id.split('/').collect();
    if segs.len() > max_segments || !segs.iter().all(|s| valid_slug(s)) {
        return Err(format!(
            "invalid id \"{}\": use lowercase letters, digits and '-' (up to {max_segments} segment(s) joined by '/', each up to 64 characters)",
            clip(id, 60)
        ));
    }
    let mut p = base.to_path_buf();
    for s in &segs {
        p.push(s);
    }
    refuse_symlinks(base, &p)?;
    Ok(p)
}

/// Resolve a user-supplied relative path against `root`. Absolute paths, `..`,
/// and any route that leaves `root` (including through a symlink) are errors.
pub fn project_file(root: &Path, rel: &str) -> Result<PathBuf, String> {
    if rel.is_empty() || rel.contains('\0') {
        return Err("path is empty or contains a NUL byte".to_string());
    }
    let p = Path::new(rel);
    if p.is_absolute()
        || p.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
    {
        return Err(format!(
            "path \"{}\" must be relative and inside the project",
            clip(rel, 80)
        ));
    }
    let joined = root.join(p);
    let canon_root = root
        .canonicalize()
        .map_err(|e| format!("cannot resolve project folder: {e}"))?;
    match joined.canonicalize() {
        Ok(c) if c.starts_with(&canon_root) => Ok(c),
        Ok(_) => Err(format!(
            "path \"{}\" resolves outside the project",
            clip(rel, 80)
        )),
        Err(_) => Err(format!("path \"{}\" does not exist", clip(rel, 80))),
    }
}

fn refuse_symlinks(base: &Path, full: &Path) -> Result<(), String> {
    let mut cur = base.to_path_buf();
    if let Ok(rest) = full.strip_prefix(base) {
        for c in rest.components() {
            cur.push(c);
            if let Ok(m) = fs::symlink_metadata(&cur) {
                if m.file_type().is_symlink() {
                    return Err(format!(
                        "{} is a symlink; refusing to follow it",
                        cur.display()
                    ));
                }
            }
        }
    }
    Ok(())
}

pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max).collect();
        t.push_str("...");
        t
    }
}

/// Current UTC time as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn racing_past_a_dead_attach_lock_leaves_one_holder() {
        use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
        let d = std::env::temp_dir().join(format!("adsertio-fsx-dead-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let lock = d.join(".c.md.lock");
        for _ in 0..12 {
            fs::write(&lock, "999999 0\n").unwrap();
            let inside = std::sync::Arc::new(AtomicU64::new(0));
            let most = std::sync::Arc::new(AtomicU64::new(0));
            let gate = std::sync::Arc::new(std::sync::Barrier::new(6));
            let handles: Vec<_> = (0..6)
                .map(|_| {
                    let (lock, inside, most, gate) =
                        (lock.clone(), inside.clone(), most.clone(), gate.clone());
                    std::thread::spawn(move || {
                        gate.wait();
                        with_claim_lock(&lock, || {
                            most.fetch_max(inside.fetch_add(1, SeqCst) + 1, SeqCst);
                            std::thread::sleep(std::time::Duration::from_millis(30));
                            inside.fetch_sub(1, SeqCst);
                            Ok(())
                        })
                        .unwrap();
                    })
                })
                .collect();
            for handle in handles {
                handle.join().unwrap();
            }
            assert_eq!(most.load(SeqCst), 1, "two attach holders at once");
            assert!(!lock.exists());
        }
        let _ = fs::remove_dir_all(&d);
    }

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("fsx-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn dates_are_right() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_782), (2026, 11, 25));
    }

    #[test]
    fn create_only_never_overwrites_and_leaves_no_temp() {
        let d = scratch("atomic");
        let f = d.join("a.md");
        write_atomic(&f, b"one", false).unwrap();
        assert!(write_atomic(&f, b"two", false).is_err());
        assert_eq!(fs::read_to_string(&f).unwrap(), "one");
        write_atomic(&f, b"three", true).unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "three");
        let strays: Vec<_> = fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|n| n.to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(strays.is_empty(), "{strays:?}");
    }

    #[test]
    fn ids_and_paths_are_guarded() {
        let d = scratch("guard");
        assert!(join_id(&d, "a/b", 2).is_ok());
        assert!(join_id(&d, "a/b/c", 2).is_err());
        assert!(join_id(&d, "../x", 2).is_err());
        assert!(join_id(&d, "A", 2).is_err());
        assert!(join_id(&d, "", 2).is_err());
        assert!(project_file(&d, "../x").is_err());
        assert!(project_file(&d, "/etc/passwd").is_err());
        fs::write(d.join("f.json"), "{}").unwrap();
        assert!(project_file(&d, "f.json").is_ok());
        assert!(project_file(&d, "missing.json").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_refused() {
        let d = scratch("symlink");
        let outside = scratch("symlink-outside");
        fs::write(outside.join("secret"), "x").unwrap();
        std::os::unix::fs::symlink(&outside, d.join("link")).unwrap();
        assert!(project_file(&d, "link/secret").is_err());
        assert!(join_id(&d, "link", 1).is_err());
    }
}
