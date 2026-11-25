//! The Probatio adapter. It reads a run's output folder. `results/*.json` is
//! the source of truth. `tally.json` is a cache: when the two disagree, the
//! results stand and the detail says so. It never runs Probatio and never
//! writes.
//!
//! A whole run fails only when some mutant survived. It passes only when at
//! least one mutant was killed and none survived. `timeout`, `no coverage`,
//! and `unviable` (including `killed` with `cause` `build`) do not survive
//! and do not kill. On their own, with no kill and no survivor, the run is
//! inconclusive. `flaky` and `error` block a pass: the run is inconclusive.
//! An outcome Probatio does not write makes the run unreadable. When
//! `run.json` names a mutant list inside the project, a pass also needs a
//! result for every mutant in it: a run still going, or a capped run, is
//! inconclusive. One broken results file makes the run unreadable; it is not
//! dropped. `#ledger` counts entries and is never a pass: an unchecked ledger
//! is not proof.

use crate::claim;
use crate::fsx;
use crate::json::Value;
use crate::read::{self, ReadError};
use crate::verdict::{Evidence, State};
use std::fs;
use std::path::{Path, PathBuf};

struct Mutant {
    id: String,
    outcome: String,
}

/// Every outcome Probatio writes (`Verdict::as_str` in Probatio).
const KNOWN_OUTCOMES: &[&str] = &[
    "killed",
    "survived",
    "timeout",
    "no coverage",
    "unviable",
    "flaky",
    "error",
];

/// Outcomes that mean the run did not settle that mutant.
const BLOCKING_OUTCOMES: &[&str] = &["flaky", "error"];

