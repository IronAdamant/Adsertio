//! Chisel selectors: latest test result, file coverage by test edge, shards.
mod common;
use common::*;
use std::fs;
use std::path::Path;

const STORE: &str = include_str!("fixtures/chisel/store.json");
const LATER_FAIL: &str = include_str!("fixtures/chisel/later-fail.json");

fn with_store(name: &str, body: &str) -> std::path::PathBuf {
    let dir = project(name);
    fs::create_dir_all(dir.join(".chisel")).unwrap();
    fs::write(dir.join(".chisel/store"), body).unwrap();
    dir
}

fn at(dir: &Path, selector: &str) -> adsertio::verdict::Evidence {
    adsertio::chisel::evaluate(dir, &format!("chisel:.chisel/store#{selector}"))
}

#[test]
fn latest_result_and_file_edges() {
    let dir = with_store("chisel-lib", STORE);
    let pass = at(&dir, "test:tests/a.rs:t1");
    assert_eq!(
        pass.state,
        adsertio::verdict::State::Pass,
        "{}",
        pass.detail
    );
    let rust_style = at(&dir, "test:tests/a.rs::t1");
    assert_eq!(
        rust_style.state,
        adsertio::verdict::State::Pass,
        "{}",
        rust_style.detail
    );
    let fail = at(&dir, "test:tests/a.rs:t2");
    assert_eq!(
        fail.state,
        adsertio::verdict::State::Fail,
        "{}",
        fail.detail
    );
    let file = at(&dir, "file:src/a.rs");
    assert_eq!(
        file.state,
        adsertio::verdict::State::Fail,
        "{}",
        file.detail
    );
    assert!(file.detail.contains("tests/a.rs:t2"), "{}", file.detail);
    let waiting = at(&dir, "file:src/b.rs");
    assert_eq!(
        waiting.state,
        adsertio::verdict::State::Missing,
        "{}",
        waiting.detail
    );
    assert!(
        waiting.detail.contains("tests/a.rs:t3"),
        "{}",
        waiting.detail
    );
    let none = at(&dir, "file:src/c.rs");
    assert_eq!(none.state, adsertio::verdict::State::Missing);
    assert!(none.detail.contains("no test edge"), "{}", none.detail);
    let before = fs::read(dir.join(".chisel/store")).unwrap();
    let _ = at(&dir, "test:tests/a.rs:t1");
    assert_eq!(fs::read(dir.join(".chisel/store")).unwrap(), before);
    assert!(!dir.join(".chisel/chisel.lock").exists());
}

#[test]
fn a_later_failing_row_refutes_a_test_that_had_passed() {
    let dir = with_store("chisel-later", LATER_FAIL);
    let fail = at(&dir, "test:tests/a.rs:t1");
    assert_eq!(
        fail.state,
        adsertio::verdict::State::Fail,
        "{}",
        fail.detail
    );
}

#[test]
fn a_file_is_verified_only_when_every_touching_test_passed() {
    let dir = with_store(
        "chisel-file-pass",
        r#"{
            "magic":"chisel-store","version":1,
            "test_results":[
                {"test_id":"tests/a.rs:t1","passed":true,"recorded_at":4102444800},
                {"test_id":"tests/a.rs:t9","passed":false,"recorded_at":4102444800}
            ],
            "test_edges":[
                {"test_id":"tests/a.rs:t1","code_id":"src/a.rs:f:function","edge_type":"call","weight":1},
                {"test_id":"tests/a.rs:t9","code_id":"src/a.rs2:f:function","edge_type":"call","weight":1}
            ]
        }"#,
    );
    assert_eq!(
        at(&dir, "file:src/a.rs").state,
        adsertio::verdict::State::Pass
    );
    assert_eq!(
        at(&dir, "file:src/a.rs2").state,
        adsertio::verdict::State::Fail
    );
    assert_eq!(
        at(&dir, "file:src/a").state,
        adsertio::verdict::State::Missing
    );
}

#[test]
fn broken_rows_and_selectors_are_not_proof() {
    let dir = with_store("chisel-sel", STORE);
    let bare = adsertio::chisel::evaluate(&dir, "chisel:.chisel/store");
    assert_eq!(bare.state, adsertio::verdict::State::Unreadable);
    assert!(
        bare.detail.contains("selector is required"),
        "{}",
        bare.detail
    );
    assert_eq!(
        at(&dir, "tests").state,
        adsertio::verdict::State::Unreadable
    );
    assert_eq!(
        at(&dir, "test:").state,
        adsertio::verdict::State::Unreadable
    );
    assert_eq!(
        at(&dir, "file:../src/a.rs").state,
        adsertio::verdict::State::Unreadable
    );

    let dir = with_store(
        "chisel-compact",
        r#"{"magic":"chisel-store","version":1,"test_results":[],"test_edges":{"table":1}}"#,
    );
    let broken = at(&dir, "file:src/a.rs");
    assert_eq!(
        broken.state,
        adsertio::verdict::State::Unreadable,
        "{}",
        broken.detail
    );
    assert!(broken.detail.contains("compact table"), "{}", broken.detail);

    let dir = with_store(
        "chisel-bad-row",
        r#"{"magic":"chisel-store","version":1,"test_results":[{"test_id":"t","passed":true},["no"],{"test_id":"t","passed":false}],"test_edges":[]}"#,
    );
    assert_eq!(
        at(&dir, "test:t").state,
        adsertio::verdict::State::Unreadable
    );

    let dir = with_store(
        "chisel-no-bool",
        r#"{"magic":"chisel-store","version":1,"test_results":[{"test_id":"t","passed":"true"}],"test_edges":[]}"#,
    );
    assert_eq!(
        at(&dir, "test:t").state,
        adsertio::verdict::State::Unreadable
    );
}

