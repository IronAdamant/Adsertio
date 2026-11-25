//! Trammel selectors, through the library and through `adsertio view`.
mod common;
use common::*;
use std::fs;
use std::path::Path;

const STORE: &str = include_str!("fixtures/trammel/store.json");
const CORRUPT: &str = include_str!("fixtures/trammel/corrupt.json");

fn with_store(name: &str, body: &str) -> std::path::PathBuf {
    let dir = project(name);
    fs::create_dir_all(dir.join(".trammel")).unwrap();
    fs::write(dir.join(".trammel/store"), body).unwrap();
    dir
}

fn at(dir: &Path, selector: &str) -> adsertio::verdict::Evidence {
    adsertio::trammel::evaluate(dir, &format!("trammel:.trammel/store#{selector}"))
}

#[test]
fn selectors_pass_fail_or_stay_unverified() {
    let dir = with_store("trammel-lib", STORE);
    let pass = at(&dir, "plan-1/step-1");
    assert_eq!(
        pass.state,
        adsertio::verdict::State::Pass,
        "{}",
        pass.detail
    );
    let fail = at(&dir, "plan-1/step-2");
    assert_eq!(
        fail.state,
        adsertio::verdict::State::Fail,
        "{}",
        fail.detail
    );
    let waiting = at(&dir, "plan-1/step-3");
    assert_eq!(waiting.state, adsertio::verdict::State::Missing);
    assert!(
        waiting.detail.contains("not yet verified"),
        "{}",
        waiting.detail
    );
    let plan_fail = at(&dir, "plan-1");
    assert_eq!(
        plan_fail.state,
        adsertio::verdict::State::Fail,
        "{}",
        plan_fail.detail
    );
    let plan_pass = at(&dir, "plan-2");
    assert_eq!(
        plan_pass.state,
        adsertio::verdict::State::Pass,
        "{}",
        plan_pass.detail
    );
    let empty = at(&dir, "plan-3");
    assert_eq!(empty.state, adsertio::verdict::State::Missing);
    assert!(empty.detail.contains("no steps"), "{}", empty.detail);
    let unknown_plan = at(&dir, "plan-9");
    assert_eq!(unknown_plan.state, adsertio::verdict::State::Missing);
    let active = at(&dir, "constraint-1");
    assert_eq!(
        active.state,
        adsertio::verdict::State::Pass,
        "{}",
        active.detail
    );
    let inactive = at(&dir, "constraint-2");
    assert_eq!(
        inactive.state,
        adsertio::verdict::State::Fail,
        "{}",
        inactive.detail
    );
    let default_active = at(&dir, "constraint-3");
    assert_eq!(
        default_active.state,
        adsertio::verdict::State::Pass,
        "{}",
        default_active.detail
    );
    let missing_constraint = at(&dir, "constraint-9");
    assert_eq!(missing_constraint.state, adsertio::verdict::State::Missing);
    let recorded = at(&dir, "failure-1");
    assert_eq!(
        recorded.state,
        adsertio::verdict::State::Pass,
        "{}",
        recorded.detail
    );
    let missing_failure = at(&dir, "failure-9");
    assert_eq!(missing_failure.state, adsertio::verdict::State::Missing);
    assert!(missing_failure.detail.contains("not in the store"));
    // `step-<id>` is the step's id, not its step_index. Step 7 has index 1.
    assert_eq!(
        at(&dir, "plan-4/step-1").state,
        adsertio::verdict::State::Missing
    );
    assert_eq!(
        at(&dir, "plan-4/step-7").state,
        adsertio::verdict::State::Pass
    );
    let before = fs::read(dir.join(".trammel/store")).unwrap();
    let _ = at(&dir, "plan-1/step-1");
    assert_eq!(fs::read(dir.join(".trammel/store")).unwrap(), before);
    assert!(!dir.join(".trammel/lock").exists());
}

#[test]
fn a_selector_is_required_and_unknown_selectors_are_not_proof() {
    let dir = with_store("trammel-sel", STORE);
    let bare = adsertio::trammel::evaluate(&dir, "trammel:.trammel/store");
    assert_eq!(bare.state, adsertio::verdict::State::Unreadable);
    assert!(
        bare.detail.contains("selector is required"),
        "{}",
        bare.detail
    );
    let bogus = at(&dir, "bogus");
    assert_eq!(bogus.state, adsertio::verdict::State::Unreadable);
    assert!(
        bogus.detail.contains("unknown trammel selector"),
        "{}",
        bogus.detail
    );
    let junk = at(&dir, "plan-1/step-");
    assert_eq!(junk.state, adsertio::verdict::State::Unreadable);
    let missing_file = adsertio::trammel::evaluate(&dir, "trammel:.trammel/other#plan-1");
    assert_eq!(missing_file.state, adsertio::verdict::State::Missing);
}

