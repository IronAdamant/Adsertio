//! The Chisel adapter. It reads a `.chisel/store` snapshot (and, when the
//! pointer is exactly that root file, each shard under `.chisel/shards/`)
//! and answers `test:<id>` or `file:<path>`. It never runs Chisel and never
//! writes. Worktree overlays under `.chisel/overlays/` are not read.
//!
//! The latest `test_results` row for an id is the last one in the root
//! snapshot. Shards add their `test_edges` only: Chisel records results in
//! the root store, so a shard's `test_results` and `test_passes` are not
//! read and never override the root. A pointer at one shard's store is not
//! read either. A `file:`
//! selector passes only when at least one `test_edges` row points at that
//! file and every such test's latest result passed.

use crate::claim;
use crate::read::{self, ReadError};
use crate::verdict::{Evidence, State};
use std::fs;
use std::path::Path;

enum Sel {
    Test(String),
    File(String),
}

pub fn evaluate(project: &Path, pointer: &str) -> Evidence {
    if let Err(text) = claim::valid_pointer(pointer) {
        return evidence(pointer, State::Unreadable, text);
    }
    let rest = pointer.split_once(':').map(|(_, rest)| rest).unwrap_or("");
    let (rel, selector) = match rest.split_once('#') {
        Some((rel, selector)) => (rel, selector),
        None => (rest, ""),
    };
    if path_leaves(rel) {
        return match read::read_chisel(project, rel) {
            Err(ReadError::Missing(text)) => evidence(pointer, State::Missing, text),
            Err(ReadError::Unreadable(text)) => evidence(pointer, State::Unreadable, text),
            Ok(_) => evidence(pointer, State::Unreadable, "path leaves the project"),
        };
    }
    if is_overlay(project, rel) {
        return evidence(
            pointer,
            State::NoAdapter,
            "worktree overlays under .chisel/overlays are not read in v1, so this is not proof",
        );
    }
    if is_shard(project, rel) {
        return evidence(
            pointer,
            State::NoAdapter,
            "Chisel records test results in the root store, not in a shard, so a shard's own rows are not proof. Point at .chisel/store, which also reads every shard's test edges",
        );
    }
    let index = match crate::cache::chisel(project, rel, || load_index(project, rel)) {
        Ok(index) => index,
        Err(ReadError::Missing(text)) => return evidence(pointer, State::Missing, text),
        Err(ReadError::Unreadable(text)) => return evidence(pointer, State::Unreadable, text),
    };
    if selector.is_empty() {
        return evidence(
            pointer,
            State::Unreadable,
            "a selector is required: test:<test_id> or file:<path>",
        );
    }
    let sel = match parse_selector(selector) {
        Ok(sel) => sel,
        Err(text) => return evidence(pointer, State::Unreadable, text),
    };
    match judge(&index, sel) {
        Ok((state, detail, stamp)) => {
            let mut found = evidence(pointer, state, detail);
            if found.state == State::Pass {
                found.at = stamp.at;
                found.commits = stamp.commits;
                found.malformed = stamp.malformed;
                if stamp.no_commit {
                    found.detail.push_str(
                        "; Chisel recorded no commit for this pass (HEAD was unreadable), so no commit is checked",
                    );
                }
            }
            found
        }
        Err(text) => evidence(pointer, State::Unreadable, text),
    }
}

fn evidence(pointer: &str, state: State, detail: impl Into<String>) -> Evidence {
    Evidence {
        pointer: pointer.to_string(),
        state,
        detail: detail.into(),
        at: None,
        commits: Vec::new(),
        malformed: false,
    }
}

fn path_leaves(rel: &str) -> bool {
    Path::new(rel).is_absolute() || rel.split(['/', '\\']).any(|part| part == "..")
}

/// A pointer under `.chisel/overlays/`, however it is spelt. The parts are
/// compared without regard to ASCII case, so `.CHISEL/OVERLAYS` on a
/// case-insensitive disk is caught, and the path is also resolved, so a
/// symlink into the overlays folder is caught too.
fn is_overlay(project: &Path, rel: &str) -> bool {
    under_chisel(project, rel, "overlays")
}

/// A pointer at one shard's own store, under `.chisel/shards/`.
fn is_shard(project: &Path, rel: &str) -> bool {
    under_chisel(project, rel, "shards")
}

fn under_chisel(project: &Path, rel: &str, folder: &str) -> bool {
    let norm = rel.replace('\\', "/");
    let parts: Vec<&str> = norm
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    if names_folder(&parts, folder) {
        return true;
    }
    let Ok(root) = project.canonicalize() else {
        return false;
    };
    let Some(target) = resolve_existing(&project.join(rel)) else {
        return false;
    };
    let root_parts = parts_of(&root);
    let target_parts = parts_of(&target);
    if target_parts.len() < root_parts.len()
        || !root_parts
            .iter()
            .zip(&target_parts)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
    {
        return false;
    }
    let inside: Vec<&str> = target_parts[root_parts.len()..]
        .iter()
        .map(String::as_str)
        .collect();
    names_folder(&inside, folder)
}

