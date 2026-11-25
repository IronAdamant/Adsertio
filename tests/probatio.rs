//! Probatio runs: survivors refute, a clean kill run verifies, a broken
//! result file is unreadable, and one mutant is judged on its own.
mod common;
use common::*;
use std::fs;
use std::path::{Path, PathBuf};

fn copy_tree(src: &Path, dest: &Path) {
    fs::create_dir_all(dest).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dest.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), &to).unwrap();
        }
    }
}

fn install(name: &str, fixture: &str) -> PathBuf {
    let dir = project(name);
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/probatio")
        .join(fixture);
    copy_tree(&src, &dir.join("out"));
    dir
}

fn at(dir: &Path, selector: &str) -> adsertio::verdict::Evidence {
    let pointer = if selector.is_empty() {
        "probatio:out".to_string()
    } else {
        format!("probatio:out#{selector}")
    };
    adsertio::probatio::evaluate(dir, &pointer)
}

fn names(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            }
            out.push(path.display().to_string());
        }
    }
    walk(dir, &mut out);
    out.sort();
    out
}

#[test]
fn a_clean_run_passes_and_a_survivor_fails() {
    let dir = install("probatio-clean", "clean");
    let before = names(&dir);
    let pass = at(&dir, "");
    assert_eq!(
        pass.state,
        adsertio::verdict::State::Pass,
        "{}",
        pass.detail
    );
    assert!(pass.detail.contains("2 killed"), "{}", pass.detail);
    assert!(!pass.detail.contains("disagrees"), "{}", pass.detail);
    assert_eq!(at(&dir, "survivors").state, adsertio::verdict::State::Pass);
    assert_eq!(names(&dir), before);

    let dir = install("probatio-survivor", "survivor");
    let fail = at(&dir, "survivors");
    assert_eq!(
        fail.state,
        adsertio::verdict::State::Fail,
        "{}",
        fail.detail
    );
    assert!(fail.detail.contains("disagrees"), "{}", fail.detail);
    assert!(fail.detail.contains("trusted results"), "{}", fail.detail);
}

#[test]
fn a_broken_result_is_unreadable_and_one_mutant_is_judged_alone() {
    let dir = install("probatio-broken", "broken");
    let broken = at(&dir, "");
    assert_eq!(
        broken.state,
        adsertio::verdict::State::Unreadable,
        "{}",
        broken.detail
    );
    assert!(broken.detail.contains("bad.json"), "{}", broken.detail);

    let dir = install("probatio-single", "single");
    let kill = at(&dir, "mutant:m-kill");
    assert_eq!(
        kill.state,
        adsertio::verdict::State::Pass,
        "{}",
        kill.detail
    );
    let live = at(&dir, "mutant:m-live");
    assert_eq!(
        live.state,
        adsertio::verdict::State::Fail,
        "{}",
        live.detail
    );
    assert_eq!(
        at(&dir, "mutant:missing").state,
        adsertio::verdict::State::Missing
    );
}

