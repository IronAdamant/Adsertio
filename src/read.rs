//! Read a Trammel or Chisel snapshot without writing, and without treating a
//! damaged file as an empty store.
//!
//! The lock file is taken shared when it already exists, and never created.
//! A journal line replaces the Trammel document only when that line is JSON
//! and carries `schema_version`. A line that does not is an error, not a skip.
//! A Chisel compact table that cannot be expanded is an error, not rows we
//! pretend are absent. A file that starts with the SQLite header is refused
//! with a pointer at that tool's migrate command.

use crate::fsx;
use crate::json::{self, Value};
use crate::lock::{self, SharedGuard};
use std::fs;
use std::path::{Path, PathBuf};

/// Evidence files larger than this are not read.
pub const MAX_EVIDENCE_BYTES: u64 = 64 * 1024 * 1024;

const COMPACT_TABLES: &[&str] = &["test_edges", "blame"];

#[derive(Debug, Clone)]
pub enum ReadError {
    Missing(String),
    Unreadable(String),
}

impl ReadError {
    pub fn text(&self) -> &str {
        match self {
            ReadError::Missing(text) | ReadError::Unreadable(text) => text,
        }
    }

    pub fn is_missing(&self) -> bool {
        matches!(self, ReadError::Missing(_))
    }
}

#[derive(Debug)]
pub struct Snapshot {
    pub doc: Value,
    /// A journal line replaced the snapshot. The document is that line.
    pub journal_applied: bool,
}

/// What to tell a caller who pointed at a SQLite file. There is no SQLite
/// reader here: both tools publish a JSON snapshot, and the old database is
/// left for that tool's own migrate command.
pub fn sqlite_message(tool: &str) -> String {
    match tool {
        "trammel" => "this file starts with the SQLite header, so it is not the JSON snapshot. Run trammel migrate and point at .trammel/store".into(),
        "chisel" => "this file starts with the SQLite header, so it is not the JSON snapshot. Run chisel migrate and point at .chisel/store".into(),
        _ => "this file starts with the SQLite header, which Adsertio does not read".into(),
    }
}

pub fn is_sqlite(bytes: &[u8]) -> bool {
    bytes.starts_with(b"SQLite format 3")
}

/// The live Trammel document: the snapshot, then any journal lines that
/// carry `schema_version`. A damaged snapshot is [`ReadError::Unreadable`],
/// never an empty plan list, even when a journal sits beside it.
pub fn read_trammel(project: &Path, rel: &str) -> Result<Snapshot, ReadError> {
    let loaded = load_trammel_text(project, rel)?;
    let doc = parse_json(&loaded.text, rel)?;
    check_trammel_schema(&doc, rel)?;
    Ok(Snapshot {
        doc,
        journal_applied: loaded.journal_applied,
    })
}

/// The JSON text the Trammel adapter judges: the snapshot, or the last
/// journal line that carries `schema_version`. The snapshot is checked as
/// JSON even when a journal line replaces it. The caller checks
/// `schema_version`.
pub struct TrammelText {
    pub text: String,
    pub journal_applied: bool,
}

pub fn load_trammel_text(project: &Path, rel: &str) -> Result<TrammelText, ReadError> {
    let path = open_contained(project, rel)?;
    let parent = parent_dir(&project.join(rel))?;
    let _guard = lock_sibling(project, &parent, "lock")?;
    let bytes = read_bytes(&path, rel)?;
    if is_sqlite(&bytes) {
        return Err(ReadError::Unreadable(sqlite_message("trammel")));
    }
    let text = decode(&bytes, rel)?;
    // A damaged snapshot is unreadable even when a journal sits beside it.
    // Validating here does not build a value; the caller parses the document
    // it actually judges.
    json_document(&text, rel)?;
    let (text, journal_applied) = match journal_line(project, &parent)? {
        Some(line) => (line, true),
        None => (text, false),
    };
    Ok(TrammelText {
        text,
        journal_applied,
    })
}

