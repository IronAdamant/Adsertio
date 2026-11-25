//! The Trammel adapter. It reads `.trammel/store` (and the journal beside it)
//! and answers one selector. It never runs Trammel and never writes.
//!
//! A step passes only when `verification.passed` is true. A missing
//! verification is not a pass and not a fail: the step has not been checked.
//! A plan passes only when it has at least one step and every step passes.
//! Any failed step refutes the plan. A constraint passes when it exists and
//! `active` is not false (`active` defaults to true, as Trammel does when it
//! lists constraints). A failure selector passes when that row exists; it
//! does not fail when the row is absent.

use crate::claim;
use crate::json::Value;
use crate::read::{self, ReadError};
use crate::verdict::{Evidence, State};
use std::path::Path;

enum Sel {
    Step { plan: i64, step: i64 },
    Plan(i64),
    Constraint(i64),
    Failure(i64),
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
    let doc = match read::read_trammel(project, rel) {
        Ok(snapshot) => snapshot.doc,
        Err(ReadError::Missing(text)) => return evidence(pointer, State::Missing, text),
        Err(ReadError::Unreadable(text)) => return evidence(pointer, State::Unreadable, text),
    };
    if selector.is_empty() {
        return evidence(
            pointer,
            State::Unreadable,
            "a selector is required: plan-<id>/step-<id>, plan-<id>, constraint-<id>, or failure-<id>",
        );
    }
    let sel = match parse_selector(selector) {
        Ok(sel) => sel,
        Err(text) => return evidence(pointer, State::Unreadable, text),
    };
    let (at, commits) = freshness(&doc, &sel);
    match judge(&doc, sel) {
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

fn parse_selector(selector: &str) -> Result<Sel, String> {
    if let Some(rest) = selector.strip_prefix("plan-") {
        if let Some((plan, step)) = rest.split_once("/step-") {
            if !plan.is_empty() && !plan.contains('/') {
                return Ok(Sel::Step {
                    plan: parse_id(plan)?,
                    step: parse_id(step)?,
                });
            }
        } else if !rest.is_empty() && !rest.contains('/') {
            return Ok(Sel::Plan(parse_id(rest)?));
        }
    } else if let Some(rest) = selector.strip_prefix("constraint-") {
        return Ok(Sel::Constraint(parse_id(rest)?));
    } else if let Some(rest) = selector.strip_prefix("failure-") {
        return Ok(Sel::Failure(parse_id(rest)?));
    }
    Err(format!(
        "unknown trammel selector \"{}\"",
        crate::fsx::clip(selector, 60)
    ))
}

fn parse_id(text: &str) -> Result<i64, String> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("\"{text}\" is not an id"));
    }
    text.parse::<i64>()
        .map_err(|_| format!("\"{text}\" is not an id"))
}

fn judge(doc: &Value, sel: Sel) -> Result<(State, String), String> {
    match sel {
        Sel::Step { plan, step } => step_state(doc, plan, step),
        Sel::Plan(plan) => plan_state(doc, plan),
        Sel::Constraint(id) => constraint_state(doc, id),
        Sel::Failure(id) => failure_state(doc, id),
    }
}

fn rows<'a>(doc: &'a Value, name: &str) -> Result<Vec<&'a Value>, String> {
    let Some(items) = doc.get(name).and_then(Value::as_array) else {
        return Err(format!(
            "missing {name}, or it is not an array. A damaged store is not treated as empty"
        ));
    };
    for (index, item) in items.iter().enumerate() {
        if !matches!(item, Value::Object(_)) {
            return Err(format!("{name}[{index}] is not an object"));
        }
    }
    Ok(items.iter().collect())
}

fn id_is(row: &Value, field: &str, id: i64) -> bool {
    row.get(field).and_then(Value::as_i64) == Some(id)
}

fn step_state(doc: &Value, plan: i64, step: i64) -> Result<(State, String), String> {
    let found = rows(doc, "steps")?
        .into_iter()
        .find(|row| id_is(row, "id", step) && id_is(row, "plan_id", plan));
    let Some(row) = found else {
        return Ok((
            State::Missing,
            format!("plan {plan} step {step} is not in the store"),
        ));
    };
    Ok(verification(row, &format!("plan {plan} step {step}")))
}

