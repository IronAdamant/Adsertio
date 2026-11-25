//! One index of a Chisel snapshot (and, for the root store, its shards).
//!
//! `test:<id>` is a map lookup. `file:<path>` reads the edges that touch that
//! path, not every edge. The scan keeps `test_id`, `passed`, `recorded_at`,
//! `commit`, and `code_id`.

use crate::json::{JsonError, Kind, Number, Parser, Value};
use crate::read::{self, ReadError};
use std::collections::HashMap;

pub struct Stamp {
    pub at: Option<f64>,
    pub commits: Vec<String>,
    pub malformed: bool,
}

impl Stamp {
    pub fn empty() -> Stamp {
        Stamp {
            at: None,
            commits: Vec::new(),
            malformed: false,
        }
    }
}

pub struct TestHit {
    /// `None` when the latest row has no boolean `passed`.
    pub passed: Option<bool>,
    pub stamp: Stamp,
}

enum Rows<T> {
    Absent,
    Bad(String),
    Ready(T),
}

struct TestRec {
    passed: Option<bool>,
    at: Option<f64>,
    at_bad: bool,
    commit_seen: bool,
    commit: Option<String>,
    commit_bad: bool,
}

struct PassCommit {
    commit: Option<String>,
    bad: bool,
}

struct Edges {
    by_code: HashMap<String, Vec<String>>,
    codes: Vec<String>,
}

pub struct Index {
    results: Rows<HashMap<String, TestRec>>,
    edges: Rows<Edges>,
    passes: HashMap<String, PassCommit>,
}

enum Fail {
    Json(JsonError),
    Read(ReadError),
}

impl From<JsonError> for Fail {
    fn from(error: JsonError) -> Fail {
        Fail::Json(error)
    }
}

impl From<ReadError> for Fail {
    fn from(error: ReadError) -> Fail {
        Fail::Read(error)
    }
}

pub fn parse_file(project: &std::path::Path, rel: &str) -> Result<Index, ReadError> {
    let text = read::load_chisel_text(project, rel)?;
    parse(&text, rel)
}

pub fn parse(text: &str, rel: &str) -> Result<Index, ReadError> {
    let mut parser = Parser::new(text);
    match parse_document(&mut parser, rel) {
        Ok(index) => parser
            .finish()
            .map_err(|error| json_read(rel, error))
            .map(|_| index),
        Err(Fail::Json(error)) => Err(json_read(rel, error)),
        Err(Fail::Read(error)) => Err(error),
    }
}

fn json_read(rel: &str, error: JsonError) -> ReadError {
    ReadError::Unreadable(format!("{rel} is not valid JSON: {error}"))
}

fn parse_document(parser: &mut Parser<'_>, rel: &str) -> Result<Index, Fail> {
    parser.skip_ws();
    if !matches!(parser.kind()?, Kind::Object) {
        parser.skip_value(0)?;
        return finish_header(Index::empty(), None, None, rel).map_err(Fail::Read);
    }
    let mut magic: Option<String> = None;
    let mut version: Option<i64> = None;
    let mut index = Index::empty();
    parser.with_object(0, |parser, key, depth| -> Result<(), Fail> {
        match key {
            "magic" => magic = take_string(parser, depth)?,
            "version" => version = take_i64(parser, depth)?,
            "test_results" => index.results = take_results(parser, depth)?,
            "test_edges" => index.edges = take_edges(parser, depth, rel)?,
            "test_passes" => take_passes(parser, depth, &mut index.passes)?,
            "blame" => skip_blame(parser, depth, rel)?,
            _ => parser.skip_value(depth)?,
        }
        Ok(())
    })?;
    if let Rows::Ready(edges) = &mut index.edges {
        edges.finish();
    }
    finish_header(index, magic, version, rel).map_err(Fail::Read)
}