pub fn check_trammel_schema(doc: &Value, rel: &str) -> Result<(), ReadError> {
    match doc.get("schema_version").and_then(Value::as_i64) {
        Some(1) => Ok(()),
        Some(version) => Err(ReadError::Unreadable(format!(
            "{rel} has schema_version {version}, which this reader does not understand"
        ))),
        None => Err(ReadError::Unreadable(format!(
            "{rel} has no schema_version after the snapshot and journal were read. A damaged store is not treated as empty"
        ))),
    }
}

/// One Chisel snapshot as text, after the shared lock, the size cap, and the
/// SQLite check. Magic and compact tables are the caller's job.
pub fn load_chisel_text(project: &Path, rel: &str) -> Result<String, ReadError> {
    let path = open_contained(project, rel)?;
    let parent = parent_dir(&project.join(rel))?;
    let _guard = lock_sibling(project, &parent, "chisel.lock")?;
    let bytes = read_bytes(&path, rel)?;
    if is_sqlite(&bytes) {
        return Err(ReadError::Unreadable(sqlite_message("chisel")));
    }
    decode(&bytes, rel)
}

/// An array of row objects, or a compact table expanded to those rows.
pub fn rows_from_table(value: Value, label: &str, key: &str) -> Result<Vec<Value>, ReadError> {
    match value {
        Value::Array(rows) => Ok(rows),
        other if other.get("table").is_some() => expand_table(&other)
            .map_err(|e| ReadError::Unreadable(format!("{label} {key} compact table: {e}"))),
        _ => Err(ReadError::Unreadable(format!(
            "{label} {key} is neither a list of rows nor a compact table"
        ))),
    }
}

/// One JSON file inside the project: size cap, SQLite header, UTF-8, then
/// the strict parser. A folder is [`ReadError::Unreadable`].
pub fn read_json(project: &Path, rel: &str, tool: &str) -> Result<Value, ReadError> {
    let path = open_contained(project, rel)?;
    read_json_file(&path, rel, tool)
}

/// Parse a file that the caller has already checked is a regular file inside
/// the project. Symlinks still go through [`read_json`], which resolves them.
pub fn read_json_file(path: &Path, label: &str, tool: &str) -> Result<Value, ReadError> {
    let bytes = read_bytes(path, label)?;
    if is_sqlite(&bytes) {
        return Err(ReadError::Unreadable(sqlite_message(tool)));
    }
    let text = decode(&bytes, label)?;
    parse_json(&text, label)
}

/// One Chisel snapshot. Compact `test_edges` and `blame` tables are expanded
/// to plain rows. Wrong magic, or a compact table that will not expand, is
/// [`ReadError::Unreadable`].
pub fn read_chisel(project: &Path, rel: &str) -> Result<Snapshot, ReadError> {
    let path = open_contained(project, rel)?;
    let parent = parent_dir(&project.join(rel))?;
    let _guard = lock_sibling(project, &parent, "chisel.lock")?;
    let bytes = read_bytes(&path, rel)?;
    if is_sqlite(&bytes) {
        return Err(ReadError::Unreadable(sqlite_message("chisel")));
    }
    let text = decode(&bytes, rel)?;
    let mut doc = parse_json(&text, rel)?;
    match doc.get("magic").and_then(Value::as_str) {
        Some("chisel-store") => {}
        Some(other) => {
            return Err(ReadError::Unreadable(format!(
                "{rel} has magic \"{other}\", not chisel-store"
            )))
        }
        None => {
            return Err(ReadError::Unreadable(format!(
                "{rel} has no magic; expected chisel-store (snapshot magic mismatch)"
            )))
        }
    }
    match doc.get("version").and_then(Value::as_i64) {
        Some(1) => {}
        Some(version) => {
            return Err(ReadError::Unreadable(format!(
                "{rel} has version {version}, not 1"
            )))
        }
        None => {
            return Err(ReadError::Unreadable(format!(
                "{rel} has no version; expected 1"
            )))
        }
    }
    expand_compact(&mut doc, rel)?;
    Ok(Snapshot {
        doc,
        journal_applied: false,
    })
}