#[test]
fn damaged_rows_do_not_pass_or_fail() {
    let dir = with_store(
        "trammel-odd",
        r#"{"schema_version":1,"plans":[{"id":1}],"steps":[{"id":1,"plan_id":1,"verification":"yes"}],"constraints":[{"id":1,"active":"yes"}],"failures":[]}"#,
    );
    let step = at(&dir, "plan-1/step-1");
    assert_eq!(
        step.state,
        adsertio::verdict::State::Unreadable,
        "{}",
        step.detail
    );
    let plan = at(&dir, "plan-1");
    assert_eq!(
        plan.state,
        adsertio::verdict::State::Unreadable,
        "{}",
        plan.detail
    );
    let constraint = at(&dir, "constraint-1");
    assert_eq!(
        constraint.state,
        adsertio::verdict::State::Unreadable,
        "{}",
        constraint.detail
    );

    let dir = with_store(
        "trammel-string-pass",
        r#"{"schema_version":1,"plans":[],"steps":[{"id":1,"plan_id":1,"verification":{"passed":"true"}}],"constraints":[],"failures":[]}"#,
    );
    assert_eq!(
        at(&dir, "plan-1/step-1").state,
        adsertio::verdict::State::Fail
    );

    let dir = with_store("trammel-no-table", r#"{"schema_version":1,"plans":[]}"#);
    let err = at(&dir, "plan-1");
    assert_eq!(err.state, adsertio::verdict::State::Unreadable);
    assert!(err.detail.contains("steps"), "{}", err.detail);
    assert_ne!(err.state, adsertio::verdict::State::Pass);
}

#[test]
fn the_journal_is_what_the_adapter_judges() {
    let dir = with_store(
        "trammel-journal-adapter",
        r#"{"schema_version":1,"plans":[{"id":1}],"steps":[{"id":1,"plan_id":1,"verification":{"passed":false}}],"constraints":[],"failures":[]}"#,
    );
    assert_eq!(
        at(&dir, "plan-1/step-1").state,
        adsertio::verdict::State::Fail
    );
    fs::write(
        dir.join(".trammel/journal"),
        r#"{"schema_version":1,"plans":[{"id":1}],"steps":[{"id":1,"plan_id":1,"verification":{"passed":true,"at":4102444800}}],"constraints":[],"failures":[]}"#,
    )
    .unwrap();
    assert_eq!(
        at(&dir, "plan-1/step-1").state,
        adsertio::verdict::State::Pass
    );
    assert!(dir.join(".trammel/journal").is_file());
}

fn claim_and_view(dir: &std::path::Path, id: &str, summary: &str, selector: &str) -> String {
    let mut input = call(
        "adsertio_claim",
        &format!(
            r#"{{"summary":"{summary}","id":"{id}","evidence":["trammel:.trammel/store#{selector}"]}}"#
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
fn view_reports_verified_refuted_and_unverified() {
    let dir = project("trammel-view");
    init(&dir);
    fs::create_dir_all(dir.join(".trammel")).unwrap();
    fs::write(dir.join(".trammel/store"), STORE).unwrap();
    shows(
        &claim_and_view(&dir, "passed", "Step passed", "plan-1/step-1"),
        "verified",
        "pass",
    );
    shows(
        &claim_and_view(&dir, "failed", "Step failed", "plan-1/step-2"),
        "refuted",
        "fail",
    );
    let waiting = claim_and_view(&dir, "waiting", "Step waiting", "plan-1/step-3");
    shows(&waiting, "unverified", "missing");
    assert!(waiting.contains("not yet verified"), "{waiting}");
    shows(
        &claim_and_view(&dir, "inactive", "Constraint off", "constraint-2"),
        "refuted",
        "fail",
    );
    let unknown = claim_and_view(&dir, "unknown", "No such failure", "failure-9");
    shows(&unknown, "unverified", "missing");
    shows(
        &claim_and_view(&dir, "whole", "Plan two passed", "plan-2"),
        "verified",
        "pass",
    );
}

#[test]
fn a_damaged_store_verifies_nothing_and_refutes_nothing() {
    let dir = project("trammel-damaged");
    init(&dir);
    fs::create_dir_all(dir.join(".trammel")).unwrap();
    fs::write(dir.join(".trammel/store"), CORRUPT).unwrap();
    let reply = claim_and_view(&dir, "broken", "Store is broken", "plan-1/step-1");
    shows(&reply, "unverified", "unreadable");
    assert!(reply.contains(r#"\"verified\":0"#), "{reply}");
    assert!(reply.contains(r#"\"refuted\":0"#), "{reply}");
    assert!(reply.contains("not valid JSON"), "{reply}");
}
