//! A pass is not proof when it is older than the claim, or when it names a
//! commit that is not the project's HEAD, unless the tree it ran against
//! (Chisel's `tree`) is HEAD's tree: then the tested code is exactly the
//! committed code.
//!
//! Only a pass is downgraded. A failure stays a failure: old evidence that
//! the claim does not hold still refutes it. Evidence with no timestamp and
//! no commit can still pass, and the detail says `no timestamp`.
//!
//! `git` is the only external program. `git rev-parse HEAD HEAD^{tree}` runs
//! once per project per view. Only when a recorded tree is not HEAD's tree,
//! `git diff-tree` runs once per such tree, to see whether the two differ
//! only in tool state. The evidence's commit is compared here and never
//! passed to git; a tree id is passed only once it is checked to be hex.

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
        Ok((head, head_tree)) => {
            // Some(true): a tested tree is HEAD's tree; Some(false): it differs
            // from it only in tool state.
            let mut by_tree: Option<bool> = None;
            for (index, commit) in evidence.commits.iter().enumerate() {
                if same_commit(&head, commit) {
                    continue;
                }
                let tree = evidence.trees.get(index).cloned().flatten();
                let why = match &tree {
                    None => format!("stale: evidence commit {commit} is not HEAD {head}"),
                    Some(tree) if same_tree(&head_tree, tree) => {
                        by_tree = by_tree.or(Some(true));
                        continue;
                    }
                    Some(tree) => match tool_state_only(project, tree, &head_tree) {
                        Ok(true) => {
                            by_tree = Some(false);
                            continue;
                        }
                        Ok(false) => format!(
                            "stale: evidence commit {commit} is not HEAD {head}, and the tested tree {tree} is not HEAD's tree {head_tree}"
                        ),
                        Err(error) => format!(
                            "stale: evidence commit {commit} is not HEAD {head}, and the tested tree {tree} could not be compared with HEAD's ({error})"
                        ),
                    },
                };
                evidence.state = State::Stale;
                evidence.detail = format!(
                    "{why}; {}{}",
                    evidence.detail,
                    rerun_hint(&evidence.pointer, tree.is_some())
                );
                return;
            }
            match by_tree {
                Some(true) => evidence.detail.push_str(&format!(
                    "; tested before the commit on a tree equal to HEAD's tree {head_tree}, so it tested the committed code"
                )),
                Some(false) => evidence.detail.push_str(&format!(
                    "; tested before the commit on a tree that differs from HEAD's tree {head_tree} only under .adsertio, .chisel, .trammel or .probatio, so it tested the committed code"
                )),
                None => {}
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
/// A Chisel pass with no recorded tree adds that Chisel 0.25.0 or later
/// records one, so committing exactly the tested work needs no rerun.
fn rerun_hint(pointer: &str, tree_recorded: bool) -> &'static str {
    if pointer.starts_with("trammel:") {
        "; fix: rerun the tests at HEAD (for example verify_step again for that step)"
    } else if tree_recorded {
        "; fix: rerun the tests at HEAD (for example chisel run -- <test command>)"
    } else {
        "; fix: rerun the tests at HEAD (for example chisel run -- <test command>). Chisel 0.25.0 or later records the tested tree, so a pass on uncommitted work stays fresh when exactly that work is committed, with no rerun"
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

/// HEAD and HEAD's tree, from one `git rev-parse` per project per view.
fn head_commit(project: &Path) -> Result<(String, String), String> {
    crate::cache::head(project, || head_and_tree_uncached(project))
}

pub fn head_commit_uncached(project: &Path) -> Result<String, String> {
    head_and_tree_uncached(project).map(|(head, _)| head)
}

/// `git rev-parse HEAD HEAD^{tree}`: the commit and its tree id.
pub fn head_and_tree_uncached(project: &Path) -> Result<(String, String), String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project)
        .args(["rev-parse", "HEAD", "HEAD^{tree}"])
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
    let mut lines = text.lines().map(str::trim);
    let is_id = |text: &str| text.len() >= 7 && text.bytes().all(|byte| byte.is_ascii_hexdigit());
    let head = lines.next().unwrap_or("");
    if !is_id(head) {
        return Err("HEAD is not a commit id".into());
    }
    let tree = lines.next().unwrap_or("");
    if !is_id(tree) {
        return Err("HEAD has no tree id".into());
    }
    Ok((head.to_string(), tree.to_ascii_lowercase()))
}

/// Tool state, not code: Adsertio's own store and the evidence stores it
/// reads. Claims are usually written after the tests ran and committed with
/// the code, so a tested tree that differs from HEAD's only here still
/// tested the committed code.
const TOOL_STATE: [&str; 4] = [".adsertio/", ".chisel/", ".trammel/", ".probatio/"];

/// True when the tested tree and HEAD's tree differ only under [`TOOL_STATE`].
/// One `git diff-tree` per tree per view. `tree` has been checked to be a
/// full hex id, so it cannot be read as an option.
fn tool_state_only(project: &Path, tree: &str, head_tree: &str) -> Result<bool, String> {
    crate::cache::tree_diff(project, tree, || {
        let output = Command::new("git")
            .arg("-C")
            .arg(project)
            .args([
                "diff-tree",
                "-r",
                "--name-only",
                "-z",
                "--no-renames",
                tree,
                head_tree,
            ])
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .map_err(|error| format!("git is not available ({error})"))?;
        if !output.status.success() {
            return Err("git does not have that tree".into());
        }
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(text
            .split('\0')
            .filter(|path| !path.is_empty())
            .all(|path| TOOL_STATE.iter().any(|dir| path.starts_with(dir))))
    })
}

/// A recorded tree matches only as a full id: a tree is never abbreviated.
fn same_tree(head_tree: &str, evidence: &str) -> bool {
    head_tree.trim().eq_ignore_ascii_case(evidence.trim())
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