fn open_contained(project: &Path, rel: &str) -> Result<PathBuf, ReadError> {
    match fsx::project_file(project, rel) {
        Ok(path) => Ok(path),
        Err(text) if text.ends_with("does not exist") => Err(ReadError::Missing(text)),
        Err(text) => Err(ReadError::Unreadable(text)),
    }
}

/// The folder of the path as the pointer names it, not of a symlink's target:
/// the other tool keeps its lock and journal beside the name it writes.
fn parent_dir(path: &Path) -> Result<PathBuf, ReadError> {
    path.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| ReadError::Unreadable(format!("{} has no parent folder", path.display())))
}

fn lock_sibling(project: &Path, dir: &Path, name: &str) -> Result<Option<SharedGuard>, ReadError> {
    let path = dir.join(name);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ReadError::Unreadable(format!("cannot stat {name}: {e}"))),
        Ok(meta) if meta.file_type().is_symlink() => {
            contained(project, &path)?;
            lock::try_shared(&path)
                .map(Some)
                .map_err(ReadError::Unreadable)
        }
        Ok(meta) if meta.is_file() => lock::try_shared(&path)
            .map(Some)
            .map_err(ReadError::Unreadable),
        Ok(_) => Err(ReadError::Unreadable(format!(
            "{name} is not a file, so a shared lock cannot be taken"
        ))),
    }
}

fn contained(project: &Path, path: &Path) -> Result<PathBuf, ReadError> {
    let root = project
        .canonicalize()
        .map_err(|e| ReadError::Unreadable(format!("cannot resolve the project folder: {e}")))?;
    let canon = path
        .canonicalize()
        .map_err(|e| ReadError::Unreadable(format!("cannot resolve {}: {e}", path.display())))?;
    if canon.starts_with(&root) {
        Ok(canon)
    } else {
        Err(ReadError::Unreadable(format!(
            "{} resolves outside the project",
            path.display()
        )))
    }
}

fn read_bytes(path: &Path, label: &str) -> Result<Vec<u8>, ReadError> {
    let meta = fs::metadata(path)
        .map_err(|e| ReadError::Unreadable(format!("cannot stat {label}: {e}")))?;
    if !meta.is_file() {
        return Err(ReadError::Unreadable(format!(
            "{label} {}",
            crate::fsx::not_a_file(&meta)
        )));
    }
    if meta.len() > MAX_EVIDENCE_BYTES {
        return Err(ReadError::Unreadable(format!(
            "{label} is larger than {MAX_EVIDENCE_BYTES} bytes"
        )));
    }
    let bytes =
        fs::read(path).map_err(|e| ReadError::Unreadable(format!("cannot read {label}: {e}")))?;
    if bytes.len() as u64 > MAX_EVIDENCE_BYTES {
        return Err(ReadError::Unreadable(format!(
            "{label} is larger than {MAX_EVIDENCE_BYTES} bytes"
        )));
    }
    Ok(bytes)
}

fn decode(bytes: &[u8], label: &str) -> Result<String, ReadError> {
    String::from_utf8(bytes.to_vec())
        .map_err(|_| ReadError::Unreadable(format!("{label} is not valid UTF-8")))
}

fn parse_json(text: &str, label: &str) -> Result<Value, ReadError> {
    json::parse(text).map_err(|e| ReadError::Unreadable(format!("{label} is not valid JSON: {e}")))
}

fn json_document(text: &str, label: &str) -> Result<(), ReadError> {
    let mut parser = json::Parser::new(text);
    parser
        .skip_value(0)
        .and_then(|_| parser.finish())
        .map_err(|e| ReadError::Unreadable(format!("{label} is not valid JSON: {e}")))
}

