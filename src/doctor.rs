//! `adsertio doctor`: read-only diagnostics for the store, git, and the
//! evidence stores, each check with a `fix` sentence when it fails.
//!
//! Doctor only reads, with two exceptions. It writes and removes one probe
//! file in the claims folder, through `fsx::write_atomic` with
//! `overwrite = false`, to show that a new claim can be written there. With
//! `fix` it removes temp files over an hour old and the attach lock files
//! 1.3 left in `claims`, which `check` already does. It never deletes a lock
//! file in `.adsertio/locks/` and never writes into another tool's folder.

use crate::fsx;
use crate::json::Value;
use crate::read;
use crate::store::Store;
use crate::verdict::{self, State};
use std::path::{Path, PathBuf};

/// One line of the report.
struct Check {
    name: &'static str,
    ok: bool,
    detail: String,
    fix: Option<String>,
}

impl Check {
    fn pass(name: &'static str, detail: impl Into<String>) -> Check {
        Check {
            name,
            ok: true,
            detail: detail.into(),
            fix: None,
        }
    }

    fn fail(name: &'static str, detail: impl Into<String>, fix: impl Into<String>) -> Check {
        Check {
            name,
            ok: false,
            detail: detail.into(),
            fix: Some(fix.into()),
        }
    }

    fn to_value(&self) -> Value {
        Value::obj(vec![
            ("name", Value::str(self.name)),
            ("ok", Value::Bool(self.ok)),
            ("detail", Value::str(self.detail.as_str())),
            (
                "fix",
                match &self.fix {
                    Some(fix) => Value::str(fix.as_str()),
                    None => Value::Null,
                },
            ),
        ])
    }
}

/// The longest name Adsertio writes in the claims folder: a temp file for a
/// 64-byte id, `.<id>.md.tmp-<pid>-<n>`, with a 10-digit pid and a 20-digit
/// counter.
pub const LONGEST_NAME: usize = 1 + 64 + 3 + 5 + 10 + 1 + 20;

/// Windows `MAX_PATH` is 260 UTF-16 units, the last one the terminating NUL.
pub const MAX_PATH: usize = 260;

/// A path this close to `MAX_PATH` gets a warning in the detail.
const PATH_MARGIN: usize = 40;