fn verification(row: &Value, what: &str) -> (State, String) {
    match row.get("verification") {
        None => (State::Missing, format!("{what} is not yet verified")),
        Some(Value::Object(_)) => match row
            .get("verification")
            .and_then(|v| v.get("passed"))
            .and_then(Value::as_bool)
        {
            Some(true) => (State::Pass, format!("{what} verification.passed is true")),
            _ => (
                State::Fail,
                format!("{what} verification.passed is not true"),
            ),
        },
        Some(_) => (
            State::Unreadable,
            format!("{what} verification is not an object"),
        ),
    }
}

fn plan_state(doc: &Value, plan: i64) -> Result<(State, String), String> {
    let steps: Vec<&Value> = rows(doc, "steps")?
        .into_iter()
        .filter(|row| id_is(row, "plan_id", plan))
        .collect();
    if steps.is_empty() {
        let known = rows(doc, "plans")?.iter().any(|row| id_is(row, "id", plan));
        let detail = if known {
            format!("plan {plan} has no steps")
        } else {
            format!("plan {plan} is not in the store")
        };
        return Ok((State::Missing, detail));
    }
    let mut failed = 0usize;
    let mut waiting = 0usize;
    for row in &steps {
        match row.get("verification") {
            None => waiting += 1,
            Some(Value::Object(_)) => {
                if row
                    .get("verification")
                    .and_then(|v| v.get("passed"))
                    .and_then(Value::as_bool)
                    != Some(true)
                {
                    failed += 1;
                }
            }
            Some(_) => {
                return Ok((
                    State::Unreadable,
                    format!("plan {plan} has a verification that is not an object"),
                ))
            }
        }
    }
    if failed > 0 {
        Ok((
            State::Fail,
            format!("plan {plan} has {failed} step(s) whose verification.passed is not true"),
        ))
    } else if waiting > 0 {
        Ok((
            State::Missing,
            format!("plan {plan} has {waiting} step(s) not yet verified"),
        ))
    } else {
        Ok((
            State::Pass,
            format!(
                "plan {plan} has {} step(s) and every verification.passed is true",
                steps.len()
            ),
        ))
    }
}

fn constraint_state(doc: &Value, id: i64) -> Result<(State, String), String> {
    let Some(row) = rows(doc, "constraints")?
        .into_iter()
        .find(|row| id_is(row, "id", id))
    else {
        return Ok((
            State::Missing,
            format!("constraint {id} is not in the store"),
        ));
    };
    match row.get("active") {
        None | Some(Value::Bool(true)) => Ok((State::Pass, format!("constraint {id} is active"))),
        Some(Value::Bool(false)) => Ok((State::Fail, format!("constraint {id} is inactive"))),
        Some(_) => Err(format!("constraint {id} active is not a boolean")),
    }
}

fn freshness(doc: &Value, sel: &Sel) -> (Option<f64>, Vec<String>) {
    match sel {
        Sel::Step { plan, step } => {
            let Ok(steps) = rows(doc, "steps") else {
                return (None, Vec::new());
            };
            let Some(row) = steps
                .into_iter()
                .find(|row| id_is(row, "id", *step) && id_is(row, "plan_id", *plan))
            else {
                return (None, Vec::new());
            };
            stamp(row.get("verification"))
        }
        Sel::Plan(plan) => {
            let Ok(steps) = rows(doc, "steps") else {
                return (None, Vec::new());
            };
            let mut at = None;
            let mut commits = Vec::new();
            for row in steps.into_iter().filter(|row| id_is(row, "plan_id", *plan)) {
                let (step_at, step_commits) = stamp(row.get("verification"));
                at = older(at, step_at);
                commits.extend(step_commits);
            }
            (at, commits)
        }
        Sel::Constraint(_) | Sel::Failure(_) => (None, Vec::new()),
    }
}

fn stamp(value: Option<&Value>) -> (Option<f64>, Vec<String>) {
    let Some(value) = value else {
        return (None, Vec::new());
    };
    let commits = commit_text(value.get("commit"))
        .map(|text| vec![text])
        .unwrap_or_default();
    (value.get("at").and_then(Value::as_f64), commits)
}

fn older(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, b) => b,
    }
}

fn commit_text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn failure_state(doc: &Value, id: i64) -> Result<(State, String), String> {
    if rows(doc, "failures")?
        .into_iter()
        .any(|row| id_is(row, "id", id))
    {
        Ok((State::Pass, format!("failure {id} is recorded")))
    } else {
        Ok((State::Missing, format!("failure {id} is not in the store")))
    }
}