#[test]
fn shards_are_read_for_the_root_store_only() {
    let dir = project("chisel-shards");
    fs::create_dir_all(dir.join(".chisel/shards/net")).unwrap();
    fs::create_dir_all(dir.join(".chisel/shards/_unassigned")).unwrap();
    fs::write(
        dir.join(".chisel/store"),
        include_str!("fixtures/chisel/sharded-root.json"),
    )
    .unwrap();
    fs::write(
        dir.join(".chisel/shards/net/store"),
        include_str!("fixtures/chisel/sharded-net.json"),
    )
    .unwrap();
    fs::write(
        dir.join(".chisel/shards/_unassigned/store"),
        include_str!("fixtures/chisel/sharded-unassigned.json"),
    )
    .unwrap();
    assert_eq!(
        at(&dir, "test:tests/a.rs:t1").state,
        adsertio::verdict::State::Pass
    );
    assert_eq!(
        at(&dir, "test:tests/a.rs:t2").state,
        adsertio::verdict::State::Fail
    );
    assert_eq!(
        at(&dir, "test:tests/a.rs:t3").state,
        adsertio::verdict::State::Pass
    );
    assert_eq!(
        at(&dir, "file:src/a.rs").state,
        adsertio::verdict::State::Fail
    );
    let only_shard =
        adsertio::chisel::evaluate(&dir, "chisel:.chisel/shards/net/store#test:tests/a.rs:t1");
    assert_eq!(
        only_shard.state,
        adsertio::verdict::State::Missing,
        "{}",
        only_shard.detail
    );
    let shard_own =
        adsertio::chisel::evaluate(&dir, "chisel:.chisel/shards/net/store#test:tests/a.rs:t2");
    assert_eq!(shard_own.state, adsertio::verdict::State::Fail);

    fs::write(dir.join(".chisel/shards/net/store"), b"{").unwrap();
    let damaged = at(&dir, "test:tests/a.rs:t1");
    assert_eq!(
        damaged.state,
        adsertio::verdict::State::Unreadable,
        "{}",
        damaged.detail
    );
    assert!(!dir.join(".chisel/shards/net/chisel.lock").exists());
}

#[cfg(unix)]
#[test]
fn a_shard_symlink_outside_the_project_is_unreadable() {
    let dir = project("chisel-shard-link");
    let outside = project("chisel-shard-link-out");
    fs::create_dir_all(dir.join(".chisel/shards")).unwrap();
    fs::create_dir_all(outside.join("net")).unwrap();
    fs::write(
        dir.join(".chisel/store"),
        include_str!("fixtures/chisel/sharded-root.json"),
    )
    .unwrap();
    fs::write(
        outside.join("net/store"),
        include_str!("fixtures/chisel/sharded-net.json"),
    )
    .unwrap();
    std::os::unix::fs::symlink(outside.join("net"), dir.join(".chisel/shards/net")).unwrap();
    let evidence = at(&dir, "test:tests/a.rs:t1");
    assert_eq!(
        evidence.state,
        adsertio::verdict::State::Unreadable,
        "{}",
        evidence.detail
    );
    assert!(evidence.detail.contains("outside"), "{}", evidence.detail);
}

#[test]
fn overlays_are_not_proof() {
    let dir = project("chisel-overlay");
    fs::create_dir_all(dir.join(".chisel/overlays/work")).unwrap();
    fs::write(
        dir.join(".chisel/overlays/work/store"),
        include_str!("fixtures/chisel/sharded-root.json"),
    )
    .unwrap();
    let evidence = adsertio::chisel::evaluate(
        &dir,
        "chisel:.chisel/overlays/work/store#test:tests/a.rs:t1",
    );
    assert_eq!(
        evidence.state,
        adsertio::verdict::State::NoAdapter,
        "{}",
        evidence.detail
    );
    assert!(evidence.detail.contains("overlays"), "{}", evidence.detail);
}

fn claim_and_view(dir: &Path, id: &str, summary: &str, selector: &str) -> String {
    let mut input = call(
        "adsertio_claim",
        &format!(
            r#"{{"summary":"{summary}","id":"{id}","evidence":["chisel:.chisel/store#{selector}"]}}"#
        ),
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

#[test]
fn view_verifies_a_passing_test_and_refutes_a_later_failure() {
    let dir = project("chisel-view");
    init(&dir);
    fs::create_dir_all(dir.join(".chisel")).unwrap();
    fs::write(dir.join(".chisel/store"), STORE).unwrap();
    shows(
        &claim_and_view(&dir, "passed", "t1 passed", "test:tests/a.rs:t1"),
        "verified",
        "pass",
    );
    shows(
        &claim_and_view(&dir, "failed", "t2 failed", "test:tests/a.rs:t2"),
        "refuted",
        "fail",
    );
    shows(
        &claim_and_view(&dir, "file", "file a touched", "file:src/a.rs"),
        "refuted",
        "fail",
    );
    let waiting = claim_and_view(&dir, "gap", "file b untouched result", "file:src/b.rs");
    shows(&waiting, "unverified", "missing");

    fs::write(dir.join(".chisel/store"), LATER_FAIL).unwrap();
    let replies = talk(
        &dir,
        call("adsertio_view", r#"{"id":"passed"}"#, 1).as_bytes(),
    );
    shows(&replies[0], "refuted", "fail");
}