pub fn evaluate(project: &Path, pointer: &str) -> Evidence {
    if let Err(text) = claim::valid_pointer(pointer) {
        return evidence(pointer, State::Unreadable, text);
    }
    let rest = pointer.split_once(':').map(|(_, rest)| rest).unwrap_or("");
    let (rel, selector) = match rest.split_once('#') {
        Some((rel, selector)) => (rel, selector),
        None => (rest, ""),
    };
    let rel = rel.trim_end_matches(['/', '\\']);
    if selector == "ledger" {
        return ledger(project, pointer, rel);
    }
    let out_dir = match resolve(project, rel) {
        Ok(path) => path,
        Err(ReadError::Missing(text)) => return evidence(pointer, State::Missing, text),
        Err(ReadError::Unreadable(text)) => return evidence(pointer, State::Unreadable, text),
    };
    if !out_dir.is_dir() {
        return file_instead_of_folder(project, pointer, rel);
    }
    if selector.is_empty() || selector == "survivors" {
        return whole_run(project, pointer, rel, &out_dir);
    }
    if let Some(id) = selector.strip_prefix("mutant:") {
        return one_mutant(project, pointer, rel, id);
    }
    evidence(
        pointer,
        State::Unreadable,
        format!("unknown probatio selector \"{}\"", fsx::clip(selector, 60)),
    )
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

fn resolve(project: &Path, rel: &str) -> Result<PathBuf, ReadError> {
    match fsx::project_file(project, rel) {
        Ok(path) => Ok(path),
        Err(text) if text.ends_with("does not exist") => Err(ReadError::Missing(text)),
        Err(text) => Err(ReadError::Unreadable(text)),
    }
}

fn file_instead_of_folder(project: &Path, pointer: &str, rel: &str) -> Evidence {
    match read::read_json(project, rel, "probatio") {
        Err(ReadError::Unreadable(text)) if text.contains("SQLite") => {
            evidence(pointer, State::Unreadable, text)
        }
        _ => evidence(
            pointer,
            State::Unreadable,
            "probatio pointers name the run's output folder",
        ),
    }
}

fn whole_run(project: &Path, pointer: &str, rel: &str, out_dir: &Path) -> Evidence {
    let loaded = match crate::cache::probatio(project, rel, || {
        load_results(project, rel, out_dir).map(|rows| {
            rows.map(|items| {
                items
                    .into_iter()
                    .map(|item| (item.id, item.outcome))
                    .collect()
            })
        })
    }) {
        Ok(rows) => rows.map(|arc| {
            arc.iter()
                .map(|(id, outcome)| Mutant {
                    id: id.clone(),
                    outcome: outcome.clone(),
                })
                .collect::<Vec<_>>()
        }),
        Err(ReadError::Unreadable(text)) => return evidence(pointer, State::Unreadable, text),
        Err(ReadError::Missing(text)) => return evidence(pointer, State::Missing, text),
    };
    let tally = tally_note(
        project,
        rel,
        loaded.as_ref().map(|rows| survived(rows)).unwrap_or(0),
    );
    let Some(rows) = loaded else {
        return if tally.saw_file {
            evidence(
                pointer,
                State::Inconclusive,
                format!("results were missing; tally.json is not used as proof. {tally}"),
            )
        } else {
            evidence(pointer, State::Missing, "no mutant results".to_string())
        };
    };
    if rows.is_empty() {
        return if tally.saw_file {
            evidence(
                pointer,
                State::Inconclusive,
                format!("results were missing; tally.json is not used as proof. {tally}"),
            )
        } else {
            evidence(pointer, State::Missing, "no mutant results".to_string())
        };
    }
    let killed = count(&rows, "killed");
    let survivors = survived(&rows);
    let mut detail = describe(&rows);
    if !tally.text.is_empty() {
        detail.push(' ');
        detail.push_str(&tally.text);
    }
    if survivors > 0 {
        return evidence(pointer, State::Fail, detail);
    }
    if let Some(odd) = rows
        .iter()
        .find(|row| !KNOWN_OUTCOMES.contains(&row.outcome.as_str()))
    {
        return evidence(
            pointer,
            State::Unreadable,
            format!(
                "{detail}; outcome \"{}\" is not one Probatio writes, so the run is not proof",
                fsx::clip(&odd.outcome, 40)
            ),
        );
    }
    let blocking: usize = BLOCKING_OUTCOMES.iter().map(|o| count(&rows, o)).sum();
    if blocking > 0 {
        return evidence(
            pointer,
            State::Inconclusive,
            format!("{detail}; a flaky or error result did not settle its mutant"),
        );
    }
    match planned(project, rel, &rows) {
        Plan::Waiting { done, total } => {
            return evidence(
                pointer,
                State::Inconclusive,
                format!("{detail}; {done} of {total} mutants in the run have a result, so the run may be unfinished or capped"),
            )
        }
        Plan::Unreadable(why) => {
            return evidence(
                pointer,
                State::Inconclusive,
                format!("{detail}; cannot tell whether the run finished: {why}"),
            )
        }
        Plan::Complete | Plan::NotChecked => {}
    }
    if killed > 0 {
        evidence(pointer, State::Pass, detail)
    } else {
        evidence(pointer, State::Inconclusive, detail)
    }
}

enum Plan {
    /// Every mutant in the run's list has a result.
    Complete,
    Waiting {
        done: usize,
        total: usize,
    },
    /// `run.json` is there but cannot be read.
    Unreadable(String),
    /// No `run.json`, or its mutant list is outside the project.
    NotChecked,
}

/// Compare the results with the mutant list `run.json` names, the same check
/// Probatio's tally makes. Only a list inside the project is read.
fn planned(project: &Path, rel: &str, rows: &[Mutant]) -> Plan {
    let run_rel = format!("{rel}/run.json");
    let run = match read::read_json(project, &run_rel, "probatio") {
        Ok(value) => value,
        Err(ReadError::Missing(_)) => return Plan::NotChecked,
        Err(ReadError::Unreadable(text)) if text.ends_with("does not exist") => {
            return Plan::NotChecked
        }
        Err(ReadError::Unreadable(text)) => return Plan::Unreadable(text),
    };
    let patches = match run.get("patches") {
        Some(Value::Array(items)) => items.first().and_then(Value::as_str),
        Some(Value::String(path)) => Some(path.as_str()),
        _ => None,
    };
    let Some(patches) = patches else {
        return Plan::NotChecked;
    };
    let Some(list_rel) = inside_project(project, patches) else {
        return Plan::NotChecked;
    };
    let list_rel = match fsx::project_file(project, &list_rel) {
        Ok(path) if path.is_dir() => format!("{list_rel}/mutants.json"),
        Ok(_) => list_rel,
        Err(_) => return Plan::NotChecked,
    };
    let list = match read::read_json(project, &list_rel, "probatio") {
        Ok(value) => value,
        Err(ReadError::Missing(_)) => return Plan::NotChecked,
        Err(ReadError::Unreadable(text)) if text.ends_with("does not exist") => {
            return Plan::NotChecked
        }
        Err(ReadError::Unreadable(text)) => return Plan::Unreadable(text),
    };
    let items = match &list {
        Value::Array(items) => items,
        Value::Object(_) => match list.get("mutants").and_then(Value::as_array) {
            Some(items) => items,
            None => return Plan::Unreadable(format!("{list_rel} has no mutants list")),
        },
        _ => return Plan::Unreadable(format!("{list_rel} is not a mutant list")),
    };
    let mut total = 0;
    let mut done = 0;
    for item in items {
        let Some(id) = item.get("id").and_then(Value::as_str) else {
            return Plan::Unreadable(format!("{list_rel} has a mutant with no id"));
        };
        total += 1;
        if rows.iter().any(|row| row.id == id) {
            done += 1;
        }
    }
    if done < total {
        Plan::Waiting { done, total }
    } else {
        Plan::Complete
    }
}

/// A path from `run.json` as a project-relative path, when it is inside the
/// project. Probatio writes absolute paths; a relative one is taken from the
/// project folder.
fn inside_project(project: &Path, path: &str) -> Option<String> {
    let p = Path::new(path);
    if !p.is_absolute() {
        return Some(path.trim_start_matches("./").to_string());
    }
    let root = project.canonicalize().ok()?;
    let canon = p.canonicalize().ok()?;
    let rel = canon.strip_prefix(&root).ok()?;
    let text = rel.to_str()?.replace('\\', "/");
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn one_mutant(project: &Path, pointer: &str, rel: &str, id: &str) -> Evidence {
    if !safe_id(id) {
        return evidence(
            pointer,
            State::Unreadable,
            format!(
                "mutant id \"{}\" is not a single file name",
                fsx::clip(id, 60)
            ),
        );
    }
    let rel_file = format!("{rel}/results/{id}.json");
    let value = match read::read_json(project, &rel_file, "probatio") {
        Ok(value) => value,
        Err(ReadError::Missing(text)) => return evidence(pointer, State::Missing, text),
        Err(ReadError::Unreadable(text)) => return evidence(pointer, State::Unreadable, text),
    };
    let outcome = match outcome_of(&value) {
        Ok(outcome) => outcome,
        Err(text) => return evidence(pointer, State::Unreadable, format!("{rel_file} {text}")),
    };
    match outcome.as_str() {
        "killed" => evidence(
            pointer,
            State::Pass,
            format!("mutant {id} outcome is killed"),
        ),
        "survived" => evidence(
            pointer,
            State::Fail,
            format!("mutant {id} outcome is survived"),
        ),
        other if KNOWN_OUTCOMES.contains(&other) => evidence(
            pointer,
            State::Inconclusive,
            format!("mutant {id} outcome \"{other}\" is not a kill and not a survivor"),
        ),
        other => evidence(
            pointer,
            State::Unreadable,
            format!(
                "mutant {id} outcome \"{}\" is not one Probatio writes",
                fsx::clip(other, 40)
            ),
        ),
    }
}

fn ledger(project: &Path, pointer: &str, rel: &str) -> Evidence {
    let rel_file = if rel.ends_with("ledger.json") {
        rel.to_string()
    } else {
        format!("{rel}/ledger.json")
    };
    let value = match read::read_json(project, &rel_file, "probatio") {
        Ok(value) => value,
        Err(ReadError::Missing(_)) => {
            return evidence(
                pointer,
                State::Missing,
                format!("{rel_file} does not exist"),
            )
        }
        Err(ReadError::Unreadable(text)) => return evidence(pointer, State::Unreadable, text),
    };
    let Some(entries) = value.get("entries").and_then(Value::as_array) else {
        return evidence(
            pointer,
            State::Unreadable,
            format!("{rel_file} has no entries array"),
        );
    };
    let mut counts: Vec<(String, usize)> = Vec::new();
    for entry in entries {
        let status = entry
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unlabelled")
            .to_string();
        if let Some(slot) = counts.iter_mut().find(|(name, _)| *name == status) {
            slot.1 += 1;
        } else {
            counts.push((status, 1));
        }
    }
    counts.sort_by(|a, b| a.0.cmp(&b.0));
    let breakdown = if counts.is_empty() {
        String::new()
    } else {
        format!(
            " ({})",
            counts
                .iter()
                .map(|(name, n)| format!("{n} {name}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    evidence(
        pointer,
        State::Inconclusive,
        format!(
            "{} ledger entries{breakdown}; an unchecked ledger is not proof",
            entries.len()
        ),
    )
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id != "."
        && id != ".."
        && !id.contains('/')
        && !id.contains('\\')
        && !id.contains('\0')
        && !id.contains("..")
}

/// `None` when `results/` is absent. An empty folder is `Some` of an empty list.
fn load_results(
    project: &Path,
    rel: &str,
    out_dir: &Path,
) -> Result<Option<Vec<Mutant>>, ReadError> {
    let results = out_dir.join("results");
    match fs::symlink_metadata(&results) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ReadError::Unreadable(format!(
                "cannot stat {rel}/results: {error}"
            )))
        }
        Ok(meta) if meta.file_type().is_symlink() || meta.is_dir() => {
            let root = project.canonicalize().map_err(|error| {
                ReadError::Unreadable(format!("cannot resolve the project folder: {error}"))
            })?;
            let canon = results.canonicalize().map_err(|error| {
                ReadError::Unreadable(format!("cannot resolve {rel}/results: {error}"))
            })?;
            if !canon.starts_with(&root) {
                return Err(ReadError::Unreadable(format!(
                    "{rel}/results resolves outside the project"
                )));
            }
            if !canon.is_dir() {
                return Err(ReadError::Unreadable(format!(
                    "{rel}/results is not a folder"
                )));
            }
        }
        Ok(_) => {
            return Err(ReadError::Unreadable(format!(
                "{rel}/results is not a folder"
            )))
        }
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&results)
        .map_err(|error| ReadError::Unreadable(format!("cannot read {rel}/results: {error}")))?
    {
        let entry = entry.map_err(|error| {
            ReadError::Unreadable(format!("cannot read {rel}/results: {error}"))
        })?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            return Err(ReadError::Unreadable(
                "a result file name is not UTF-8".into(),
            ));
        };
        if name.ends_with(".json") {
            names.push(name);
        }
    }
    names.sort();
    let mut rows = Vec::new();
    for name in names {
        let rel_file = format!("{rel}/results/{name}");
        let path = results.join(&name);
        let meta = fs::symlink_metadata(&path)
            .map_err(|error| ReadError::Unreadable(format!("cannot stat {rel_file}: {error}")))?;
        // A symlink can leave the project, so it keeps the full path check.
        // A regular file under the results directory, which is already inside
        // the project, cannot: its name is one path segment.
        let value = if meta.file_type().is_symlink() {
            read::read_json(project, &rel_file, "probatio")?
        } else if meta.is_file() {
            read::read_json_file(&path, &rel_file, "probatio")?
        } else {
            return Err(ReadError::Unreadable(format!(
                "{rel_file} {}",
                crate::fsx::not_a_file(&meta)
            )));
        };
        let outcome = outcome_of(&value)
            .map_err(|text| ReadError::Unreadable(format!("{rel_file} {text}")))?;
        let id = name.trim_end_matches(".json").to_string();
        rows.push(Mutant { id, outcome });
    }
    Ok(Some(rows))
}

fn outcome_of(value: &Value) -> Result<String, String> {
    let Some(outcome) = value.get("outcome").and_then(Value::as_str) else {
        return Err("has no outcome".into());
    };
    if outcome == "killed" && value.get("cause").and_then(Value::as_str) == Some("build") {
        Ok("unviable".into())
    } else {
        Ok(outcome.to_string())
    }
}

fn count(rows: &[Mutant], outcome: &str) -> usize {
    rows.iter().filter(|row| row.outcome == outcome).count()
}

fn survived(rows: &[Mutant]) -> usize {
    count(rows, "survived")
}

fn describe(rows: &[Mutant]) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for row in rows {
        if let Some(slot) = counts.iter_mut().find(|(name, _)| *name == row.outcome) {
            slot.1 += 1;
        } else {
            counts.push((row.outcome.clone(), 1));
        }
    }
    counts.sort_by(|a, b| a.0.cmp(&b.0));
    counts
        .iter()
        .map(|(name, n)| format!("{n} {name}"))
        .collect::<Vec<_>>()
        .join(", ")
}

