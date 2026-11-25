//! A pass is not proof when it is older than the claim, or when it names a
//! commit that is not the project's HEAD.
//!
//! Only a pass is downgraded. A failure stays a failure: old evidence that
//! the claim does not hold still refutes it. Evidence with no timestamp and
//! no commit can still pass, and the detail says `no timestamp`.
//!
//! `git` is the only external program, and only `git rev-parse HEAD` is run.
//! The evidence's commit is compared here; it is never passed to the shell.

use crate::verdict::{Evidence, State};
use std::path::Path;
use std::process::{Command, Stdio};

pub fn apply(project: &Path, created: &str, evidence: &mut Evidence) {
    if evidence.state != State::Pass {
        return;
    }
    if evidence.malformed {
        evidence.state = State::Stale;
        evidence.detail = format!("stale: malformed timestamp or commit; {}", evidence.detail);
        return;
    }
    if evidence.at.is_none() && evidence.commits.is_empty() {
        note_no_time(evidence);
        return;
    }
    if let Some(at) = evidence.at {
        match created_unix(created) {
            Some(created_at) if at + 1.0 < created_at as f64 => {
                evidence.state = State::Stale;
                evidence.detail = format!(
                    "stale: evidence is older than the claim; {}",
                    evidence.detail
                );
                return;
            }
            Some(_) => {}
            None => {
                evidence.state = State::Stale;
                evidence.detail = format!(
                    "stale: could not read the claim created time; {}",
                    evidence.detail
                );
                return;
            }
        }
    }
    if evidence.commits.is_empty() {
        return;
    }
    match head_commit(project) {
        Ok(head) => {
            if let Some(commit) = evidence
                .commits
                .iter()
                .find(|commit| !same_commit(&head, commit))
            {
                evidence.state = State::Stale;
                evidence.detail = format!(
                    "stale: evidence commit {commit} is not HEAD {head}; {}{}",
                    evidence.detail,
                    rerun_hint(&evidence.pointer)
                );
                return;
            }
        }
        Err(error) => {
            evidence.state = State::Stale;
            evidence.detail = format!(
                "stale: could not check the evidence commit ({error}); {}",
                evidence.detail
            );
            return;
        }
    }
    if evidence.at.is_none() {
        note_no_time(evidence);
    }
}

/// What to do when a pass is stale only because its commit is not HEAD. This
/// is the usual case after an agent tests uncommitted work and then commits.
fn rerun_hint(pointer: &str) -> &'static str {
    if pointer.starts_with("trammel:") {
        "; fix: rerun the tests at HEAD (for example verify_step again for that step)"
    } else {
        "; fix: rerun the tests at HEAD (for example chisel run -- <test command>)"
    }
}

fn note_no_time(evidence: &mut Evidence) {
    if !evidence.detail.contains("no timestamp") {
        evidence.detail.push_str("; no timestamp");
    }
}

/// A timestamp field. The bool is true when the field is present but not a finite number.
pub fn read_time(value: Option<&crate::json::Value>) -> (Option<f64>, bool) {
    match value {
        None => (None, false),
        Some(v) => match v.as_f64() {
            Some(n) => (Some(n), false),
            None => (None, true),
        },
    }
}

/// A commit field. The bool is true when the field is present but empty or not text.
pub fn read_commit(value: Option<&crate::json::Value>) -> (Option<String>, bool) {
    match value {
        None => (None, false),
        Some(crate::json::Value::String(text)) => {
            let text = text.trim();
            if text.is_empty() {
                (None, true)
            } else {
                (Some(text.to_string()), false)
            }
        }
        Some(_) => (None, true),
    }
}

/// `YYYY-MM-DDTHH:MM:SSZ`.
pub fn valid_created(text: &str) -> bool {
    created_unix(text).is_some()
}

/// `YYYY-MM-DDTHH:MM:SSZ` as unix seconds. Anything else is `None`.
fn created_unix(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return None;
    }
    let year = number(text, 0, 4)?;
    let month = number(text, 5, 7)?;
    let day = number(text, 8, 10)?;
    let hour = number(text, 11, 13)?;
    let minute = number(text, 14, 16)?;
    let second = number(text, 17, 19)?;
    if !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Only ASCII digits: `i64::from_str` would also take a leading `+`.
fn number(text: &str, start: usize, end: usize) -> Option<i64> {
    let part = text.get(start..end)?;
    if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// Howard Hinnant's civil-from-days inverse. Days since 1970-01-01.
fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    if !(1..=12).contains(&month) {
        return None;
    }
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let month_prime = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * month_prime + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn head_commit(project: &Path) -> Result<String, String> {
    crate::cache::head(project, || head_commit_uncached(project))
}

pub fn head_commit_uncached(project: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project)
        .args(["rev-parse", "HEAD"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("git is not available ({error})"))?;
    if !output.status.success() {
        return Err("not a git repository, or HEAD could not be read".into());
    }
    let text =
        String::from_utf8(output.stdout).map_err(|_| "HEAD is not valid UTF-8".to_string())?;
    let text = text.trim();
    if text.len() < 7 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("HEAD is not a commit id".into());
    }
    Ok(text.to_string())
}

fn same_commit(head: &str, evidence: &str) -> bool {
    let head = head.trim().to_ascii_lowercase();
    let evidence = evidence.trim().to_ascii_lowercase();
    if evidence.len() < 7 || !evidence.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return false;
    }
    head == evidence || head.starts_with(&evidence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn created_times_match_known_days() {
        assert_eq!(created_unix("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(created_unix("2026-11-25T00:00:00Z"), Some(20_782 * 86_400));
        assert!(created_unix("2024-02-29T00:00:00Z").is_some());
        assert!(created_unix("2023-02-29T00:00:00Z").is_none());
        assert!(created_unix("2026-11-25T00:00:00+00:00").is_none());
        assert!(created_unix("not-a-date").is_none());
        assert!(created_unix("+026-11-25T00:00:00Z").is_none());
        assert!(created_unix("2026-+1-25T00:00:00Z").is_none());
        assert!(created_unix("2026-11-25T+1:00:00Z").is_none());
    }
}
