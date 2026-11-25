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
use crate::json::Value;
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
    let docs = match load(project, rel) {
        Ok(docs) => docs,
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
    let (at, commits) = freshness(&docs, &sel);
    match judge(&docs, sel) {
        Ok((state, detail)) => {
            let mut found = evidence(pointer, state, detail);
            if found.state == State::Pass {
                found.at = at;
                found.commits = commits;
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

fn is_root_store(rel: &str) -> bool {
    let norm = rel.replace('\\', "/");
    let norm = norm.trim_start_matches("./");
    norm == ".chisel/store"
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

/// Same shape Chisel stores. `tests/a.rs::t1` and `tests/a.rs:t1` meet.
fn normalize_test_id(id: &str) -> String {
    let id = id.trim();
    let Some((path, rest)) = id.split_once("::") else {
        return id.to_string();
    };
    if path.contains('/') || path.contains('.') {
        format!("{path}:{}", rest.replace("::", "."))
    } else {
        id.replace("::", ":")
    }
}

fn load(project: &Path, rel: &str) -> Result<Vec<Value>, ReadError> {
    let mut docs = vec![read::read_chisel(project, rel)?.doc];
    if !is_root_store(rel) {
        return Ok(docs);
    }
    for rel_store in shard_stores(project)? {
        match read::read_chisel(project, &rel_store) {
            Ok(snapshot) => docs.push(snapshot.doc),
            Err(ReadError::Missing(_)) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(docs)
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

fn judge(docs: &[Value], sel: Sel) -> Result<(State, String), String> {
    match sel {
        Sel::Test(id) => test_state(docs, &id),
        Sel::File(path) => file_state(docs, &path),
    }
}

fn table<'a>(docs: &'a [Value], name: &str) -> Result<Vec<&'a Value>, String> {
    let mut out = Vec::new();
    for doc in docs {
        let Some(items) = doc.get(name).and_then(Value::as_array) else {
            return Err(format!(
                "missing {name}, or it is not an array. A damaged store is not treated as empty"
            ));
        };
        for (index, item) in items.iter().enumerate() {
            if !matches!(item, Value::Object(_)) {
                return Err(format!("{name}[{index}] is not an object"));
            }
            out.push(item);
        }
    }
    Ok(out)
}

fn test_state(docs: &[Value], id: &str) -> Result<(State, String), String> {
    let rows = table(docs, "test_results")?;
    let Some(row) = latest(&rows, id) else {
        return Ok((State::Missing, format!("test {id} has no result")));
    };
    Ok(result_state(row, id))
}

fn latest<'a>(rows: &[&'a Value], id: &str) -> Option<&'a Value> {
    let want = normalize_test_id(id);
    rows.iter()
        .rev()
        .find(|row| {
            row.get("test_id")
                .and_then(Value::as_str)
                .is_some_and(|stored| normalize_test_id(stored) == want)
        })
        .copied()
}

fn result_state(row: &Value, id: &str) -> (State, String) {
    match row.get("passed").and_then(Value::as_bool) {
        Some(true) => (State::Pass, format!("latest result for {id} passed")),
        Some(false) => (State::Fail, format!("latest result for {id} failed")),
        None => (
            State::Unreadable,
            format!("latest result for {id} has no boolean passed"),
        ),
    }
}

fn file_state(docs: &[Value], file: &str) -> Result<(State, String), String> {
    let edges = table(docs, "test_edges")?;
    let mut ids: Vec<String> = Vec::new();
    for edge in edges {
        let Some(code_id) = edge.get("code_id").and_then(Value::as_str) else {
            return Err("a test edge has no code_id".into());
        };
        if !file_matches(code_id, file) {
            continue;
        }
        let Some(test_id) = edge.get("test_id").and_then(Value::as_str) else {
            return Err(format!("a test edge for {code_id} has no test_id"));
        };
        let test_id = normalize_test_id(test_id);
        if !ids.iter().any(|known| known == &test_id) {
            ids.push(test_id);
        }
    }
    if ids.is_empty() {
        return Ok((State::Missing, format!("no test edge touches {file}")));
    }
    let rows = table(docs, "test_results")?;
    let mut failed: Vec<&str> = Vec::new();
    let mut waiting: Vec<&str> = Vec::new();
    for id in &ids {
        match latest(&rows, id) {
            None => waiting.push(id),
            Some(row) => match result_state(row, id) {
                (State::Pass, _) => {}
                (State::Fail, _) => failed.push(id),
                (state, detail) => return Ok((state, detail)),
            },
        }
    }
    if !failed.is_empty() {
        Ok((
            State::Fail,
            format!("tests touching {file} failed: {}", failed.join(", ")),
        ))
    } else if !waiting.is_empty() {
        Ok((
            State::Missing,
            format!(
                "tests touching {file} have no result: {}",
                waiting.join(", ")
            ),
        ))
    } else {
        Ok((
            State::Pass,
            format!(
                "{} test(s) touch {file} and the latest result of each passed",
                ids.len()
            ),
        ))
    }
}

fn freshness(docs: &[Value], sel: &Sel) -> (Option<f64>, Vec<String>) {
    match sel {
        Sel::Test(id) => {
            let Ok(rows) = table(docs, "test_results") else {
                return (None, Vec::new());
            };
            let Some(row) = latest(&rows, id) else {
                return (None, Vec::new());
            };
            (
                row.get("recorded_at").and_then(Value::as_f64),
                commits_for(docs, id, row),
            )
        }
        Sel::File(path) => {
            let Ok(edges) = table(docs, "test_edges") else {
                return (None, Vec::new());
            };
            let Ok(rows) = table(docs, "test_results") else {
                return (None, Vec::new());
            };
            let mut ids = Vec::new();
            for edge in edges {
                let Some(code_id) = edge.get("code_id").and_then(Value::as_str) else {
                    continue;
                };
                if !file_matches(code_id, path) {
                    continue;
                }
                if let Some(test_id) = edge.get("test_id").and_then(Value::as_str) {
                    let test_id = normalize_test_id(test_id);
                    if !ids.iter().any(|known| known == &test_id) {
                        ids.push(test_id);
                    }
                }
            }
            let mut at = None;
            let mut commits = Vec::new();
            for id in ids {
                if let Some(row) = latest(&rows, &id) {
                    at = older(at, row.get("recorded_at").and_then(Value::as_f64));
                    commits.extend(commits_for(docs, &id, row));
                }
            }
            (at, commits)
        }
    }
}

fn commits_for(docs: &[Value], id: &str, row: &Value) -> Vec<String> {
    if let Some(text) = commit_text(row.get("commit")) {
        return vec![text];
    }
    let want = normalize_test_id(id);
    for doc in docs {
        let Some(Value::Object(pairs)) = doc.get("test_passes") else {
            continue;
        };
        for (key, value) in pairs {
            if normalize_test_id(key) == want {
                if let Some(text) = commit_text(value.get("commit")) {
                    return vec![text];
                }
            }
        }
    }
    Vec::new()
}

fn commit_text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn older(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, b) => b,
    }
}

fn file_matches(code_id: &str, file: &str) -> bool {
    let code = code_id.replace('\\', "/");
    let file = file.trim_matches('/');
    if file.is_empty() {
        return false;
    }
    code == file || code.starts_with(&format!("{file}:")) || code.starts_with(&format!("{file}/"))
}
