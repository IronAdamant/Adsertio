//! 1.4.0: a Chisel pass on uncommitted work stays fresh once exactly that
//! work is committed. Chisel 0.25.0 records `tree`, the git tree of the
//! working tree the tests ran against; when it is HEAD's tree (or differs
//! from it only in tool state such as `.adsertio` and `.chisel`), the pass
//! tested the committed code.
//!
//! The fixture `tests/fixtures/chisel/real-tree` is real output: a crate was
//! committed, analysed, edited (one changed function, one new test, and
//! cargo wrote `Cargo.lock`), then `chisel run -- cargo test` ran on the
//! dirty tree. `store.json` is the store it wrote (the project path in its
//! meta replaced by `/tmp/walk`); `files.json` is the working tree it ran
//! against. A tree id depends only on file names, modes and contents, so
//! writing those files here gives the same tree.
mod common;
use adsertio::json::{self, Value};
use common::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The tree Chisel recorded for the run in the fixture.
const TESTED_TREE: &str = "ba49cf57f39acfee44cae0e2a8f066b906ba4495";

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/chisel/real-tree")
        .join(name)
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn cli(dir: &Path, args: &[&str]) -> String {
    let out = Command::new(CLI)
        .args(args)
        .current_dir(dir)
        .env_remove("ADSERTIO_DIR")
        .output()
        .unwrap();
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn write_claim(dir: &Path, id: &str, evidence: &str) {
    fs::write(
        dir.join(format!(".adsertio/claims/{id}.md")),
        format!("---\nsummary: {id}\nauthor: agent\ncreated: 2026-01-01T00:00:00Z\nevidence: [{evidence}]\n---\n"),
    )
    .unwrap();
}

/// The tested working tree, committed without the tool folders, with the
/// store Chisel wrote and claims at its tests. HEAD is not the commit the
/// store names, but HEAD's tree is the tree it recorded.
fn tested_repo(name: &str) -> PathBuf {
    let dir = project(name);
    git(&dir, &["init", "-q"]);
    for (key, value) in [
        ("user.email", "test@example.com"),
        ("user.name", "Test"),
        ("core.autocrlf", "false"),
        ("commit.gpgsign", "false"),
    ] {
        git(&dir, &["config", key, value]);
    }
    let files = json::parse(&fs::read_to_string(fixture("files.json")).unwrap()).unwrap();
    let Value::Object(pairs) = files else {
        panic!("files.json is an object")
    };
    for (path, text) in &pairs {
        let to = dir.join(path);
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::write(to, text.as_str().unwrap()).unwrap();
    }
    init(&dir);
    fs::create_dir_all(dir.join(".chisel")).unwrap();
    fs::copy(fixture("store.json"), dir.join(".chisel/store")).unwrap();
    write_claim(
        &dir,
        "new-walk",
        "chisel:.chisel/store#test:src/tests/recursive.rs:new_walk",
    );
    write_claim(&dir, "lib", "chisel:.chisel/store#file:src/lib.rs");
    git(
        &dir,
        &[
            "add",
            "-A",
            "--",
            ".",
            ":(exclude).chisel",
            ":(exclude).adsertio",
        ],
    );
    git(&dir, &["commit", "-q", "-m", "the tested change"]);
    assert_eq!(git(&dir, &["rev-parse", "HEAD^{tree}"]), TESTED_TREE);
    dir
}

fn evidence(dir: &Path, id: &str) -> (String, String, String) {
    let body = cli(dir, &["view", "--id", id]);
    let doc = json::parse(body.trim()).unwrap_or_else(|_| panic!("{body}"));
    let claim = &doc.get("claims").unwrap().as_array().unwrap()[0];
    let ev = &claim.get("evidence").unwrap().as_array().unwrap()[0];
    let text = |value: Option<&Value>| value.and_then(Value::as_str).unwrap_or("").to_string();
    (
        text(claim.get("verdict")),
        text(ev.get("state")),
        text(ev.get("detail")),
    )
}

#[test]
fn a_dirty_pass_is_fresh_once_exactly_that_work_is_committed() {
    let dir = tested_repo("v140-exact");
    let store = fs::read_to_string(dir.join(".chisel/store")).unwrap();
    assert!(!store.contains(&git(&dir, &["rev-parse", "HEAD"])));
    for id in ["new-walk", "lib"] {
        let (verdict, state, detail) = evidence(&dir, id);
        assert_eq!(
            (verdict.as_str(), state.as_str()),
            ("verified", "pass"),
            "{detail}"
        );
        assert!(
            detail.contains(&format!(
                "tested before the commit on a tree equal to HEAD's tree {TESTED_TREE}"
            )),
            "{detail}"
        );
    }
}

#[test]
fn committing_the_claims_and_the_store_too_keeps_it_fresh() {
    let dir = tested_repo("v140-tool-state");
    // `git add -A` with nothing ignored: the claims and the Chisel store go
    // into the commit as well. Only tool state differs from the tested tree.
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "claims and store"]);
    assert_ne!(git(&dir, &["rev-parse", "HEAD^{tree}"]), TESTED_TREE);
    let (verdict, state, detail) = evidence(&dir, "new-walk");
    assert_eq!(
        (verdict.as_str(), state.as_str()),
        ("verified", "pass"),
        "{detail}"
    );
    assert!(
        detail.contains("only under .adsertio, .chisel, .trammel or .probatio"),
        "{detail}"
    );
}