struct TallyNote {
    text: String,
    saw_file: bool,
}

impl std::fmt::Display for TallyNote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

fn tally_note(project: &Path, rel: &str, result_survivors: usize) -> TallyNote {
    let rel_file = format!("{rel}/tally.json");
    let doc = match read::read_json(project, &rel_file, "probatio") {
        Err(ReadError::Missing(_)) => {
            return TallyNote {
                text: String::new(),
                saw_file: false,
            }
        }
        Err(ReadError::Unreadable(text)) if text.ends_with("does not exist") => {
            return TallyNote {
                text: String::new(),
                saw_file: false,
            }
        }
        Err(ReadError::Unreadable(text)) => {
            return TallyNote {
                text: format!("tally.json ignored ({text}); trusted results/"),
                saw_file: true,
            }
        }
        Ok(doc) => doc,
    };
    let Some(tally_survivors) = tally_survivors(&doc) else {
        return TallyNote {
            text: String::new(),
            saw_file: true,
        };
    };
    if tally_survivors == result_survivors {
        TallyNote {
            text: String::new(),
            saw_file: true,
        }
    } else {
        TallyNote {
            text: format!(
                "tally.json disagrees (tally survived {tally_survivors}, results survived {result_survivors}); trusted results/"
            ),
            saw_file: true,
        }
    }
}

fn tally_survivors(doc: &Value) -> Option<usize> {
    if let (Some(gaps), Some(rest)) = (
        doc.get("gaps").and_then(Value::as_array),
        doc.get("rest").and_then(Value::as_i64),
    ) {
        if rest >= 0 {
            return Some(gaps.len() + rest as usize);
        }
    }
    if let Some(n) = doc.get("survived").and_then(Value::as_i64) {
        if n >= 0 {
            return Some(n as usize);
        }
    }
    doc.get("summary")
        .and_then(Value::as_str)
        .and_then(survived_in_summary)
}

fn survived_in_summary(text: &str) -> Option<usize> {
    let mut number = None;
    for token in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        if token.is_empty() {
            continue;
        }
        if token == "survived" {
            return number;
        }
        number = token.parse().ok();
    }
    None
}
