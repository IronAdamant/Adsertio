//! A pass older than the claim, or tied to another commit, does not verify.
//! A failure still refutes when the evidence is old.
mod common;
use common::*;
use std::fs;
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn repo(name: &str) -> std::path::PathBuf {
    let dir = project(name);
    init(&dir);
    git(&dir, &["init"]);
    git(&dir, &["config", "user.email", "test@example.com"]);
    git(&dir, &["config", "user.name", "Test"]);
    fs::write(dir.join("tracked.txt"), "x").unwrap();
    git(&dir, &["add", "tracked.txt"]);
    git(&dir, &["commit", "-m", "init"]);
    dir
}

fn claim_and_view(dir: &Path, id: &str, summary: &str, pointer: &str) -> String {
    let mut input = call(
        "adsertio_claim",
        &format!(r#"{{"summary":"{summary}","id":"{id}","evidence":["{pointer}"]}}"#),
        1,
    );
    input += &call("adsertio_view", &format!(r#"{{"id":"{id}"}}"#), 2);
    let replies = talk(dir, input.as_bytes());
    assert!(is_ok(&replies[0]), "{}", replies[0]);
    replies.into_iter().nth(1).unwrap()
}

fn shows(reply: &str, verdict: &str, state: &str) {
    assert!(is_ok(reply), "{reply}");
    assert!(
        reply.contains(&format!(r#"\"verdict\":\"{verdict}\""#)),
        "{reply}"
    );
    assert!(
        reply.contains(&format!(r#"\"state\":\"{state}\""#)),
        "{reply}"
    );
}

fn trammel(dir: &Path, body: &str) {
    fs::create_dir_all(dir.join(".trammel")).unwrap();
    fs::write(dir.join(".trammel/store"), body).unwrap();
}

#[test]
fn an_old_pass_is_stale_and_a_recent_pass_verifies() {
    let dir = project("fresh-old");
    init(&dir);
    trammel(
        &dir,
        r#"{"schema_version":1,"plans":[{"id":1}],"steps":[{"id":1,"plan_id":1,"verification":{"passed":true,"at":1000,"command":"cargo test","source":"working_tree"}}],"constraints":[],"failures":[]}"#,
    );
    let reply = claim_and_view(
        &dir,
        "old",
        "Old pass",
        "trammel:.trammel/store#plan-1/step-1",
    );
    shows(&reply, "unverified", "stale");
    assert!(reply.contains("older than the claim"), "{reply}");

    let dir = project("fresh-new");
    init(&dir);
    trammel(
        &dir,
        r#"{"schema_version":1,"plans":[{"id":1}],"steps":[{"id":1,"plan_id":1,"verification":{"passed":true,"at":4102444800,"command":"cargo test","source":"working_tree"}}],"constraints":[{"id":1,"active":true}],"failures":[]}"#,
    );
    shows(
        &claim_and_view(
            &dir,
            "new",
            "Recent pass",
            "trammel:.trammel/store#plan-1/step-1",
        ),
        "verified",
        "pass",
    );
    let constraint = claim_and_view(
        &dir,
        "bare",
        "Constraint holds",
        "trammel:.trammel/store#constraint-1",
    );
    shows(&constraint, "verified", "pass");
    assert!(constraint.contains("no timestamp"), "{constraint}");
}

#[test]
fn an_old_failure_still_refutes() {
    let dir = project("fresh-fail");
    init(&dir);
    trammel(
        &dir,
        r#"{"schema_version":1,"plans":[{"id":1}],"steps":[{"id":1,"plan_id":1,"verification":{"passed":false,"at":1000,"command":"cargo test","source":"working_tree"}}],"constraints":[],"failures":[]}"#,
    );
    let reply = claim_and_view(
        &dir,
        "fail",
        "Old failure",
        "trammel:.trammel/store#plan-1/step-1",
    );
    shows(&reply, "refuted", "fail");
    assert!(!reply.contains("stale:"), "{reply}");
}

#[test]
fn a_commit_must_be_head_and_is_never_executed() {
    let dir = repo("fresh-head");
    let head = git(&dir, &["rev-parse", "HEAD"]);
    let head = head.trim();
    trammel(
        &dir,
        &format!(
            r#"{{"schema_version":1,"plans":[{{"id":1}}],"steps":[{{"id":1,"plan_id":1,"verification":{{"passed":true,"at":4102444800,"commit":"{head}"}}}}],"constraints":[],"failures":[]}}"#
        ),
    );
    shows(
        &claim_and_view(
            &dir,
            "match",
            "Same commit",
            "trammel:.trammel/store#plan-1/step-1",
        ),
        "verified",
        "pass",
    );

    let short = &head[..12];
    trammel(
        &dir,
        &format!(
            r#"{{"schema_version":1,"plans":[{{"id":1}}],"steps":[{{"id":1,"plan_id":1,"verification":{{"passed":true,"at":4102444800,"commit":"{short}"}}}}],"constraints":[],"failures":[]}}"#
        ),
    );
    // The previous claim still points at the store, which now has the short hash.
    let replies = talk(
        &dir,
        call("adsertio_view", r#"{"id":"match"}"#, 1).as_bytes(),
    );
    shows(&replies[0], "verified", "pass");

    trammel(
        &dir,
        r#"{"schema_version":1,"plans":[{"id":1}],"steps":[{"id":1,"plan_id":1,"verification":{"passed":true,"at":4102444800,"commit":"0123456789abcdef0123456789abcdef01234567"}}],"constraints":[],"failures":[]}"#,
    );
    let replies = talk(
        &dir,
        call("adsertio_view", r#"{"id":"match"}"#, 1).as_bytes(),
    );
    shows(&replies[0], "unverified", "stale");
    assert!(
        replies[0].contains("0123456789abcdef0123456789abcdef01234567"),
        "{}",
        replies[0]
    );
    assert!(replies[0].contains(head), "{}", replies[0]);

    trammel(
        &dir,
        r#"{"schema_version":1,"plans":[{"id":1}],"steps":[{"id":1,"plan_id":1,"verification":{"passed":true,"commit":"$(touch pwned)"}}],"constraints":[],"failures":[]}"#,
    );
    let replies = talk(
        &dir,
        call("adsertio_view", r#"{"id":"match"}"#, 1).as_bytes(),
    );
    shows(&replies[0], "unverified", "stale");
    assert!(!dir.join("pwned").exists());
    assert!(!dir.join(".trammel/pwned").exists());
}

#[test]
fn a_commit_cannot_be_checked_outside_a_repository() {
    let dir = project("fresh-nogit");
    init(&dir);
    trammel(
        &dir,
        r#"{"schema_version":1,"plans":[{"id":1}],"steps":[{"id":1,"plan_id":1,"verification":{"passed":true,"at":4102444800,"commit":"abcdef1"}}],"constraints":[],"failures":[]}"#,
    );
    let reply = claim_and_view(
        &dir,
        "loose",
        "Unchecked commit",
        "trammel:.trammel/store#plan-1/step-1",
    );
    shows(&reply, "unverified", "stale");
    assert!(reply.contains("could not check"), "{reply}");

    fs::write(
        dir.join(".adsertio/claims/bad-date.md"),
        "---\nsummary: Bad date\nauthor: main\ncreated: not-a-date\nevidence: [trammel:.trammel/store#plan-1/step-1]\n---\n",
    )
    .unwrap();
    let replies = talk(
        &dir,
        call("adsertio_view", r#"{"id":"bad-date"}"#, 1).as_bytes(),
    );
    shows(&replies[0], "unverified", "stale");
    assert!(replies[0].contains("created time"), "{}", replies[0]);
}

#[test]
fn an_old_chisel_pass_is_stale_and_an_old_chisel_fail_still_fails() {
    let dir = project("fresh-chisel");
    init(&dir);
    fs::create_dir_all(dir.join(".chisel")).unwrap();
    fs::write(
        dir.join(".chisel/store"),
        r#"{"magic":"chisel-store","version":1,"test_results":[{"test_id":"t","passed":true,"recorded_at":1000}],"test_edges":[]}"#,
    )
    .unwrap();
    shows(
        &claim_and_view(
            &dir,
            "old",
            "Old chisel pass",
            "chisel:.chisel/store#test:t",
        ),
        "unverified",
        "stale",
    );
    fs::write(
        dir.join(".chisel/store"),
        r#"{"magic":"chisel-store","version":1,"test_results":[{"test_id":"t","passed":false,"recorded_at":1000}],"test_edges":[]}"#,
    )
    .unwrap();
    let replies = talk(&dir, call("adsertio_view", r#"{"id":"old"}"#, 1).as_bytes());
    shows(&replies[0], "refuted", "fail");
    assert!(!replies[0].contains("stale:"), "{}", replies[0]);
}