pub fn run(fix: bool) -> Value {
    let mut checks = Vec::new();
    let mut fixed: Vec<String> = Vec::new();
    checks.push(Check::pass(
        "version",
        format!(
            "adsertio {} on {}-{} ({})",
            crate::VERSION,
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::env::consts::FAMILY
        ),
    ));
    let store = store_check(&mut checks);
    let project = match &store {
        Some(store) => store.project.clone(),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    if let Some(store) = &store {
        let dir = store.claims_dir();
        if fix {
            fixed = fsx::clear_abandoned(&dir);
        }
        checks.push(writable_check(&dir));
        checks.push(claims_check(store));
        checks.push(leftovers_check(&dir, fix));
        if cfg!(windows) {
            checks.push(path_length_check(&absolute(&dir)));
        }
        checks.push(file_system_check(&absolute(&store.dir)));
    }
    checks.push(git_check(&project));
    crate::cache::with_cache(|| {
        checks.push(store_file_check(&project, "trammel", ".trammel/store"));
        checks.push(store_file_check(&project, "chisel", ".chisel/store"));
    });
    checks.push(probatio_check(&project));
    checks.push(binaries_check());
    let ok = checks.iter().all(|c| c.ok);
    let mut out = vec![
        ("ok", Value::Bool(ok)),
        (
            "checks",
            Value::Array(checks.iter().map(Check::to_value).collect()),
        ),
    ];
    if fix {
        out.push((
            "fixed",
            Value::Array(fixed.into_iter().map(Value::String).collect()),
        ));
    }
    Value::obj(out)
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

fn store_check(checks: &mut Vec<Check>) -> Option<Store> {
    let from_env = std::env::var_os("ADSERTIO_DIR");
    let source = match &from_env {
        Some(dir) => format!("ADSERTIO_DIR is {}", Path::new(dir).display()),
        None => "ADSERTIO_DIR is not set; found by walking up from the working folder".into(),
    };
    let store = match Store::open() {
        Ok(store) => store,
        Err(e) => {
            let fix = if from_env.is_some() {
                "Set ADSERTIO_DIR to the project's .adsertio folder (an absolute path), or unset it and run adsertio from inside the project. Run adsertio init in the project folder if there is no store yet."
            } else {
                "Run adsertio init in the project folder, or set ADSERTIO_DIR to the absolute path of the project's .adsertio folder (the MCP server needs this when the host starts it elsewhere)."
            };
            checks.push(Check::fail("store", format!("{e}. {source}"), fix));
            return None;
        }
    };
    if let Err(e) = crate::store::claims_ready(&store, false) {
        checks.push(Check::fail(
            "store",
            format!("{}: {e}. {source}", store.dir.display()),
            "Run adsertio init in the project folder so .adsertio/claims is a real folder.",
        ));
        return None;
    }
    checks.push(Check::pass(
        "store",
        format!(
            "{} (project {}). {source}",
            absolute(&store.dir).display(),
            absolute(&store.project).display()
        ),
    ));
    Some(store)
}

fn writable_check(dir: &Path) -> Check {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or_default();
    // A temp-file name: if this process dies before removing it, check or
    // doctor --fix removes it once it is an hour old.
    let probe = dir.join(format!(".doctor.tmp-{}-{nanos}", std::process::id()));
    match fsx::write_atomic(&probe, b"adsertio doctor probe\n", false) {
        Ok(()) => match std::fs::remove_file(&probe) {
            Ok(()) => Check::pass(
                "store_writable",
                format!(
                    "wrote and removed a probe file in {} the way a new claim is written",
                    dir.display()
                ),
            ),
            Err(e) => Check::fail(
                "store_writable",
                format!("wrote {} but could not remove it: {e}", probe.display()),
                "Check that nothing (an antivirus scanner, a sync client) holds files open in .adsertio/claims, then run adsertio doctor --fix.",
            ),
        },
        Err(e) => Check::fail(
            "store_writable",
            format!("cannot write a new file in {}: {e}", dir.display()),
            "Give your user write access to .adsertio/claims. New claims are hard-linked into place, so the folder must be on a disk that supports hard links (NTFS, APFS, ext4), not FAT, exFAT or some network shares.",
        ),
    }
}

fn claims_check(store: &Store) -> Check {
    match crate::tools::claim_problems(store, false, true) {
        Ok(found) if found.problems.is_empty() => {
            Check::pass("claims", format!("{} claims, none damaged", found.claims))
        }
        Ok(found) => {
            let shown: Vec<&str> = found.problems.iter().take(5).map(String::as_str).collect();
            let more = found.problems.len().saturating_sub(shown.len());
            let tail = if more > 0 {
                format!(" (and {more} more)")
            } else {
                String::new()
            };
            Check::fail(
                "claims",
                format!(
                    "{} claims; {} problem(s): {}{tail}",
                    found.claims,
                    found.problems.len(),
                    shown.join(" ")
                ),
                "Repair each file named, or move it out of .adsertio/claims. Adsertio never skips a damaged claim; adsertio check lists every problem.",
            )
        }
        Err(e) => Check::fail(
            "claims",
            e,
            "Repair the claim file named (its frontmatter must have summary, author and created), or move it out of .adsertio/claims. Adsertio never skips or resets a damaged claim.",
        ),
    }
}

fn leftovers_check(dir: &Path, fixed: bool) -> Check {
    let files = fsx::work_files(dir);
    let dead: Vec<&str> = files
        .iter()
        .filter(|(_, abandoned)| *abandoned)
        .map(|(name, _)| name.as_str())
        .collect();
    let live: Vec<&str> = files
        .iter()
        .filter(|(_, abandoned)| !*abandoned)
        .map(|(name, _)| name.as_str())
        .collect();
    let live_note = if live.is_empty() {
        String::new()
    } else {
        format!(
            "; temp files under an hour old, which a writer may still own, left alone: {}",
            clip_list(&live)
        )
    };
    if dead.is_empty() {
        let done = if fixed { " after --fix" } else { "" };
        return Check::pass(
            "leftover_files",
            format!(
                "no temp file over an hour old and no lock file from Adsertio 1.3{done}{live_note}"
            ),
        );
    }
    Check::fail(
        "leftover_files",
        format!(
            "{} file(s) to remove, temp files over an hour old or lock files from Adsertio 1.3 (which block nothing): {}{live_note}",
            dead.len(),
            clip_list(&dead)
        ),
        "Run adsertio doctor --fix (or adsertio check) to remove them. The lock files in .adsertio/locks are kept, and a temp file under an hour old is never removed.",
    )
}

fn clip_list(names: &[&str]) -> String {
    let shown: Vec<&str> = names.iter().take(10).copied().collect();
    if names.len() > shown.len() {
        format!(
            "{} (and {} more)",
            shown.join(", "),
            names.len() - shown.len()
        )
    } else {
        shown.join(", ")
    }
}

/// The claims folder path length against Windows `MAX_PATH`, counting the
/// longest file name Adsertio writes there. `chars` is the folder path in
/// UTF-16 units.
pub fn path_length(chars: usize) -> (bool, String) {
    let longest = chars + 1 + LONGEST_NAME;
    if longest >= MAX_PATH {
        (
            false,
            format!(
                "the claims folder path is {chars} characters, so the longest file Adsertio writes there is {longest}, over the Windows limit of {} (MAX_PATH)",
                MAX_PATH - 1
            ),
        )
    } else if longest + PATH_MARGIN >= MAX_PATH {
        (
            true,
            format!(
                "the claims folder path is {chars} characters; the longest file Adsertio writes there is {longest}, close to the Windows limit of {}",
                MAX_PATH - 1
            ),
        )
    } else {
        (
            true,
            format!(
                "the claims folder path is {chars} characters; the longest file Adsertio writes there is {longest}, under the Windows limit of {}",
                MAX_PATH - 1
            ),
        )
    }
}

fn path_length_check(dir: &Path) -> Check {
    let text = dir.as_os_str().to_string_lossy();
    // `\\?\` paths are not limited by MAX_PATH.
    if text.starts_with(r"\\?\") {
        return Check::pass(
            "path_length",
            "the store path uses the \\\\?\\ prefix, which has no MAX_PATH limit",
        );
    }
    let (ok, detail) = path_length(text.encode_utf16().count());
    if ok {
        Check::pass("path_length", detail)
    } else {
        Check::fail(
            "path_length",
            detail,
            "Move the project to a shorter path (for example C:\\src\\name), or turn on Windows long paths (LongPathsEnabled in the registry, or the group policy Enable Win32 long paths).",
        )
    }
}

/// A store on a network file system: operating-system locks there may be
/// emulated or not shared between machines, and hard links and file times may
/// not behave. A warning, not a failure: the check passes with a `fix`.
fn file_system_check(dir: &Path) -> Check {
    match crate::oslock::network_fs(dir) {
        Some(kind) => Check {
            name: "file_system",
            ok: true,
            detail: format!(
                "{} is on a network file system ({kind}); file locks there may be emulated or not shared between machines, so two attaches on different machines could both get the lock",
                dir.display()
            ),
            fix: Some("Keep the project on a local disk, or let only one machine write claims to it.".into()),
        },
        None => Check::pass(
            "file_system",
            "the store is on a local file system, or this platform cannot tell: attach locks are reliable",
        ),
    }
}

fn git_check(project: &Path) -> Check {
    match crate::fresh::head_commit_uncached(project) {
        Ok(head) => Check::pass(
            "git",
            format!("git rev-parse HEAD in {} is {head}", absolute(project).display()),
        ),
        Err(e) if e.starts_with("git is not available") => Check::fail(
            "git",
            e,
            "Install git and put it on PATH (on Windows, Git for Windows adds it). Without git, every pass that names a commit is stale.",
        ),
        Err(e) => Check::fail(
            "git",
            format!("git rev-parse HEAD in {}: {e}", absolute(project).display()),
            "Run Adsertio in a git repository with at least one commit. Without a HEAD, every Trammel or Chisel pass that names a commit is stale.",
        ),
    }
}

fn store_file_check(project: &Path, tool: &'static str, rel: &str) -> Check {
    let name = if tool == "trammel" {
        "trammel_store"
    } else {
        "chisel_store"
    };
    let path = project.join(rel);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(_) => return Check::pass(name, format!("{rel} is not present")),
    };
    let size = std::fs::metadata(&path)
        .map(|m| m.len())
        .unwrap_or(meta.len());
    if size > read::MAX_STORE_BYTES {
        return Check::fail(
            name,
            read::size_detail(rel, size, read::MAX_STORE_BYTES),
            format!("Ask {tool} to compact or prune its store until it is under 1 GiB; Adsertio reads the whole file."),
        );
    }
    let near = if size > read::MAX_STORE_BYTES / 10 * 9 {
        format!(
            ", close to the {} MiB limit",
            read::MAX_STORE_BYTES / (1024 * 1024)
        )
    } else {
        String::new()
    };
    // A selector that names no row: a store that parses answers `missing`.
    let probe = if tool == "trammel" {
        format!("trammel:{rel}#plan-0")
    } else {
        format!("chisel:{rel}#test:adsertio-doctor-probe")
    };
    let evidence = verdict::read_evidence(project, &probe);
    let size_text = format!("{rel} is {size} bytes{near}");
    match evidence.state {
        State::Unreadable => {
            let detail = evidence.detail;
            let fix = if detail.contains("SQLite") {
                format!("Run {tool} migrate so {rel} is the JSON snapshot.")
            } else if detail.contains("lock") {
                format!("{tool} is writing its store; retry once it has finished.")
            } else {
                format!("Let {tool} rewrite {rel} (run any {tool} command that saves it), or restore it from a backup. Adsertio never repairs another tool's store.")
            };
            Check::fail(
                name,
                format!("{size_text}; Adsertio cannot read it: {detail}"),
                fix,
            )
        }
        _ => Check::pass(name, format!("{size_text}; Adsertio can read it")),
    }
}

/// `.probatio` and the run folders in it, up to two levels down. A run
/// folder holds `results/`, `run.json`, `tally.json` or `ledger.json`.
fn probatio_check(project: &Path) -> Check {
    let root = project.join(".probatio");
    match std::fs::symlink_metadata(&root) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            return Check::fail(
                "probatio",
                ".probatio is not a folder",
                "Point Probatio's --out at a folder inside the project, and point at that folder in probatio: pointers.",
            )
        }
        Err(_) => return Check::pass("probatio", ".probatio is not present"),
    }
    let mut runs = Vec::new();
    find_runs(&root, ".probatio", 0, &mut runs);
    if runs.is_empty() {
        return Check::pass(
            "probatio",
            ".probatio is present and holds no run folder (results/, run.json, tally.json or ledger.json)",
        );
    }
    let mut bad = Vec::new();
    crate::cache::with_cache(|| {
        for run in &runs {
            let evidence = verdict::read_evidence(project, &format!("probatio:{run}"));
            if evidence.state == State::Unreadable {
                bad.push(format!("{run}: {}", evidence.detail));
            }
        }
    });
    if bad.is_empty() {
        Check::pass(
            "probatio",
            format!(
                "{} run folder(s) under .probatio, each readable: {}",
                runs.len(),
                runs.join(", ")
            ),
        )
    } else {
        Check::fail(
            "probatio",
            format!(
                "{} of {} run folder(s) cannot be read: {}",
                bad.len(),
                runs.len(),
                bad.join("; ")
            ),
            "Rerun Probatio into that folder, or delete the damaged result file it names (each result must be JSON with an outcome, and at most 64 MiB).",
        )
    }
}

