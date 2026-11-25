//! Agent-facing additions: a guide, page limits, and errors that say what to call next.
mod common;
use common::*;

#[test]
fn guide_names_selectors_and_does_not_need_a_store() {
    let dir = project("guide");
    let reply = talk(&dir, call("adsertio_guide", "{}", 1).as_bytes());
    assert!(is_ok(&reply[0]), "{}", reply[0]);
    assert!(reply[0].contains("plan-<id>/step-<id>"), "{}", reply[0]);
    assert!(
        reply[0].contains("probatio:<out-dir>#survivors")
            && reply[0].contains("probatio:out/run#survivors"),
        "{}",
        reply[0]
    );
    assert!(reply[0].contains("after_compaction"), "{}", reply[0]);
    assert!(reply[0].contains("adsertio_attach"), "{}", reply[0]);
    assert!(!dir.join(".adsertio").exists());
}

#[test]
fn lists_are_paged_and_counts_stay_complete() {
    let dir = project("page");
    init(&dir);
    for (id, summary) in [("a", "Alpha"), ("b", "Beta"), ("c", "Gamma")] {
        let made = talk(
            &dir,
            call(
                "adsertio_claim",
                &format!(r#"{{"summary":"{summary}","id":"{id}"}}"#),
                1,
            )
            .as_bytes(),
        );
        assert!(is_ok(&made[0]), "{}", made[0]);
    }
    let page = talk(&dir, call("adsertio_ls", r#"{"limit":2}"#, 1).as_bytes());
    assert!(is_ok(&page[0]), "{}", page[0]);
    assert!(page[0].contains(r#"\"truncated\":true"#), "{}", page[0]);
    assert!(page[0].contains(r#"\"next_offset\":2"#), "{}", page[0]);
    assert!(
        page[0].contains(r#"next: adsertio_ls with {\\\"limit\\\":2,\\\"offset\\\":2}"#),
        "{}",
        page[0]
    );
    assert!(page[0].contains(r#"\"count\":3"#), "{}", page[0]);
    assert!(
        page[0].contains("Alpha") && page[0].contains("Beta"),
        "{}",
        page[0]
    );
    assert!(!page[0].contains("Gamma"), "{}", page[0]);

    let rest = talk(
        &dir,
        call("adsertio_ls", r#"{"limit":2,"offset":2}"#, 1).as_bytes(),
    );
    assert!(rest[0].contains("Gamma"), "{}", rest[0]);
    assert!(rest[0].contains(r#"\"truncated\":false"#), "{}", rest[0]);
    assert!(rest[0].contains(r#"\"next_offset\":null"#), "{}", rest[0]);

    let all = talk(&dir, call("adsertio_ls", "{}", 1).as_bytes());
    assert!(is_ok(&all[0]), "{}", all[0]);
    assert!(
        all[0].contains("Gamma") && all[0].contains(r#"\"truncated\":false"#),
        "{}",
        all[0]
    );
    assert!(!all[0].contains("\"hint\""), "{}", all[0]);

    let viewed = talk(&dir, call("adsertio_view", r#"{"limit":1}"#, 1).as_bytes());
    assert!(viewed[0].contains(r#"\"unverified\":1"#), "{}", viewed[0]);
    assert!(
        viewed[0].contains(r#"\"counts_complete\":false"#),
        "{}",
        viewed[0]
    );
    assert!(viewed[0].contains(r#"\"truncated\":true"#), "{}", viewed[0]);
    assert!(viewed[0].contains("next: adsertio_view"), "{}", viewed[0]);
    assert!(viewed[0].contains(r#"\\\"after\\\""#), "{}", viewed[0]);

    let bad = talk(&dir, call("adsertio_ls", r#"{"limit":0}"#, 1).as_bytes());
    assert!(
        is_error(&bad[0]) && bad[0].contains("from 1 to 500"),
        "{}",
        bad[0]
    );
    let huge = talk(
        &dir,
        call("adsertio_view", r#"{"limit":501}"#, 1).as_bytes(),
    );
    assert!(is_error(&huge[0]) && huge[0].contains("500"), "{}", huge[0]);
}

#[test]
fn a_missing_id_says_what_to_call() {
    let dir = project("missing-id");
    init(&dir);
    let reply = talk(
        &dir,
        call("adsertio_view", r#"{"id":"nope"}"#, 1).as_bytes(),
    );
    assert!(is_error(&reply[0]), "{}", reply[0]);
    assert!(reply[0].contains("adsertio_ls"), "{}", reply[0]);
    assert!(reply[0].contains("adsertio_claim"), "{}", reply[0]);
}