fn finish_header(
    index: Index,
    magic: Option<String>,
    version: Option<i64>,
    rel: &str,
) -> Result<Index, ReadError> {
    match magic.as_deref() {
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
    match version {
        Some(1) => Ok(index),
        Some(found) => Err(ReadError::Unreadable(format!(
            "{rel} has version {found}, not 1"
        ))),
        None => Err(ReadError::Unreadable(format!(
            "{rel} has no version; expected 1"
        ))),
    }
}

impl Index {
    fn empty() -> Index {
        Index {
            results: Rows::Absent,
            edges: Rows::Absent,
            passes: HashMap::new(),
        }
    }

    pub fn merge(&mut self, later: Index) {
        merge_map(
            &mut self.results,
            later.results,
            "test_results",
            |left, right| {
                for (id, rec) in right {
                    left.insert(id, rec);
                }
            },
        );
        merge_map(&mut self.edges, later.edges, "test_edges", |left, right| {
            for (code, tests) in right.by_code {
                let list = left.by_code.entry(code).or_default();
                for test in tests {
                    if !list.iter().any(|known| known == &test) {
                        list.push(test);
                    }
                }
            }
            left.finish();
        });
        for (id, pass) in later.passes {
            self.passes.entry(id).or_insert(pass);
        }
    }

    pub fn test(&self, id: &str) -> Result<Option<TestHit>, String> {
        let rows = ready(&self.results, "test_results")?;
        let Some(rec) = rows.get(&normalize_test_id(id)) else {
            return Ok(None);
        };
        Ok(Some(TestHit {
            passed: rec.passed,
            stamp: self.stamp(id, rec),
        }))
    }

    /// Normalised test ids that touch `file`, in code-id order.
    pub fn tests_for(&self, file: &str) -> Result<Vec<String>, String> {
        let edges = ready(&self.edges, "test_edges")?;
        let file = file.trim_matches('/');
        if file.is_empty() {
            return Ok(Vec::new());
        }
        let mut ids = Vec::new();
        if let Some(list) = edges.by_code.get(file) {
            push_unique(&mut ids, list);
        }
        for prefix in [format!("{file}:"), format!("{file}/")] {
            let start = edges
                .codes
                .partition_point(|code| code.as_str() < prefix.as_str());
            for code in edges.codes[start..]
                .iter()
                .take_while(|code| code.starts_with(&prefix))
            {
                if let Some(list) = edges.by_code.get(code) {
                    push_unique(&mut ids, list);
                }
            }
        }
        Ok(ids)
    }

    fn stamp(&self, id: &str, rec: &TestRec) -> Stamp {
        if rec.at_bad {
            return Stamp {
                at: None,
                commits: Vec::new(),
                malformed: true,
            };
        }
        if rec.commit_bad {
            return Stamp {
                at: rec.at,
                commits: Vec::new(),
                malformed: true,
            };
        }
        if rec.commit_seen {
            return Stamp {
                at: rec.at,
                commits: rec.commit.iter().cloned().collect(),
                malformed: false,
            };
        }
        match self.passes.get(&normalize_test_id(id)) {
            Some(pass) if pass.bad => Stamp {
                at: rec.at,
                commits: Vec::new(),
                malformed: true,
            },
            Some(pass) => Stamp {
                at: rec.at,
                commits: pass.commit.iter().cloned().collect(),
                malformed: false,
            },
            None => Stamp {
                at: rec.at,
                commits: Vec::new(),
                malformed: false,
            },
        }
    }
}

fn push_unique(ids: &mut Vec<String>, list: &[String]) {
    for id in list {
        if !ids.iter().any(|known| known == id) {
            ids.push(id.clone());
        }
    }
}

fn merge_map<T>(into: &mut Rows<T>, extra: Rows<T>, name: &str, join: impl FnOnce(&mut T, T)) {
    match extra {
        Rows::Bad(message) => *into = Rows::Bad(message),
        Rows::Absent => {
            if !matches!(into, Rows::Bad(_)) {
                *into = Rows::Bad(missing(name));
            }
        }
        Rows::Ready(right) => match into {
            Rows::Ready(left) => join(left, right),
            Rows::Bad(_) => {}
            Rows::Absent => *into = Rows::Bad(missing(name)),
        },
    }
}

fn ready<'a, T>(rows: &'a Rows<T>, name: &str) -> Result<&'a T, String> {
    match rows {
        Rows::Ready(value) => Ok(value),
        Rows::Absent => Err(missing(name)),
        Rows::Bad(message) => Err(message.clone()),
    }
}

fn missing(name: &str) -> String {
    format!("missing {name}, or it is not an array. A damaged store is not treated as empty")
}

fn not_object(name: &str, index: usize) -> String {
    format!("{name}[{index}] is not an object")
}