#[test]
fn other_outcomes_do_not_pass_and_a_kill_beside_a_timeout_does() {
    let dir = project("probatio-other");
    fs::create_dir_all(dir.join("out/results")).unwrap();
    fs::write(
        dir.join("out/results/m1.json"),
        r#"{"id":"m1","outcome":"timeout"}"#,
    )
    .unwrap();
    let timeout = at(&dir, "");
    assert_eq!(
        timeout.state,
        adsertio::verdict::State::Inconclusive,
        "{}",
        timeout.detail
    );
    assert!(timeout.detail.contains("timeout"), "{}", timeout.detail);
    assert_eq!(
        at(&dir, "mutant:m1").state,
        adsertio::verdict::State::Inconclusive
    );

    fs::write(
        dir.join("out/results/m1.json"),
        r#"{"id":"m1","outcome":"killed","cause":"build"}"#,
    )
    .unwrap();
    let unviable = at(&dir, "mutant:m1");
    assert_eq!(
        unviable.state,
        adsertio::verdict::State::Inconclusive,
        "{}",
        unviable.detail
    );
    assert!(unviable.detail.contains("unviable"), "{}", unviable.detail);

    fs::write(
        dir.join("out/results/m1.json"),
        r#"{"id":"m1","outcome":"no coverage"}"#,
    )
    .unwrap();
    assert_eq!(at(&dir, "").state, adsertio::verdict::State::Inconclusive);

    fs::write(
        dir.join("out/results/m1.json"),
        r#"{"id":"m1","outcome":"killed"}"#,
    )
    .unwrap();
    fs::write(
        dir.join("out/results/m2.json"),
        r#"{"id":"m2","outcome":"timeout"}"#,
    )
    .unwrap();
    let mixed = at(&dir, "");
    assert_eq!(
        mixed.state,
        adsertio::verdict::State::Pass,
        "{}",
        mixed.detail
    );
    assert!(mixed.detail.contains("timeout"), "{}", mixed.detail);

    fs::write(dir.join("out/results/m2.json"), r#"{"id":"m2"}"#).unwrap();
    assert_eq!(at(&dir, "").state, adsertio::verdict::State::Unreadable);
}

#[test]
fn ignored_files_do_not_count_and_bad_selectors_are_refused() {
    let dir = install("probatio-tmp", "clean");
    fs::write(
        dir.join("out/results/m9.json.tmp"),
        r#"{"id":"m9","outcome":"survived"}"#,
    )
    .unwrap();
    fs::write(dir.join("out/results/notes.txt"), "survived").unwrap();
    assert_eq!(at(&dir, "").state, adsertio::verdict::State::Pass);

    let dir = install("probatio-mismatch", "mismatch");
    let mismatch = at(&dir, "");
    assert_eq!(
        mismatch.state,
        adsertio::verdict::State::Pass,
        "{}",
        mismatch.detail
    );
    assert!(mismatch.detail.contains("disagrees"), "{}", mismatch.detail);

    let dir = install("probatio-tally-only", "only-tally");
    let only = at(&dir, "");
    assert_eq!(
        only.state,
        adsertio::verdict::State::Inconclusive,
        "{}",
        only.detail
    );
    assert!(
        only.detail.contains("results were missing"),
        "{}",
        only.detail
    );
    assert_ne!(only.state, adsertio::verdict::State::Pass);

    let dir = install("probatio-ledger", "ledger");
    let ledger = at(&dir, "ledger");
    assert_eq!(
        ledger.state,
        adsertio::verdict::State::Inconclusive,
        "{}",
        ledger.detail
    );
    assert!(
        ledger.detail.contains("3 ledger entries"),
        "{}",
        ledger.detail
    );
    assert!(ledger.detail.contains("not proof"), "{}", ledger.detail);
    assert_ne!(ledger.state, adsertio::verdict::State::Pass);

    let dir = install("probatio-bad-sel", "clean");
    assert_eq!(at(&dir, "nope").state, adsertio::verdict::State::Unreadable);
    assert_eq!(
        at(&dir, "mutant:../m1").state,
        adsertio::verdict::State::Unreadable
    );
    assert_eq!(
        at(&dir, "mutant:m1/../../etc/passwd").state,
        adsertio::verdict::State::Unreadable
    );
    let file = adsertio::probatio::evaluate(&dir, "probatio:out/results/m1.json");
    assert_eq!(
        file.state,
        adsertio::verdict::State::Unreadable,
        "{}",
        file.detail
    );
}

#[cfg(unix)]
#[test]
fn a_result_symlink_outside_the_project_is_unreadable() {
    let dir = install("probatio-link", "clean");
    let outside = project("probatio-link-out");
    fs::write(outside.join("secret.json"), r#"{"outcome":"killed"}"#).unwrap();
    std::os::unix::fs::symlink(outside.join("secret.json"), dir.join("out/results/m3.json"))
        .unwrap();
    let evidence = at(&dir, "");
    assert_eq!(
        evidence.state,
        adsertio::verdict::State::Unreadable,
        "{}",
        evidence.detail
    );
    assert!(evidence.detail.contains("outside"), "{}", evidence.detail);
}

fn claim_and_view(dir: &Path, id: &str, summary: &str, selector: &str) -> String {
    let pointer = if selector.is_empty() {
        "probatio:out".to_string()
    } else {
        format!("probatio:out#{selector}")
    };
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

#[test]
fn view_matches_the_four_fixtures() {
    let dir = project("probatio-view");
    init(&dir);
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/probatio");
    copy_tree(&src.join("clean"), &dir.join("out"));
    shows(
        &claim_and_view(&dir, "clean", "Mutants were killed", ""),
        "verified",
        "pass",
    );

    fs::remove_dir_all(dir.join("out")).unwrap();
    copy_tree(&src.join("survivor"), &dir.join("out"));
    shows(
        &claim_and_view(&dir, "live", "A mutant survived", "survivors"),
        "refuted",
        "fail",
    );

    fs::remove_dir_all(dir.join("out")).unwrap();
    copy_tree(&src.join("broken"), &dir.join("out"));
    shows(
        &claim_and_view(&dir, "broken", "A result file is broken", ""),
        "unverified",
        "unreadable",
    );

    fs::remove_dir_all(dir.join("out")).unwrap();
    copy_tree(&src.join("single"), &dir.join("out"));
    shows(
        &claim_and_view(&dir, "one-kill", "This mutant was killed", "mutant:m-kill"),
        "verified",
        "pass",
    );
    shows(
        &claim_and_view(&dir, "one-live", "This mutant survived", "mutant:m-live"),
        "refuted",
        "fail",
    );
}