#[test]
fn a_change_committed_after_the_run_is_stale() {
    let dir = tested_repo("v140-changed");
    fs::write(
        dir.join("src/lib.rs"),
        "pub fn depth(n: u32) -> u32 {\n    n + 2\n}\n\n#[cfg(test)]\nmod tests;\n",
    )
    .unwrap();
    git(&dir, &["commit", "-q", "-am", "change after the run"]);
    for id in ["new-walk", "lib"] {
        let (verdict, state, detail) = evidence(&dir, id);
        assert_eq!(
            (verdict.as_str(), state.as_str()),
            ("unverified", "stale"),
            "{detail}"
        );
        assert!(
            detail.contains(&format!("the tested tree {TESTED_TREE} is not HEAD's tree")),
            "{detail}"
        );
        assert!(
            detail.contains("; fix: rerun the tests at HEAD"),
            "{detail}"
        );
        // A tree was recorded, so the note about newer Chisel is not added.
        assert!(!detail.contains("0.25.0"), "{detail}");
    }
}

#[test]
fn an_absent_or_bad_tree_keeps_the_commit_rule() {
    let dir = tested_repo("v140-bad-tree");
    let real = fs::read_to_string(dir.join(".chisel/store")).unwrap();
    let field = format!(",\"tree\":\"{TESTED_TREE}\"");
    assert!(real.contains(&field), "the fixture records a tree");
    let cases = [
        (
            String::new(),
            "Chisel 0.25.0 or later records the tested tree",
        ),
        (
            ",\"tree\":\"not-a-tree\"".to_string(),
            "Chisel 0.25.0 or later records the tested tree",
        ),
        (
            format!(",\"tree\":\"{}\"", &TESTED_TREE[..12]),
            "Chisel 0.25.0 or later records the tested tree",
        ),
        (
            ",\"tree\":\"0123456789abcdef0123456789abcdef01234567\"".to_string(),
            "could not be compared with HEAD's",
        ),
    ];
    for (replacement, expect) in cases {
        fs::write(
            dir.join(".chisel/store"),
            real.replace(&field, &replacement),
        )
        .unwrap();
        let (verdict, state, detail) = evidence(&dir, "new-walk");
        assert_eq!(
            (verdict.as_str(), state.as_str()),
            ("unverified", "stale"),
            "{detail}"
        );
        assert!(detail.contains("is not HEAD"), "{detail}");
        assert!(detail.contains(expect), "{replacement}: {detail}");
    }
}

#[test]
fn a_failure_with_a_matching_tree_still_refutes() {
    let dir = tested_repo("v140-fail");
    let real = fs::read_to_string(dir.join(".chisel/store")).unwrap();
    let pass = "{\"test_id\":\"src/tests/recursive.rs:new_walk\",\"passed\":true";
    assert!(real.contains(pass));
    fs::write(
        dir.join(".chisel/store"),
        real.replace(
            pass,
            "{\"test_id\":\"src/tests/recursive.rs:new_walk\",\"passed\":false",
        ),
    )
    .unwrap();
    let (verdict, state, detail) = evidence(&dir, "new-walk");
    assert_eq!(
        (verdict.as_str(), state.as_str()),
        ("refuted", "fail"),
        "{detail}"
    );
}