/// Same shape Chisel stores. `tests/a.rs::t1` and `tests/a.rs:t1` meet.
pub fn normalize_test_id(id: &str) -> String {
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

fn take_string(parser: &mut Parser<'_>, depth: usize) -> Result<Option<String>, JsonError> {
    if matches!(parser.kind()?, Kind::String) {
        Ok(Some(parser.take_string()?))
    } else {
        parser.skip_value(depth)?;
        Ok(None)
    }
}

fn take_i64(parser: &mut Parser<'_>, depth: usize) -> Result<Option<i64>, JsonError> {
    if matches!(parser.kind()?, Kind::Number) {
        Ok(parser.take_number()?.as_i64())
    } else {
        parser.skip_value(depth)?;
        Ok(None)
    }
}

fn take_results(
    parser: &mut Parser<'_>,
    depth: usize,
) -> Result<Rows<HashMap<String, TestRec>>, Fail> {
    if !matches!(parser.kind()?, Kind::Array) {
        parser.skip_value(depth)?;
        return Ok(Rows::Bad(missing("test_results")));
    }
    let mut rows = HashMap::new();
    let mut poison: Option<String> = None;
    parser.with_array(depth, |parser, index| {
        if !matches!(parser.kind()?, Kind::Object) {
            if poison.is_none() {
                poison = Some(not_object("test_results", index));
            }
            return parser.skip_value(depth + 1);
        }
        let rec = parse_result(parser, depth + 1)?;
        if poison.is_none() {
            if let Some(id) = rec.0 {
                rows.insert(id, rec.1);
            }
        }
        Ok(())
    })?;
    Ok(match poison {
        Some(message) => Rows::Bad(message),
        None => Rows::Ready(rows),
    })
}

fn parse_result(
    parser: &mut Parser<'_>,
    depth: usize,
) -> Result<(Option<String>, TestRec), JsonError> {
    let mut id = None;
    let mut passed: Option<Option<bool>> = None;
    let mut at_seen = false;
    let mut at_number: Option<Number> = None;
    let mut commit_seen = false;
    let mut commit: Option<String> = None;
    let mut commit_bad = false;
    parser.with_object(depth, |parser, key, depth| {
        match key {
            "test_id" => id = take_string(parser, depth)?,
            "passed" => {
                if matches!(parser.kind()?, Kind::Bool) {
                    passed = Some(Some(parser.take_bool()?));
                } else {
                    passed = Some(None);
                    parser.skip_value(depth)?;
                }
            }
            "recorded_at" => {
                at_seen = true;
                if matches!(parser.kind()?, Kind::Number) {
                    at_number = Some(parser.take_number()?);
                } else {
                    parser.skip_value(depth)?;
                }
            }
            "commit" => {
                commit_seen = true;
                if matches!(parser.kind()?, Kind::String) {
                    let text = parser.take_string()?;
                    let (parsed, bad) = crate::fresh::read_commit(Some(&Value::String(text)));
                    commit = parsed;
                    commit_bad = bad;
                } else {
                    commit_bad = true;
                    parser.skip_value(depth)?;
                }
            }
            _ => parser.skip_value(depth)?,
        }
        Ok(())
    })?;
    let (at, at_bad) = read_at(at_seen, at_number);
    let id = id.map(|text| normalize_test_id(&text));
    Ok((
        id,
        TestRec {
            passed: passed.flatten(),
            at,
            at_bad,
            commit_seen,
            commit,
            commit_bad,
        },
    ))
}

fn read_at(seen: bool, number: Option<Number>) -> (Option<f64>, bool) {
    if !seen {
        return (None, false);
    }
    match number {
        Some(number) => {
            let value = number.as_value();
            let (at, bad) = crate::fresh::read_time(Some(&value));
            (at, bad)
        }
        None => (None, true),
    }
}

fn take_edges(parser: &mut Parser<'_>, depth: usize, rel: &str) -> Result<Rows<Edges>, Fail> {
    match parser.kind()? {
        Kind::Array => stream_edges(parser, depth),
        Kind::Object => {
            let value = parser.take_value(depth)?;
            let rows = read::rows_from_table(value, rel, "test_edges")?;
            Ok(edges_from_values(&rows))
        }
        _ => {
            parser.skip_value(depth)?;
            Err(Fail::Read(ReadError::Unreadable(format!(
                "{rel} test_edges is neither a list of rows nor a compact table"
            ))))
        }
    }
}

fn stream_edges(parser: &mut Parser<'_>, depth: usize) -> Result<Rows<Edges>, Fail> {
    let mut edges = Edges {
        by_code: HashMap::new(),
        codes: Vec::new(),
    };
    let mut poison: Option<String> = None;
    parser.with_array(depth, |parser, index| {
        if !matches!(parser.kind()?, Kind::Object) {
            if poison.is_none() {
                poison = Some(not_object("test_edges", index));
            }
            return parser.skip_value(depth + 1);
        }
        let parsed = parse_edge(parser, depth + 1)?;
        if poison.is_none() {
            match parsed {
                Ok((code, test)) => edges.add(code, test),
                Err(message) => poison = Some(message),
            }
        }
        Ok(())
    })?;
    Ok(match poison {
        Some(message) => Rows::Bad(message),
        None => {
            edges.finish();
            Rows::Ready(edges)
        }
    })
}

fn edges_from_values(rows: &[Value]) -> Rows<Edges> {
    let mut edges = Edges {
        by_code: HashMap::new(),
        codes: Vec::new(),
    };
    for (index, row) in rows.iter().enumerate() {
        if !matches!(row, Value::Object(_)) {
            return Rows::Bad(not_object("test_edges", index));
        }
        match edge_from_value(row) {
            Ok((code, test)) => edges.add(code, test),
            Err(message) => return Rows::Bad(message),
        }
    }
    edges.finish();
    Rows::Ready(edges)
}

fn parse_edge(
    parser: &mut Parser<'_>,
    depth: usize,
) -> Result<Result<(String, String), String>, JsonError> {
    let mut code: Option<Option<String>> = None;
    let mut test: Option<Option<String>> = None;
    parser.with_object(depth, |parser, key, depth| {
        match key {
            "code_id" => code = Some(take_string(parser, depth)?),
            "test_id" => test = Some(take_string(parser, depth)?),
            _ => parser.skip_value(depth)?,
        }
        Ok(())
    })?;
    Ok(edge_parts(code.flatten(), test.flatten()))
}

fn edge_from_value(row: &Value) -> Result<(String, String), String> {
    let code = match row.get("code_id") {
        Some(Value::String(text)) => Some(text.clone()),
        Some(_) => None,
        None => None,
    };
    let test = match row.get("test_id") {
        Some(Value::String(text)) => Some(text.clone()),
        _ => None,
    };
    // A non-string code_id is the same as a missing one for the error text
    // the file selector already uses: it only proceeds when the field is text.
    if row.get("code_id").is_some() && code.is_none() {
        return Err("a test edge has no code_id".into());
    }
    edge_parts(code, test)
}

fn edge_parts(code: Option<String>, test: Option<String>) -> Result<(String, String), String> {
    let Some(code) = code else {
        return Err("a test edge has no code_id".into());
    };
    let code = code.replace('\\', "/");
    let Some(test) = test else {
        return Err(format!("a test edge for {code} has no test_id"));
    };
    Ok((code, normalize_test_id(&test)))
}

impl Edges {
    fn add(&mut self, code: String, test: String) {
        let list = self.by_code.entry(code).or_default();
        if !list.iter().any(|known| known == &test) {
            list.push(test);
        }
    }

    fn finish(&mut self) {
        self.codes = self.by_code.keys().cloned().collect();
        self.codes.sort();
    }
}

fn take_passes(
    parser: &mut Parser<'_>,
    depth: usize,
    into: &mut HashMap<String, PassCommit>,
) -> Result<(), JsonError> {
    if !matches!(parser.kind()?, Kind::Object) {
        return parser.skip_value(depth);
    }
    parser.with_object(depth, |parser, key, depth| {
        let id = normalize_test_id(key);
        let pass = if matches!(parser.kind()?, Kind::Object) {
            parse_pass(parser, depth)?
        } else {
            parser.skip_value(depth)?;
            PassCommit {
                commit: None,
                bad: false,
            }
        };
        into.entry(id).or_insert(pass);
        Ok(())
    })
}

fn parse_pass(parser: &mut Parser<'_>, depth: usize) -> Result<PassCommit, JsonError> {
    let mut commit: Option<Option<String>> = None;
    let mut bad = false;
    parser.with_object(depth, |parser, key, depth| {
        if key == "commit" {
            if matches!(parser.kind()?, Kind::String) {
                commit = Some(Some(parser.take_string()?));
            } else {
                bad = true;
                parser.skip_value(depth)?;
            }
        } else {
            parser.skip_value(depth)?;
        }
        Ok(())
    })?;
    if bad {
        return Ok(PassCommit {
            commit: None,
            bad: true,
        });
    }
    let (commit, bad) = match commit.flatten() {
        None => (None, false),
        Some(text) => {
            let (commit, bad) = crate::fresh::read_commit(Some(&Value::String(text)));
            (commit, bad)
        }
    };
    Ok(PassCommit { commit, bad })
}

fn skip_blame(parser: &mut Parser<'_>, depth: usize, rel: &str) -> Result<(), Fail> {
    match parser.kind()? {
        Kind::Array => {
            parser.skip_value(depth)?;
            Ok(())
        }
        Kind::Object => {
            let value = parser.take_value(depth)?;
            read::rows_from_table(value, rel, "blame")?;
            Ok(())
        }
        _ => {
            parser.skip_value(depth)?;
            Err(Fail::Read(ReadError::Unreadable(format!(
                "{rel} blame is neither a list of rows nor a compact table"
            ))))
        }
    }
}
