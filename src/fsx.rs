//! File helpers: atomic writes with cleanup, strict reads, path guards.

use crate::oslock::FileLock;
use std::fs;
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

/// How long an attach waits for another attach to the same claim.
pub const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// How old a temp file must be before it is taken as abandoned. A write holds
/// its temp for one write, sync and rename: milliseconds. A writer paused past
/// this (a laptop asleep mid-write) loses only that write: its rename or link
/// then fails with an error and nothing else changes, because each temp name
/// holds the writer's pid and a counter and is never reused.
pub const TEMP_STALE: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Hold the operating-system lock at `lock` for a read-modify-write, then run
/// `body`. The kernel releases the lock when this process exits or is killed,
/// so there is no stale lock to judge. A busy lock is retried with a sleep of
/// 1 to 10 ms for up to `LOCK_WAIT`; after that this returns an error and does
/// not run `body`. The lock file stays: lock files are never deleted.
pub fn with_claim_lock<T>(
    lock: &Path,
    body: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let started = std::time::Instant::now();
    let mut pause = std::time::Duration::from_millis(1);
    let held = loop {
        match FileLock::try_acquire(lock).map_err(|e| format!("cannot lock the claim: {e}"))? {
            Some(held) => break held,
            None if started.elapsed() >= LOCK_WAIT => {
                return Err(
                    "this claim is being updated by another attach. Retry adsertio_attach; nothing was written."
                        .into(),
                );
            }
            None => {
                std::thread::sleep(pause);
                pause = (pause * 2).min(std::time::Duration::from_millis(10));
            }
        }
    };
    let result = body();
    drop(held);
    result
}

/// Delete temp files in `dir` older than `TEMP_STALE`. Nothing else is
/// touched: a younger temp may belong to a writer still running.
pub fn sweep_abandoned_temps(dir: &Path) {
    for (name, abandoned) in work_files(dir) {
        if abandoned && is_temp(&name) {
            let _ = fs::remove_file(dir.join(name));
        }
    }
}

/// Adsertio's own work files in `dir` (temp files, and the attach locks and
/// break guards 1.3 and earlier left there), each with `true` when it may be
/// removed: a temp older than `TEMP_STALE`, or any old-style lock file, which
/// nothing uses since 1.4. Reads only.
pub fn work_files(dir: &Path) -> Vec<(String, bool)> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !is_work_file(&name) {
            continue;
        }
        if entry.file_type().is_ok_and(|t| !t.is_file()) {
            continue;
        }
        let abandoned = is_old_lock(&name) || aged(&entry.path(), TEMP_STALE);
        out.push((name, abandoned));
    }
    out.sort();
    out
}

/// Remove the work files in `dir` that `work_files` marks removable (old
/// temps and old-style lock files), as `check` does, and return the names
/// removed. A temp younger than `TEMP_STALE` is left alone.
pub fn clear_abandoned(dir: &Path) -> Vec<String> {
    work_files(dir)
        .into_iter()
        .filter(|(name, abandoned)| *abandoned && fs::remove_file(dir.join(name)).is_ok())
        .map(|(name, _)| name)
        .collect()
}

/// Files the operating system drops into any folder (macOS Finder's
/// `.DS_Store`). They are not claims and not strays: every command ignores them.
pub fn is_os_file(name: &str) -> bool {
    name == ".DS_Store"
}

/// A file Adsertio itself leaves in the claims folder: a temp file
/// (`.<name>.tmp-<pid>-<n>`) for a moment, or a lock file from 1.3 and
/// earlier (`.<id>.md.lock`, `.<id>.md.lock.break`). Any other name that is
/// not `<id>.md`, dotfiles included, is not a claim file.
pub fn is_work_file(name: &str) -> bool {
    is_temp(name) || is_old_lock(name)
}

fn is_temp(name: &str) -> bool {
    name.starts_with('.') && name.contains(".tmp-")
}

/// An attach lock or break guard from 1.3 and earlier. 1.4 never uses these
/// names, so one always blocks nothing and may be removed.
pub fn is_old_lock(name: &str) -> bool {
    name.starts_with('.')
        && !is_temp(name)
        && (name.ends_with(".md.lock") || name.ends_with(".md.lock.break"))
}

/// `true` when `path`'s mtime is more than `limit` in the past. A mtime in the
/// future is not old.
fn aged(path: &Path, limit: std::time::Duration) -> bool {
    fs::symlink_metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age > limit)
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
    fn one_attach_holds_a_claim_lock_at_a_time_and_the_file_stays() {
        use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
        let d = scratch("claim-lock");
        let lock = d.join("c.lock");
        // Left by an older writer: the contents mean nothing now.
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
        assert!(lock.is_file(), "a lock file is never deleted");
    }

    #[test]
    fn a_busy_claim_lock_gives_up_with_nothing_written() {
        let d = scratch("claim-busy");
        let lock = d.join("c.lock");
        let _held = FileLock::try_acquire(&lock).unwrap().unwrap();
        let started = std::time::Instant::now();
        let err = with_claim_lock(&lock, || -> Result<(), String> {
            panic!("ran the body while the lock was held")
        })
        .unwrap_err();
        assert!(started.elapsed() >= LOCK_WAIT);
        assert!(
            err.contains("another attach") && err.contains("nothing was written"),
            "{err}"
        );
    }

    #[test]
    fn temps_are_judged_by_age_and_old_locks_are_always_removable() {
        let d = scratch("work-files");
        let set_age = |name: &str, secs: u64| {
            let path = d.join(name);
            fs::write(&path, b"x").unwrap();
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs))
                .unwrap();
        };
        set_age(".a.md.tmp-1-1", 2 * 60 * 60);
        set_age(".b.md.tmp-999999-1", 120);
        set_age(".c.md.lock", 0);
        set_age(".c.md.lock.break", 0);
        set_age("c.md", 2 * 60 * 60);
        let files = work_files(&d);
        assert_eq!(
            files,
            [
                (".a.md.tmp-1-1".to_string(), true),
                (".b.md.tmp-999999-1".to_string(), false),
                (".c.md.lock".to_string(), true),
                (".c.md.lock.break".to_string(), true),
            ]
        );
        sweep_abandoned_temps(&d);
        assert!(!d.join(".a.md.tmp-1-1").exists());
        assert!(
            d.join(".c.md.lock").exists(),
            "the sweep removes temps only"
        );
        let removed = clear_abandoned(&d);
        assert_eq!(removed, [".c.md.lock", ".c.md.lock.break"]);
        assert!(d.join(".b.md.tmp-999999-1").exists());
        assert!(d.join("c.md").exists());
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
