//! The Probatio adapter. It reads a run's output folder. `results/*.json` is
//! the source of truth. `tally.json` is a cache: when the two disagree, the
//! results stand and the detail says so. It never runs Probatio and never
//! writes.
//!
//! A whole run fails only when some mutant survived. It passes only when at
//! least one mutant was killed, none survived, and the run is shown to be
//! complete. `timeout` and `unviable` (including `killed` with `cause`
//! `build`) do not survive and do not kill. On their own, with no kill and no
//! survivor, the run is inconclusive. `flaky`, `error`, and `no coverage`
//! block a pass: the run is inconclusive. An outcome Probatio does not write
//! makes the run unreadable.
//!
//! Complete means every mutant in every mutant list `run.json` names has a
//! result. A run still going, or a capped run, is inconclusive. So is a run
//! whose list cannot be checked: no `run.json`, no patches, a list outside
//! the project or gone. A `mutate diff` run is always inconclusive: Probatio
//! does not write the full list a diff plans, so a call capped by
//! `--max-mutants` cannot be told from a whole one. One broken results file
//! makes the run unreadable; it is not dropped. `#ledger` counts entries and
//! is never a pass: an unchecked ledger is not proof.

use crate::claim;
use crate::fsx;
use crate::json::Value;
use crate::read::{self, ReadError};
use crate::selector::{self, Probatio as Sel};
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
    let (_, rel, selector) = selector::split(pointer);
    let rel = rel.trim_end_matches(['/', '\\']);
    let sel = selector::probatio(selector.unwrap_or(""));
    if sel == Ok(Sel::Ledger) {
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
    match sel {
        Ok(Sel::Run) => whole_run(project, pointer, rel, &out_dir),
        Ok(Sel::Ledger) => ledger(project, pointer, rel),
        Ok(Sel::Mutant(id)) => one_mutant(project, pointer, rel, &id),
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
        trees: Vec::new(),
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
    let uncovered = count(&rows, "no coverage");
    if uncovered > 0 {
        return evidence(
            pointer,
            State::Inconclusive,
            format!(
                "{detail}; {uncovered} mutant(s) had no coverage: no test ran that line, and Probatio's tally says that is not a pass"
            ),
        );
    }
    match planned(project, rel, out_dir, &rows) {
        Plan::Waiting { done, total } => {
            return evidence(
                pointer,
                State::Inconclusive,
                format!("{detail}; {done} of {total} mutants in the run have a result, so the run may be unfinished or capped"),
            )
        }
        Plan::Unknown(why) => {
            return evidence(
                pointer,
                State::Inconclusive,
                format!("{detail}; cannot tell whether the run finished: {why}"),
            )
        }
        Plan::Complete => {}
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
    /// The run's full mutant list cannot be found or read, so the run cannot
    /// be shown to be complete.
    Unknown(String),
}

/// Compare the results with every mutant list `run.json` names, the same
/// check Probatio's tally makes. A list that cannot be found, read, or is
/// outside the project, means the run cannot be shown to be complete.
fn planned(project: &Path, rel: &str, out_dir: &Path, rows: &[Mutant]) -> Plan {
    if let Some(why) = diff_run(out_dir) {
        return Plan::Unknown(why);
    }
    let run_rel = format!("{rel}/run.json");
    let run = match read::read_json(project, &run_rel, "probatio") {
        Ok(value) => value,
        Err(ReadError::Missing(_)) => {
            return Plan::Unknown(format!(
                "{run_rel} is missing, so the run's mutant list is unknown"
            ))
        }
        Err(ReadError::Unreadable(text)) if text.ends_with("does not exist") => {
            return Plan::Unknown(format!(
                "{run_rel} is missing, so the run's mutant list is unknown"
            ))
        }
        Err(ReadError::Unreadable(text)) => return Plan::Unknown(text),
    };
    let entries: Vec<&str> = match run.get("patches") {
        Some(Value::Array(items)) => {
            let mut out = Vec::new();
            for item in items {
                match item.as_str() {
                    Some(text) if !text.is_empty() => out.push(text),
                    _ => {
                        return Plan::Unknown(format!(
                            "{run_rel} has a patches entry that is not a path"
                        ))
                    }
                }
            }
            out
        }
        Some(Value::String(path)) if !path.is_empty() => vec![path.as_str()],
        _ => return Plan::Unknown(format!("{run_rel} names no patches")),
    };
    if entries.is_empty() {
        return Plan::Unknown(format!("{run_rel} names no patches"));
    }
    let mut ids: Vec<String> = Vec::new();
    for entry in entries {
        match mutant_ids(project, entry) {
            Ok(found) => {
                for id in found {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
            }
            Err(why) => return Plan::Unknown(why),
        }
    }
    if ids.is_empty() {
        return Plan::Unknown(format!("the patches {run_rel} names list no mutants"));
    }
    let total = ids.len();
    let done = ids
        .iter()
        .filter(|id| rows.iter().any(|row| &row.id == *id))
        .count();
    if done < total {
        Plan::Waiting { done, total }
    } else {
        Plan::Complete
    }
}

/// `mutate diff` writes `<out>/plan.json`, `<out>/generate`, and the run in
/// `<out>/run`. Its `plan.json` holds only a hash of the plan, and
/// `generate/mutants.json` holds only the mutants sent to the run so far, so
/// a call capped by `--max-mutants` (or stopped by its time budget before the
/// batch was written) looks complete. No file names the full set.
fn diff_run(out_dir: &Path) -> Option<String> {
    let parent = out_dir.parent()?;
    if parent.join("plan.json").is_file() && parent.join("generate").is_dir() {
        Some(
            "this is a mutate diff run; Probatio does not write the full list of mutants a diff plans (plan.json holds only a hash, and generate/mutants.json only the batch sent to the run), so a capped call cannot be told from a whole one. Point at #mutant:<id>, or score with mutate generate and mutate run".into(),
        )
    } else {
        None
    }
}

/// The mutant ids one `patches` entry holds, read the way Probatio's
/// `load_mutants` reads them: a file is a mutant list; a folder is its
/// `mutants.json`, or, without one, every `*.json` file in it. A relative
/// entry is taken from the project folder, where Probatio was run.
fn mutant_ids(project: &Path, entry: &str) -> Result<Vec<String>, String> {
    let Some(list_rel) = inside_project(project, entry) else {
        return Err(format!(
            "patches \"{}\" is outside the project, or does not exist",
            fsx::clip(entry, 80)
        ));
    };
    let path = match fsx::project_file(project, &list_rel) {
        Ok(path) => path,
        Err(text) => return Err(format!("patches {text}")),
    };
    let files: Vec<String> = if path.is_file() {
        vec![list_rel]
    } else if path.is_dir() {
        let combined = format!("{list_rel}/mutants.json");
        if path.join("mutants.json").is_file() {
            vec![combined]
        } else {
            let mut names = Vec::new();
            let read =
                fs::read_dir(&path).map_err(|error| format!("cannot read {list_rel}: {error}"))?;
            for item in read {
                let item = item.map_err(|error| format!("cannot read {list_rel}: {error}"))?;
                let Some(name) = item.file_name().to_str().map(str::to_string) else {
                    return Err(format!("a file name in {list_rel} is not UTF-8"));
                };
                if Path::new(&name).extension().and_then(|ext| ext.to_str()) == Some("json") {
                    names.push(name);
                }
            }
            names.sort();
            names
                .into_iter()
                .map(|name| format!("{list_rel}/{name}"))
                .collect()
        }
    } else {
        return Err(format!("patches {list_rel} is not a file or folder"));
    };
    let mut ids = Vec::new();
    for file in files {
        let list = read::read_json(project, &file, "probatio")
            .map_err(|error| error.text().to_string())?;
        let items = match &list {
            Value::Array(items) => items,
            Value::Object(_) => match list.get("mutants").and_then(Value::as_array) {
                Some(items) => items,
                None => return Err(format!("{file} has no mutants list")),
            },
            _ => return Err(format!("{file} is not a mutant list")),
        };
        for item in items {
            let Some(id) = item.get("id").and_then(Value::as_str) else {
                return Err(format!("{file} has a mutant with no id"));
            };
            ids.push(id.to_string());
        }
    }
    Ok(ids)
}

/// A path from `run.json` as a project-relative path, when it is inside the
/// project. Probatio reads a relative path from the folder it ran in; that is
/// taken to be the project folder.
fn inside_project(project: &Path, path: &str) -> Option<String> {
    let p = Path::new(path);
    if !p.is_absolute() {
        let text = path.replace('\\', "/");
        let text = text.trim_start_matches("./").trim_end_matches('/');
        return if text.is_empty() || text == "." {
            None
        } else {
            Some(text.to_string())
        };
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

/// `id` is a single file name: `selector::probatio` checked it.
fn one_mutant(project: &Path, pointer: &str, rel: &str, id: &str) -> Evidence {
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