/// The last non-empty journal line that is JSON and carries `schema_version`.
/// Empty lines are ignored. Any other line fails the read. Absent journal is
/// `Ok(None)`. This never deletes the journal.
fn journal_line(project: &Path, dir: &Path) -> Result<Option<String>, ReadError> {
    let journal = dir.join("journal");
    match fs::symlink_metadata(&journal) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(ReadError::Unreadable(format!("cannot stat journal: {e}"))),
        Ok(_) => {}
    }
    contained(project, &journal)?;
    let bytes = read_bytes(&journal, "journal")?;
    if is_sqlite(&bytes) {
        return Err(ReadError::Unreadable(sqlite_message("trammel")));
    }
    let text = decode(&bytes, "journal")?;
    let mut chosen: Option<String> = None;
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value = json::parse(line).map_err(|e| {
            ReadError::Unreadable(format!("journal line {} is not valid JSON: {e}", index + 1))
        })?;
        if value.get("schema_version").is_none() {
            return Err(ReadError::Unreadable(format!(
                "journal line {} has no schema_version",
                index + 1
            )));
        }
        chosen = Some(line.to_string());
    }
    Ok(chosen)
}

fn expand_compact(doc: &mut Value, label: &str) -> Result<(), ReadError> {
    for key in COMPACT_TABLES {
        let Some(value) = doc.take(key) else {
            continue;
        };
        match value {
            Value::Array(rows) => doc.insert(key, Value::Array(rows)),
            other if other.get("table").is_some() => {
                let rows = expand_table(&other).map_err(|e| {
                    ReadError::Unreadable(format!("{label} {key} compact table: {e}"))
                })?;
                doc.insert(key, Value::Array(rows));
            }
            _ => {
                return Err(ReadError::Unreadable(format!(
                    "{label} {key} is neither a list of rows nor a compact table"
                )))
            }
        }
    }
    Ok(())
}

/// The compact shape Chisel writes:
/// `{"table":1,"cols":[...],"dict":[...],"strings":[...],"rows":[[...],...]}`.
/// A string column stores an index into `strings`. Anything short, long, or
/// the wrong type is an error.
fn expand_table(table: &Value) -> Result<Vec<Value>, String> {
    if table.get("table").and_then(Value::as_i64) != Some(1) {
        return Err("table is not 1".into());
    }
    let cols = string_list(table, "cols")?;
    let dict = bool_list(table, "dict")?;
    let strings = table
        .get("strings")
        .and_then(Value::as_array)
        .ok_or("strings is missing")?;
    let rows = table
        .get("rows")
        .and_then(Value::as_array)
        .ok_or("rows is missing")?;
    if dict.len() != cols.len() {
        return Err("dict length does not match cols".into());
    }
    let mut out = Vec::with_capacity(rows.len());
    for (row_index, row) in rows.iter().enumerate() {
        let Some(cells) = row.as_array() else {
            return Err(format!("row {row_index} is not an array"));
        };
        if cells.len() != cols.len() {
            return Err(format!(
                "row {row_index} has {} cells, not {}",
                cells.len(),
                cols.len()
            ));
        }
        let mut pairs = Vec::with_capacity(cols.len());
        for (cell_index, ((col, cell), text)) in cols.iter().zip(cells).zip(&dict).enumerate() {
            let value = if *text {
                let index = cell.as_i64().ok_or_else(|| {
                    format!("row {row_index} column {cell_index} is not a string index")
                })?;
                if index < 0 {
                    return Err(format!("row {row_index} has a negative string index"));
                }
                let entry = strings.get(index as usize).ok_or_else(|| {
                    format!("row {row_index} string index {index} is out of range")
                })?;
                let Some(text) = entry.as_str() else {
                    return Err(format!("strings[{index}] is not a string"));
                };
                Value::str(text)
            } else {
                cell.clone()
            };
            pairs.push((col.clone(), value));
        }
        out.push(Value::Object(pairs));
    }
    Ok(out)
}

fn string_list(table: &Value, key: &str) -> Result<Vec<String>, String> {
    let Some(items) = table.get(key).and_then(Value::as_array) else {
        return Err(format!("{key} is missing"));
    };
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{key} holds a non-string"))
        })
        .collect()
}

fn bool_list(table: &Value, key: &str) -> Result<Vec<bool>, String> {
    let Some(items) = table.get(key).and_then(Value::as_array) else {
        return Err(format!("{key} is missing"));
    };
    items
        .iter()
        .map(|item| {
            item.as_bool()
                .ok_or_else(|| format!("{key} holds a non-boolean"))
        })
        .collect()
}
