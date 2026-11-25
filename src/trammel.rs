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
    let index = match crate::cache::trammel(project, rel, || {
        let loaded = read::load_trammel_text(project, rel)?;
        crate::trammel_idx::parse(&loaded.text, rel)
    }) {
        Ok(index) => index,
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
        trees: Vec::new(),
        malformed: false,
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

fn judge(
    index: &crate::trammel_idx::Index,
    sel: Sel,
) -> Result<(State, String, crate::trammel_idx::Stamp), String> {
    let bare = crate::trammel_idx::Stamp {
        at: None,
        commits: Vec::new(),
        malformed: false,
    };
    match sel {
        Sel::Step { plan, step } => {
            let hit = index.step(plan, step)?;
            let what = format!("plan {plan} step {step}");
            let (state, detail) = match hit.kind {
                crate::trammel_idx::StepKind::NotInStore => (
                    State::Missing,
                    format!("plan {plan} step {step} is not in the store"),
                ),
                crate::trammel_idx::StepKind::NotVerified => {
                    (State::Missing, format!("{what} is not yet verified"))
                }
                crate::trammel_idx::StepKind::Bad => (
                    State::Unreadable,
                    format!("{what} verification is not an object"),
                ),
                crate::trammel_idx::StepKind::Fail => (
                    State::Fail,
                    format!("{what} verification.passed is not true"),
                ),
                crate::trammel_idx::StepKind::Pass => {
                    (State::Pass, format!("{what} verification.passed is true"))
                }
            };
            Ok((state, detail, hit.stamp))
        }
        Sel::Plan(plan) => {
            let hit = index.plan(plan)?;
            let (state, detail) = match hit.kind {
                crate::trammel_idx::PlanKind::Unknown => {
                    (State::Missing, format!("plan {plan} is not in the store"))
                }
                crate::trammel_idx::PlanKind::NoSteps => {
                    (State::Missing, format!("plan {plan} has no steps"))
                }
                crate::trammel_idx::PlanKind::Bad => (
                    State::Unreadable,
                    format!("plan {plan} has a verification that is not an object"),
                ),
                crate::trammel_idx::PlanKind::Fail { failed } => (
                    State::Fail,
                    format!(
                        "plan {plan} has {failed} step(s) whose verification.passed is not true"
                    ),
                ),
                crate::trammel_idx::PlanKind::Waiting { waiting } => (
                    State::Missing,
                    format!("plan {plan} has {waiting} step(s) not yet verified"),
                ),
                crate::trammel_idx::PlanKind::Pass { steps } => (
                    State::Pass,
                    format!(
                        "plan {plan} has {steps} step(s) and every verification.passed is true"
                    ),
                ),
            };
            Ok((state, detail, hit.stamp))
        }
        Sel::Constraint(id) => match index.constraint(id)? {
            crate::trammel_idx::ConstraintKind::Missing => Ok((
                State::Missing,
                format!("constraint {id} is not in the store"),
                bare,
            )),
            crate::trammel_idx::ConstraintKind::Active => {
                Ok((State::Pass, format!("constraint {id} is active"), bare))
            }
            crate::trammel_idx::ConstraintKind::Inactive => {
                Ok((State::Fail, format!("constraint {id} is inactive"), bare))
            }
            crate::trammel_idx::ConstraintKind::Bad => {
                Err(format!("constraint {id} active is not a boolean"))
            }
        },
        Sel::Failure(id) => {
            if index.failure(id)? {
                Ok((State::Pass, format!("failure {id} is recorded"), bare))
            } else {
                Ok((
                    State::Missing,
                    format!("failure {id} is not in the store"),
                    bare,
                ))
            }
        }
    }
}
