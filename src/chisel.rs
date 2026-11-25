//! The Chisel adapter. It reads a `.chisel/store` snapshot (and, when the
//! pointer is exactly that root file, each shard under `.chisel/shards/`)
//! and answers `test:<id>` or `file:<path>`. It never runs Chisel and never
//! writes. Worktree overlays under `.chisel/overlays/` are not read.
//!
//! The latest `test_results` row for an id is the last one in the snapshot.
//! Shard rows come after the root snapshot, shard names in alphabetical
//! order, which matches Chisel walking the list from the end. A `file:`
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
    if is_overlay(rel) {
        return evidence(
            pointer,
            State::NoAdapter,
            "worktree overlays under .chisel/overlays are not read in v1, so this is not proof",
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

fn is_overlay(rel: &str) -> bool {
    let norm = rel.replace('\\', "/");
    let parts: Vec<&str> = norm
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    parts.windows(2).any(|pair| pair == [".chisel", "overlays"])
}

/// The root store, however the pointer spells it (`./`, `//`, `.`, or a
/// symlinked folder): the shards merge into it, so a failing shard result is
/// never hidden.
fn is_root_store(project: &Path, rel: &str) -> bool {
    let norm = rel.replace('\\', "/");
    if norm.trim_start_matches("./") == ".chisel/store" {
        return true;
    }
    match (
        project.join(rel).canonicalize(),
        project.join(".chisel").join("store").canonicalize(),
    ) {
        (Ok(named), Ok(root)) => named == root,
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
            Ok(part) => index.merge(part),
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
    Ok(names
        .into_iter()
        .map(|name| format!(".chisel/shards/{name}/store"))
        .collect())
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
}