fn names_folder(parts: &[&str], folder: &str) -> bool {
    parts
        .windows(2)
        .any(|pair| pair[0].eq_ignore_ascii_case(".chisel") && pair[1].eq_ignore_ascii_case(folder))
}

fn parts_of(path: &Path) -> Vec<String> {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect()
}

/// The path with its longest existing prefix resolved (symlinks followed),
/// and the rest, which does not exist, appended as written.
fn resolve_existing(path: &Path) -> Option<std::path::PathBuf> {
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = path.to_path_buf();
    loop {
        if let Ok(canon) = cur.canonicalize() {
            let mut out = canon;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return Some(out);
        }
        rest.push(cur.file_name()?.to_os_string());
        if !cur.pop() {
            return None;
        }
    }
}

/// The root store, however the pointer spells it (`./`, `//`, `.`, a
/// symlinked folder, or another letter case on a case-insensitive disk): the
/// shards' test edges merge into it.
fn is_root_store(project: &Path, rel: &str) -> bool {
    let norm = rel.replace('\\', "/");
    if norm.trim_start_matches("./") == ".chisel/store" {
        return true;
    }
    match (
        project.join(rel).canonicalize(),
        project.join(".chisel").join("store").canonicalize(),
    ) {
        (Ok(named), Ok(root)) => named
            .to_string_lossy()
            .eq_ignore_ascii_case(&root.to_string_lossy()),
        _ => false,
    }
}

/// At most ten names, then how many more, so one pointer's detail stays short.
fn some(names: &[&str]) -> String {
    const SHOWN: usize = 10;
    if names.len() <= SHOWN {
        return names.join(", ");
    }
    format!(
        "{} and {} more",
        names[..SHOWN].join(", "),
        names.len() - SHOWN
    )
}

fn parse_selector(selector: &str) -> Result<Sel, String> {
    if let Some(id) = selector.strip_prefix("test:") {
        if id.is_empty() {
            return Err("test id is empty".into());
        }
        return Ok(Sel::Test(id.to_string()));
    }
    if let Some(path) = selector.strip_prefix("file:") {
        if path.is_empty() || path.split(['/', '\\']).any(|part| part == "..") {
            return Err("file selector is empty or contains ..".into());
        }
        return Ok(Sel::File(path.replace('\\', "/")));
    }
    Err(format!(
        "unknown chisel selector \"{}\"",
        crate::fsx::clip(selector, 60)
    ))
}

fn load_index(project: &Path, rel: &str) -> Result<crate::chisel_idx::Index, ReadError> {
    let mut index = crate::chisel_idx::parse_file(project, rel)?;
    if !is_root_store(project, rel) {
        return Ok(index);
    }
    for rel_store in shard_stores(project)? {
        match crate::chisel_idx::parse_file(project, &rel_store) {
            Ok(part) => index.merge_shard(part),
            Err(ReadError::Missing(_)) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(index)
}

fn shard_stores(project: &Path) -> Result<Vec<String>, ReadError> {
    let root = project.join(".chisel").join("shards");
    match fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(ReadError::Unreadable(format!(
                "cannot stat .chisel/shards: {error}"
            )))
        }
        Ok(meta) if meta.file_type().is_symlink() => {
            let canon = root.canonicalize().map_err(|error| {
                ReadError::Unreadable(format!("cannot resolve .chisel/shards: {error}"))
            })?;
            let project_root = project.canonicalize().map_err(|error| {
                ReadError::Unreadable(format!("cannot resolve the project folder: {error}"))
            })?;
            if !canon.starts_with(&project_root) {
                return Err(ReadError::Unreadable(
                    ".chisel/shards resolves outside the project".into(),
                ));
            }
        }
        Ok(meta) if !meta.is_dir() => {
            return Err(ReadError::Unreadable(
                ".chisel/shards is not a folder".into(),
            ))
        }
        Ok(_) => {}
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&root)
        .map_err(|error| ReadError::Unreadable(format!("cannot read .chisel/shards: {error}")))?
    {
        let entry = entry.map_err(|error| {
            ReadError::Unreadable(format!("cannot read .chisel/shards: {error}"))
        })?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            return Err(ReadError::Unreadable("a shard name is not UTF-8".into()));
        };
        if name.starts_with('.') || name.contains("..") {
            continue;
        }
        let kind = entry
            .file_type()
            .map_err(|error| ReadError::Unreadable(format!("cannot stat shard {name}: {error}")))?;
        if kind.is_dir() || kind.is_symlink() {
            names.push(name);
        }
    }
    names.sort();
    let mut stores = Vec::new();
    for name in names {
        nested_stores(&root.join(&name), &name, 1, &mut stores)?;
    }
    Ok(stores
        .into_iter()
        .map(|name| format!(".chisel/shards/{name}/store"))
        .collect())
}