const MAX_RUNS: usize = 50;

fn find_runs(dir: &Path, rel: &str, depth: usize, out: &mut Vec<String>) {
    if out.len() >= MAX_RUNS {
        return;
    }
    let is_run = ["results", "run.json", "tally.json", "ledger.json"]
        .iter()
        .any(|name| dir.join(name).exists());
    if is_run {
        out.push(rel.to_string());
        return;
    }
    if depth >= 2 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut subdirs: Vec<String> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| !name.starts_with('.'))
        .collect();
    subdirs.sort();
    for name in subdirs {
        find_runs(&dir.join(&name), &format!("{rel}/{name}"), depth + 1, out);
    }
}

fn binaries_check() -> Check {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            return Check::fail(
                "binaries",
                format!("cannot find this program's path: {e}"),
                "Run adsertio by its full path, or put the folder holding adsertio and adsertio-mcp on PATH.",
            )
        }
    };
    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    let suffix = std::env::consts::EXE_SUFFIX;
    let missing: Vec<String> = ["adsertio", "adsertio-mcp"]
        .iter()
        .map(|name| format!("{name}{suffix}"))
        .filter(|name| !dir.join(name).is_file())
        .collect();
    if missing.is_empty() {
        Check::pass(
            "binaries",
            format!(
                "adsertio{suffix} and adsertio-mcp{suffix} are both in {}",
                dir.display()
            ),
        )
    } else {
        Check::fail(
            "binaries",
            format!("{} not found in {}", missing.join(" and "), dir.display()),
            "Copy both programs from the release archive into the same folder, or point the MCP config's command at the full path of adsertio-mcp (or run adsertio mcp).",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_length_flags_long_store_paths() {
        assert!(path_length(40).0);
        assert!(path_length(40).1.contains("under"));
        assert!(path_length(MAX_PATH - LONGEST_NAME - 20)
            .1
            .contains("close to"));
        assert!(!path_length(MAX_PATH - LONGEST_NAME).0);
        assert!(!path_length(300).0);
    }
}