/// How deep shard folders are looked for. Chisel names a shard after its
/// path prefix, so a shard `src/net` is stored at `.chisel/shards/src/net/`.
const SHARD_DEPTH: usize = 8;

/// `name` when it holds a `store`, then every folder below it that does, so
/// a nested shard's test edges are not missed. Symlinked folders below the
/// first level are not followed.
fn nested_stores(
    dir: &Path,
    name: &str,
    depth: usize,
    out: &mut Vec<String>,
) -> Result<(), ReadError> {
    if fs::symlink_metadata(dir.join("store")).is_ok() {
        out.push(name.to_string());
    }
    if depth >= SHARD_DEPTH {
        return Ok(());
    }
    let Ok(read) = fs::read_dir(dir) else {
        return Ok(());
    };
    let mut kids = Vec::new();
    for entry in read {
        let entry = entry.map_err(|error| {
            ReadError::Unreadable(format!("cannot read .chisel/shards/{name}: {error}"))
        })?;
        let Some(kid) = entry.file_name().to_str().map(str::to_string) else {
            return Err(ReadError::Unreadable("a shard name is not UTF-8".into()));
        };
        if kid.starts_with('.') || kid.contains("..") {
            continue;
        }
        if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            kids.push(kid);
        }
    }
    kids.sort();
    for kid in kids {
        nested_stores(&dir.join(&kid), &format!("{name}/{kid}"), depth + 1, out)?;
    }
    Ok(())
}

fn judge(
    index: &crate::chisel_idx::Index,
    sel: Sel,
) -> Result<(State, String, crate::chisel_idx::Stamp), String> {
    match sel {
        Sel::Test(id) => test_state(index, &id),
        Sel::File(path) => file_state(index, &path),
    }
}

fn test_state(
    index: &crate::chisel_idx::Index,
    id: &str,
) -> Result<(State, String, crate::chisel_idx::Stamp), String> {
    let Some(hit) = index.test(id)? else {
        return Ok((
            State::Missing,
            format!("test {id} has no result"),
            crate::chisel_idx::Stamp::empty(),
        ));
    };
    let (state, detail) = match hit.passed {
        Some(true) => (State::Pass, format!("latest result for {id} passed")),
        Some(false) => (State::Fail, format!("latest result for {id} failed")),
        None => (
            State::Unreadable,
            format!("latest result for {id} has no boolean passed"),
        ),
    };
    Ok((state, detail, hit.stamp))
}

fn file_state(
    index: &crate::chisel_idx::Index,
    file: &str,
) -> Result<(State, String, crate::chisel_idx::Stamp), String> {
    let ids = index.tests_for(file)?;
    if ids.is_empty() {
        return Ok((
            State::Missing,
            format!("no test edge touches {file}"),
            crate::chisel_idx::Stamp::empty(),
        ));
    }
    let mut failed: Vec<&str> = Vec::new();
    let mut waiting: Vec<&str> = Vec::new();
    let mut stamp = crate::chisel_idx::Stamp::empty();
    for id in &ids {
        match index.test(id)? {
            None => waiting.push(id),
            Some(hit) => match hit.passed {
                Some(true) => merge_stamp(&mut stamp, &hit.stamp),
                Some(false) => {
                    failed.push(id);
                    merge_stamp(&mut stamp, &hit.stamp);
                }
                None => {
                    return Ok((
                        State::Unreadable,
                        format!("latest result for {id} has no boolean passed"),
                        crate::chisel_idx::Stamp::empty(),
                    ))
                }
            },
        }
    }
    if !failed.is_empty() {
        Ok((
            State::Fail,
            format!("tests touching {file} failed: {}", some(&failed)),
            stamp,
        ))
    } else if !waiting.is_empty() {
        Ok((
            State::Missing,
            format!("tests touching {file} have no result: {}", some(&waiting)),
            stamp,
        ))
    } else {
        Ok((
            State::Pass,
            format!(
                "{} test(s) touch {file} and the latest result of each passed",
                ids.len()
            ),
            stamp,
        ))
    }
}

fn merge_stamp(into: &mut crate::chisel_idx::Stamp, step: &crate::chisel_idx::Stamp) {
    into.at = match (into.at, step.at) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(left), None) => Some(left),
        (None, right) => right,
    };
    into.commits.extend(step.commits.iter().cloned());
    into.malformed |= step.malformed;
    into.no_commit |= step.no_commit;
}
